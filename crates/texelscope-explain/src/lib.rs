//! Explanations of traced pixels. Each one gives the chain from inputs to color, the
//! branches that decided it and the inputs that move it most.

mod compare;
mod model;
mod sensitivity;
mod text;
mod why;

use std::collections::{BTreeSet, HashMap};

pub use compare::{compare, compare_pixels};
pub use model::{
    Comparison, Decision, DecisionPair, Explanation, Link, LinkKind, Reason, Sensitivity, ValuePair,
};
pub use text::render_text;

use texelscope_interp::{BranchKind, Interpreter, Origin, Trace};
use texelscope_ir::{Override, ShaderModule, SourceSpan, Value};

#[derive(Debug, thiserror::Error)]
pub enum ExplainError {
    #[error(transparent)]
    Interp(#[from] texelscope_interp::InterpError),
}

/// Traces a pixel and explains it, including which inputs move its color most.
pub fn explain_pixel(
    interp: &Interpreter,
    pixel: [u32; 2],
    overrides: &[Override],
) -> Result<Explanation, ExplainError> {
    let trace = interp.trace_pixel(pixel, overrides)?;
    let mut explanation = explain(interp.module(), &trace);
    explanation.sensitivities = sensitivity::measure(interp, &trace, overrides)?;
    Ok(explanation)
}

/// Builds an explanation by walking back from a trace's output to its inputs. It leaves
/// sensitivities empty, since they need the interpreter to run the pixel again.
pub fn explain(module: &ShaderModule, trace: &Trace) -> Explanation {
    let cone = cone(trace);
    let mut links: Vec<(u32, Link)> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    // A value computed again by the same code in the same call, as in a loop, keeps its
    // first place and its last value. Another call or another assignment gets its own link.
    let mut push =
        |links: &mut Vec<(u32, Link)>, at: u32, key: String, link: Link| match seen.get(&key) {
            Some(&i) => {
                links[i].1.value = link.value;
                links[i].1.step = link.step;
            }
            None => {
                seen.insert(key, links.len());
                links.push((at, link));
            }
        };
    let scope = |function: texelscope_ir::FunctionRef| match function {
        texelscope_ir::FunctionRef::EntryPoint(_) => None,
        f => module.function(f).map(|(name, _)| name.to_string()),
    };

    // Keys are doubled so a store at `n`, which ran before step n, sorts ahead of it.
    for &index in &cone {
        let step = &trace.steps[index as usize];
        if let Some(origin) = &step.origin {
            let (kind, name, detail) = describe_origin(origin);
            let name = step.name.clone().unwrap_or(name);
            push(
                &mut links,
                index * 2 + 1,
                format!("{kind:?} {name}"),
                Link {
                    kind,
                    scope: scope(step.expr.function),
                    name,
                    value: step.value.clone(),
                    detail,
                    span: step.span,
                    expr: Some(step.expr),
                    step: Some(index),
                    setting: module.is_uniform(step.expr),
                },
            );
        } else if let Some(name) = &step.name {
            push(
                &mut links,
                index * 2 + 1,
                format!(
                    "let {:?} {} {}",
                    step.expr.function, step.expr.expr, step.call
                ),
                Link {
                    kind: LinkKind::Let,
                    scope: scope(step.expr.function),
                    name: name.clone(),
                    value: step.value.clone(),
                    detail: source(module, step.span),
                    span: step.span,
                    expr: Some(step.expr),
                    step: Some(index),
                    setting: module.is_uniform(step.expr),
                },
            );
        }
    }
    for store in &trace.stores {
        let Some(step) = store.step.filter(|s| cone.contains(s)) else {
            continue;
        };
        let place = store.span.map(|s| s.start).unwrap_or_default();
        let function = trace.steps.get(step as usize).map(|s| s.expr.function);
        push(
            &mut links,
            store.at * 2,
            format!("var {} {place} {}", store.target, store.call),
            Link {
                kind: LinkKind::Var,
                scope: function.and_then(scope),
                name: store.target.clone(),
                value: store.value.clone(),
                detail: format!("assigned {}", source(module, store.span)),
                span: store.span,
                // The value assigned, so it can be shown over the image or pinned.
                expr: trace.steps.get(step as usize).map(|s| s.expr),
                step: Some(step),
                setting: false,
            },
        );
    }
    links.sort_by_key(|(at, _)| *at);

    Explanation {
        pixel: trace.pixel,
        color: trace.color().cloned().unwrap_or(Value::Vector(Vec::new())),
        discarded: trace.discarded,
        why: why::why(module, trace),
        links: links.into_iter().map(|(_, link)| link).collect(),
        decisions: decisions(module, trace),
        sensitivities: Vec::new(),
        notes: trace.notes.clone(),
    }
}

/// Every step the output depends on: what it was computed from, and the conditions that
/// chose which code computed it.
fn cone(trace: &Trace) -> BTreeSet<u32> {
    let mut cone = BTreeSet::new();
    let mut stack: Vec<u32> = trace.result.into_iter().chain(trace.result_guard).collect();
    while let Some(index) = stack.pop() {
        if cone.insert(index)
            && let Some(step) = trace.steps.get(index as usize)
        {
            stack.extend(step.inputs.iter().copied());
            stack.extend(step.guard);
        }
    }
    cone
}

/// Decisions grouped by condition, each with its last outcome and how often it ran.
fn decisions(module: &ShaderModule, trace: &Trace) -> Vec<Decision> {
    let mut decisions: Vec<Decision> = Vec::new();
    let mut index: HashMap<texelscope_ir::ExprRef, usize> = HashMap::new();
    for branch in &trace.branches {
        if branch.kind == BranchKind::Switch {
            continue;
        }
        let decision = Decision {
            condition: source(module, branch.span),
            taken: branch.taken,
            margin: branch.margin,
            span: branch.span,
            expr: Some(branch.condition),
            count: 1,
            forced: branch.forced,
            setting: module.is_uniform(branch.condition),
        };
        match index.get(&branch.condition) {
            Some(&i) => {
                let count = decisions[i].count + 1;
                decisions[i] = Decision { count, ..decision };
            }
            None => {
                index.insert(branch.condition, decisions.len());
                decisions.push(decision);
            }
        }
    }
    decisions
}

fn describe_origin(origin: &Origin) -> (LinkKind, String, String) {
    match origin {
        Origin::Buffer {
            group,
            binding,
            path,
            ..
        } => (
            LinkKind::Buffer,
            path.clone(),
            format!("read from @group({group}) @binding({binding})"),
        ),
        Origin::Texels {
            name,
            level,
            texels,
            ..
        } => {
            let shown: Vec<String> = texels
                .iter()
                .take(4)
                .map(|[x, y, z]| {
                    if *z == 0 {
                        format!("({x}, {y})")
                    } else {
                        format!("({x}, {y}, {z})")
                    }
                })
                .collect();
            let more = if texels.len() > 4 { ", ..." } else { "" };
            let at = if *level > 0 {
                format!(" at level {level}")
            } else {
                String::new()
            };
            (
                LinkKind::Texture,
                format!("{name} {}", shown.join(" ")),
                format!(
                    "{} {}{more}{at}",
                    if texels.len() == 1 { "texel" } else { "texels" },
                    shown.join(", ")
                ),
            )
        }
        Origin::Input { name, .. } => (LinkKind::Input, name.clone(), "fragment input".to_string()),
    }
}

/// The source a span covers, on one line and at most 64 characters.
fn source(module: &ShaderModule, span: Option<SourceSpan>) -> String {
    let Some(span) = span else {
        return String::new();
    };
    let text = module
        .text(span)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() > 64 {
        format!("{}...", text.chars().take(61).collect::<String>())
    } else {
        text
    }
}
