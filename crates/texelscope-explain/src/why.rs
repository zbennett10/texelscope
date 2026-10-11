//! A short account of why a pixel has its color, in a few plain sentences.
//!
//! It walks back from the returned color through steps that only pass a value along, such as
//! loads, call results, the chosen side of a select and a mix at 0 or 1. On the way it notes
//! where the value was read and any clamp, min, max or smoothstep stuck at a bound, which is
//! where the inputs before it stop mattering. Then it names the branches that chose the path,
//! and the per-pixel weight or condition when a mix or select kept a constant whole.

use texelscope_interp::{Branch, Origin, Step, Trace};
use texelscope_ir::naga::{
    self, BinaryOperator, Expression, Handle, Literal, MathFunction, TypeInner,
};
use texelscope_ir::{ExprRef, ShaderModule, SourceSpan, Value};

use crate::model::Reason;
use crate::text::number;

/// What the walk back from a pixel's color found.
pub(crate) struct Analysis {
    /// Why the pixel has its color, most telling first.
    pub(crate) reasons: Vec<Reason>,
    /// Named values and reads on the way to the color, the color end first.
    pub(crate) chain: Vec<u32>,
    /// A value held at a bound: its step, the function holding it and the bound.
    pub(crate) held: Option<(u32, &'static str, f64)>,
    /// The conditions of the branches that chose the path to the color.
    pub(crate) path: Vec<ExprRef>,
}

/// The reasons a traced pixel has its color, most telling first.
pub(crate) fn why(module: &ShaderModule, trace: &Trace) -> Vec<Reason> {
    analyze(module, trace).reasons
}

pub(crate) fn analyze(module: &ShaderModule, trace: &Trace) -> Analysis {
    let ctx = Context { module, trace };
    let (reasons, walk) = reasons(&ctx);
    let walk = walk.unwrap_or_default();
    let named = |s: &u32| {
        let step = &trace.steps[*s as usize];
        step.name.is_some() || step.origin.is_some()
    };
    let mut chain: Vec<u32> = walk.steps.iter().copied().filter(named).collect();
    // Past a value held at a bound, the values feeding it still differ between pixels.
    if let Some(upstream) = walk.held.as_ref().and_then(|h| h.upstream) {
        let beyond = ctx.follow(upstream);
        chain.extend(beyond.steps.iter().copied().filter(named));
    }
    let exit = ctx.branch_of(trace.result_guard);
    let mut path: Vec<ExprRef> = exit.map(|b| b.condition).into_iter().collect();
    path.extend(ctx.path_branches(&walk, exit).iter().map(|b| b.condition));
    path.extend(ctx.kept_branch(&walk).map(|(b, ..)| b.condition));
    Analysis {
        reasons,
        chain,
        held: walk.held.map(|h| (h.step, h.function, h.bound)),
        path,
    }
}

fn reasons(ctx: &Context<'_>) -> (Vec<Reason>, Option<Walk>) {
    let trace = ctx.trace;
    let mut reasons = Vec::new();
    let exit = ctx.branch_of(trace.result_guard);
    if trace.discarded {
        reasons.push(match exit {
            Some(b) => Reason::at(
                b.span,
                format!("The pixel was discarded because {}.", ctx.outcome(b)),
            ),
            None => Reason::at(None, "The pixel was discarded.".to_string()),
        });
        return (reasons, None);
    }
    if let Some(b) = exit {
        reasons.push(Reason::at(
            b.span,
            format!(
                "It returned early, on line {}, because {}.",
                ctx.line(b.span),
                ctx.outcome(b)
            ),
        ));
    }
    let Some(result) = trace.result else {
        reasons.push(Reason::at(
            None,
            "The color is a constant the shader returns.".to_string(),
        ));
        return (reasons, None);
    };
    let walk = ctx.follow(result);
    if let Some(blend) = &walk.blend {
        reasons.push(ctx.blend_reason(blend));
    }
    reasons.push(ctx.source(&walk));
    reasons.extend(ctx.choice(&walk));
    if let Some(held) = &walk.held {
        reasons.push(ctx.held_reason(held));
    }
    if let Some(path) = ctx.path(&walk, exit) {
        reasons.push(path);
    }
    if let Some(kept) = ctx.kept(&walk) {
        reasons.push(kept);
    }
    if trace.truncated {
        reasons.push(Reason::at(
            None,
            "The trace stopped at the step limit, so this account may miss the end.".to_string(),
        ));
    }
    (reasons, Some(walk))
}

/// Where the color's value was made.
enum Producer {
    /// A read from a texture or buffer.
    Read(u32),
    /// A fragment input, unchanged.
    Input(u32),
    /// A step that combines several per-pixel values.
    Combined(u32),
    /// A step computed only from settings and constants.
    Settings(u32),
    /// A named value the walk stopped at, such as one held at a bound.
    Value(u32),
    Constant,
}

/// A mix that moves the color part of the way toward a setting, such as a grid line's color.
struct Blend {
    step: u32,
    toward: Handle<Expression>,
    weight: Value,
}

/// A mix by exactly 0 or 1, or a select, that kept one side whole, and the per-pixel value that
/// chose that side.
struct Choice {
    step: u32,
    function: &'static str,
    kept: Handle<Expression>,
    by: u32,
}

/// A clamp, min, max or smoothstep whose result sits at a bound, so its input no longer matters.
struct Held {
    step: u32,
    function: &'static str,
    bound: f64,
    input: f64,
    /// The nearest named value feeding the held input.
    upstream: Option<u32>,
}

#[derive(Default)]
struct Walk {
    steps: Vec<u32>,
    /// Stores the walk followed back through a variable, by index in the trace.
    stores: Vec<usize>,
    producer: Option<Producer>,
    /// The first call the walk left the color through, before reaching the producer.
    call: Option<u32>,
    /// The first named value after the producer, which picks what it reads.
    key: Option<u32>,
    held: Option<Held>,
    /// The first partial blend toward a setting, nearest the color.
    blend: Option<Blend>,
    /// The first mix or select nearest the color that kept one side whole.
    choice: Option<Choice>,
    /// Steps that passed a value through unchanged, such as a mix by 0, whose branches don't
    /// explain anything.
    unchanged: Vec<u32>,
    /// A variable read before anything assigned it, so it still held its initial value.
    initial: Option<String>,
}

struct Context<'a> {
    module: &'a ShaderModule,
    trace: &'a Trace,
}

impl Context<'_> {
    fn step(&self, i: u32) -> &Step {
        &self.trace.steps[i as usize]
    }

    fn follow(&self, start: u32) -> Walk {
        let mut walk = Walk::default();
        let mut member: Option<String> = None;
        let mut at = Some(start);
        while let Some(i) = at.take() {
            if walk.steps.len() > 4096 || walk.steps.contains(&i) {
                break;
            }
            walk.steps.push(i);
            let step = self.step(i);
            if walk.producer.is_some() && walk.key.is_none() && step.name.is_some() {
                walk.key = Some(i);
            }
            match &step.origin {
                Some(Origin::Input { .. }) => {
                    walk.producer.get_or_insert(Producer::Input(i));
                    break;
                }
                Some(Origin::Buffer { .. } | Origin::Texels { .. }) => {
                    // A read's coordinates or index can still be held at a bound.
                    walk.producer.get_or_insert(Producer::Read(i));
                    at = self.single_varying(i);
                    continue;
                }
                None => {}
            }
            let Some((f, h)) = self.module.expression(step.expr) else {
                break;
            };
            at = match &f.expressions[h] {
                Expression::Load { pointer } => {
                    self.stored(i, f, *pointer, member.take(), &mut walk)
                }
                Expression::CallResult(_) => {
                    if walk.producer.is_none() && walk.call.is_none() {
                        walk.call = Some(i);
                    }
                    step.inputs.last().copied()
                }
                Expression::AccessIndex { base, index } => {
                    if let Some(name) = self.member_name(step.expr, *base, *index) {
                        member = Some(name);
                    }
                    self.operand(i, *base)
                }
                Expression::Select {
                    condition,
                    accept,
                    reject,
                } => {
                    let by = self.operand(i, *condition);
                    match by.map(|c| &self.step(c).value) {
                        Some(&Value::Bool(taken)) => {
                            let kept = if taken { *accept } else { *reject };
                            self.note_choice(&mut walk, i, "select", kept, by);
                            self.operand(i, kept)
                        }
                        _ => self.next(i, &mut walk),
                    }
                }
                Expression::Math {
                    fun: MathFunction::Mix,
                    arg,
                    arg1: Some(b),
                    arg2: Some(t),
                    ..
                } => {
                    let weight = self.operand_value(i, f, *t);
                    let even = weight.as_ref().and_then(all_equal);
                    let varies = |handle| {
                        self.operand(i, handle)
                            .is_some_and(|s| !self.module.is_uniform(self.step(s).expr))
                    };
                    if even == Some(0.0) || even == Some(1.0) {
                        let kept = if even == Some(0.0) { *arg } else { *b };
                        walk.unchanged.push(i);
                        self.note_choice(&mut walk, i, "mix", kept, self.operand(i, *t));
                        self.operand(i, kept)
                    } else if varies(*arg) != varies(*b) && walk.producer.is_none() {
                        // Part of the way toward a setting: note it and explain the other side.
                        let (from, toward) = if varies(*arg) { (*arg, *b) } else { (*b, *arg) };
                        if walk.blend.is_none() {
                            walk.blend = weight.map(|weight| Blend {
                                step: i,
                                toward,
                                weight,
                            });
                        }
                        self.operand(i, from)
                    } else {
                        self.next(i, &mut walk)
                    }
                }
                Expression::Math { .. } => match self.held_at(i, f, h) {
                    Some(held) => {
                        walk.producer.get_or_insert(Producer::Value(i));
                        walk.held = Some(held);
                        break;
                    }
                    None => self.next(i, &mut walk),
                },
                _ => self.next(i, &mut walk),
            };
        }
        walk.producer.get_or_insert(Producer::Constant);
        walk
    }

    /// The one per-pixel input to follow, or none when several combine or none varies.
    fn next(&self, i: u32, walk: &mut Walk) -> Option<u32> {
        let varying = self.varying_inputs(i);
        match varying.as_slice() {
            [] => {
                let producer = if self.module.is_uniform(self.step(i).expr) {
                    Producer::Settings(i)
                } else {
                    Producer::Constant
                };
                walk.producer.get_or_insert(producer);
                None
            }
            [only] => Some(*only),
            several => {
                // A vector built from the parts of one value, as naga's GLSL front end writes
                // `vec4(v, 1.0)`, is that value.
                if let Some(base) = self.parts_of(several) {
                    return Some(base);
                }
                // Inputs that all come from one named value are that value.
                let root = self.root(several[0]);
                if several.iter().all(|&s| self.root(s) == root) {
                    return Some(root);
                }
                walk.producer.get_or_insert(Producer::Combined(i));
                None
            }
        }
    }

    /// The one value that every step reads a component of, when they all read the same one.
    fn parts_of(&self, steps: &[u32]) -> Option<u32> {
        let base = |s: u32| {
            let (f, h) = self.module.expression(self.step(s).expr)?;
            match f.expressions[h] {
                Expression::AccessIndex { base, .. } => self.operand(s, base),
                Expression::Swizzle { vector, .. } => self.operand(s, vector),
                _ => None,
            }
        };
        let first = base(*steps.first()?)?;
        steps
            .iter()
            .all(|&s| base(s) == Some(first))
            .then_some(first)
    }

    /// Notes the first mix or select that kept one side whole because of a per-pixel value.
    fn note_choice(
        &self,
        walk: &mut Walk,
        i: u32,
        function: &'static str,
        kept: Handle<Expression>,
        by: Option<u32>,
    ) {
        let by = by.filter(|&s| !self.module.is_uniform(self.step(s).expr));
        if let (None, Some(by)) = (&walk.choice, by) {
            walk.choice = Some(Choice {
                step: i,
                function,
                kept,
                by,
            });
        }
    }

    fn single_varying(&self, i: u32) -> Option<u32> {
        match self.varying_inputs(i).as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// Inputs that change from pixel to pixel, leaving out settings and branch conditions.
    fn varying_inputs(&self, i: u32) -> Vec<u32> {
        let mut inputs: Vec<u32> = self
            .step(i)
            .inputs
            .iter()
            .copied()
            .filter(|&j| {
                let s = self.step(j);
                !self.module.is_uniform(s.expr) && !matches!(s.value, Value::Bool(_))
            })
            .collect();
        inputs.dedup();
        inputs
    }

    /// The nearest named value or read that a step passes along.
    fn root(&self, mut i: u32) -> u32 {
        for _ in 0..256 {
            let step = self.step(i);
            if step.name.is_some() || step.origin.is_some() || self.stored_into(i).is_some() {
                return i;
            }
            match self.single_varying(i) {
                Some(next) => i = next,
                None => return i,
            }
        }
        i
    }

    /// The step of one operand of step `i`, which may be a caller's step for a parameter.
    fn operand(&self, i: u32, handle: Handle<Expression>) -> Option<u32> {
        let step = self.step(i);
        let (f, _) = self.module.expression(step.expr)?;
        if let Expression::FunctionArgument(k) = f.expressions[handle] {
            let call = self.trace.calls.get(step.call as usize)?;
            return call.args.get(k as usize).copied().flatten();
        }
        let want = handle.index() as u32;
        step.inputs.iter().copied().find(|&j| {
            let s = self.step(j);
            s.expr.function == step.expr.function && s.expr.expr == want && s.call == step.call
        })
    }

    /// An operand's value, from its step or from the constant it is.
    fn operand_value(
        &self,
        i: u32,
        f: &naga::Function,
        handle: Handle<Expression>,
    ) -> Option<Value> {
        match self.operand(i, handle) {
            Some(j) => Some(self.step(j).value.clone()),
            None => constant(self.module, f, handle).map(Value::F32),
        }
    }

    /// Follows a load back to the last assignment that wrote what it reads.
    fn stored(
        &self,
        i: u32,
        f: &naga::Function,
        pointer: Handle<Expression>,
        member: Option<String>,
        walk: &mut Walk,
    ) -> Option<u32> {
        let step = self.step(i);
        let mut path = pointer_path(self.module, step.expr, f, pointer)?;
        if let Some(member) = member {
            path = format!("{path}.{member}");
        }
        let covers = |target: &str| {
            target == path
                || path.starts_with(&format!("{target}."))
                || path.starts_with(&format!("{target}["))
        };
        let inside = |target: &str| target.starts_with(&format!("{path}."));
        // A global can be written in any call, as GLSL's outputs are inside `main`.
        let global = root_is_global(f, pointer);
        let candidates = self
            .trace
            .stores
            .iter()
            .enumerate()
            .filter(|(_, s)| s.at <= i && (global || s.call == step.call));
        let found = candidates
            .clone()
            .filter(|(_, s)| covers(&s.target))
            .max_by_key(|(_, s)| s.at)
            .or_else(|| {
                candidates
                    .filter(|(_, s)| inside(&s.target))
                    .max_by_key(|(_, s)| s.at)
            });
        let Some((index, store)) = found else {
            // Nothing assigned it yet, so it holds the value it was declared with.
            walk.initial = path.split(['.', '[']).next().map(str::to_string);
            walk.producer.get_or_insert(Producer::Constant);
            return None;
        };
        walk.stores.push(index);
        if store.step.is_none() {
            walk.producer.get_or_insert(Producer::Constant);
        }
        store.step
    }

    fn member_name(&self, at: ExprRef, base: Handle<Expression>, index: u32) -> Option<String> {
        let base = ExprRef {
            function: at.function,
            expr: base.index() as u32,
        };
        let types = &self.module.naga().types;
        let inner = match self.module.type_of(base)? {
            TypeInner::Pointer { base, .. } => &types[*base].inner,
            other => other,
        };
        match inner {
            TypeInner::Struct { members, .. } => members.get(index as usize)?.name.clone(),
            _ => None,
        }
    }

    /// Whether a math step is stuck at a bound, so its input no longer moves it.
    fn held_at(&self, i: u32, f: &naga::Function, h: Handle<Expression>) -> Option<Held> {
        let Expression::Math {
            fun,
            arg,
            arg1,
            arg2,
            ..
        } = f.expressions[h]
        else {
            return None;
        };
        let value = |handle| self.operand_value(i, f, handle).as_ref().and_then(scalar);
        let result = scalar(&self.step(i).value)?;
        let held = |function, bound, input, varying: Handle<Expression>| {
            Some(Held {
                step: i,
                function,
                bound,
                input,
                upstream: self.operand(i, varying).map(|s| self.root(s)),
            })
        };
        match fun {
            MathFunction::Clamp => {
                let (x, lo, hi) = (value(arg)?, value(arg1?)?, value(arg2?)?);
                if x < lo || x > hi {
                    return held("clamp", result, x, arg);
                }
            }
            MathFunction::Saturate => {
                let x = value(arg)?;
                if !(0.0..=1.0).contains(&x) {
                    return held("saturate", result, x, arg);
                }
            }
            MathFunction::SmoothStep => {
                let (e0, e1, x) = (value(arg)?, value(arg1?)?, value(arg2?)?);
                if x <= e0.min(e1) || x >= e0.max(e1) {
                    return held("smoothstep", result, x, arg2?);
                }
            }
            MathFunction::Min | MathFunction::Max => {
                let b = arg1?;
                let (va, vb) = (value(arg)?, value(b)?);
                let varies = |handle| {
                    self.operand(i, handle)
                        .is_some_and(|s| !self.module.is_uniform(self.step(s).expr))
                };
                let name = if fun == MathFunction::Min {
                    "min"
                } else {
                    "max"
                };
                // The other side wins, so the per-pixel side no longer matters.
                if va != vb && result == vb && varies(arg) && !varies(b) {
                    return held(name, result, va, arg);
                }
                if va != vb && result == va && varies(b) && !varies(arg) {
                    return held(name, result, vb, b);
                }
            }
            _ => {}
        }
        None
    }

    fn source(&self, walk: &Walk) -> Reason {
        let keyed = match walk.key {
            Some(k) if walk.held.as_ref().is_none_or(|h| h.step != k) => format!(
                ", picked by `{}` = {}",
                self.name(k),
                value_text(&self.step(k).value)
            ),
            _ => String::new(),
        };
        match walk.producer.as_ref() {
            Some(Producer::Read(i)) => {
                let step = self.step(*i);
                let what = match &step.origin {
                    Some(Origin::Texels { name, texels, .. }) => texel_text(name, texels),
                    Some(Origin::Buffer { path, .. }) if self.module.is_uniform(step.expr) => {
                        format!("the setting `{path}`")
                    }
                    Some(Origin::Buffer { path, .. }) => format!("`{path}` in a buffer"),
                    _ => self.name(*i),
                };
                let line = self.line(step.span);
                let text = match walk.call {
                    Some(call) => format!(
                        "The color comes from `{}`, which reads {what} on line {line}{keyed}.",
                        self.code(self.step(call).span)
                    ),
                    None => format!("The color comes from {what}, read on line {line}{keyed}."),
                };
                Reason::at(step.span, text)
            }
            Some(Producer::Input(i)) => {
                let step = self.step(*i);
                Reason::at(
                    step.span,
                    format!(
                        "The color comes from the fragment input `{}`.",
                        self.name(*i)
                    ),
                )
            }
            Some(Producer::Value(i)) => {
                let step = self.step(*i);
                Reason::at(
                    step.span,
                    format!(
                        "The color comes from `{}`, on line {}.",
                        self.name(*i),
                        self.line(step.span)
                    ),
                )
            }
            Some(Producer::Combined(i)) => {
                let step = self.step(*i);
                Reason::at(
                    step.span,
                    format!(
                        "The color is worked out on line {} from several values: `{}`.",
                        self.line(step.span),
                        self.code(step.span)
                    ),
                )
            }
            Some(Producer::Settings(i)) => {
                let step = self.step(*i);
                let text = if step.inputs.is_empty() {
                    "The color is the constant"
                } else {
                    "The color comes only from settings and constants:"
                };
                Reason::at(
                    step.span,
                    format!(
                        "{text} `{}` on line {}.",
                        self.code(step.span),
                        self.line(step.span)
                    ),
                )
            }
            Some(Producer::Constant) | None => {
                let span = walk.stores.last().and_then(|&s| self.trace.stores[s].span);
                let text = match span {
                    Some(_) => format!(
                        "The color is a constant, assigned on line {}.",
                        self.line(span)
                    ),
                    None => "The color is a constant.".to_string(),
                };
                Reason::at(span, text)
            }
        }
    }

    /// Why a mix or select kept one side whole. Only worth saying when that side is a constant
    /// or a setting, so the choice is what differs from pixel to pixel.
    fn choice(&self, walk: &Walk) -> Vec<Reason> {
        let Some(choice) = &walk.choice else {
            return Vec::new();
        };
        let same_everywhere = match walk.producer {
            Some(Producer::Settings(_) | Producer::Constant) | None => true,
            Some(Producer::Read(i) | Producer::Value(i) | Producer::Combined(i)) => {
                self.module.is_uniform(self.step(i).expr)
            }
            Some(Producer::Input(_)) => false,
        };
        if !same_everywhere {
            return Vec::new();
        }
        let step = self.step(choice.step);
        // naga's GLSL front end spans a mix's splatted weight as the whole call.
        let span = step.span.or(self.step(choice.by).span);
        let kept = ExprRef {
            function: step.expr.function,
            expr: choice.kept.index() as u32,
        };
        let kept = self.code(self.module.span_of(kept));
        let by = self.unsplat(choice.by);
        let mut reasons = vec![Reason::at(
            span,
            format!(
                "`{}` on line {} keeps only `{kept}`, because `{}` is {}.",
                choice.function,
                self.line(span),
                self.name(by),
                value_text(&self.step(by).value)
            ),
        )];
        reasons.extend(self.settled(by));
        reasons
    }

    /// The value a splat repeats, or the step itself.
    fn unsplat(&self, i: u32) -> u32 {
        match self.module.expression(self.step(i).expr) {
            Some((f, h)) => match f.expressions[h] {
                Expression::Splat { value, .. } => self.operand(i, value).unwrap_or(i),
                _ => i,
            },
            None => i,
        }
    }

    /// Why a weight is exactly 0 or 1: the factor that zeroes a product, the comparison inside
    /// a `step`, or the bound something is held at.
    fn settled(&self, start: u32) -> Option<Reason> {
        let mut i = start;
        for _ in 0..64 {
            let step = self.step(i);
            let (f, h) = self.module.expression(step.expr)?;
            i = match f.expressions[h] {
                Expression::Load { pointer } => {
                    self.stored(i, f, pointer, None, &mut Walk::default())?
                }
                Expression::Splat { value, .. } => self.operand(i, value)?,
                Expression::Binary {
                    op: BinaryOperator::Multiply,
                    left,
                    right,
                } if all_equal(&step.value) == Some(0.0) => {
                    let zero = |side| {
                        self.operand_value(i, f, side).as_ref().and_then(all_equal) == Some(0.0)
                    };
                    let side = if zero(left) {
                        left
                    } else if zero(right) {
                        right
                    } else {
                        return None;
                    };
                    self.operand(i, side)?
                }
                Expression::Math {
                    fun: MathFunction::Step,
                    arg: edge,
                    arg1: Some(x),
                    ..
                } => {
                    let e = self.operand_value(i, f, edge).as_ref().and_then(scalar)?;
                    let v = self.operand_value(i, f, x).as_ref().and_then(scalar)?;
                    let side = if v < e { "below" } else { "at or above" };
                    let span = self.place(i);
                    return Some(Reason::at(
                        span,
                        format!(
                            "`{}` on line {} is {}, because `{}` ({}) is {side} `{}` ({}).",
                            self.code(span),
                            self.line(span),
                            value_text(&step.value),
                            self.operand_name(i, f, x),
                            number(v),
                            self.operand_name(i, f, edge),
                            number(e)
                        ),
                    ));
                }
                Expression::Math { .. } => {
                    return self.held_at(i, f, h).map(|held| self.held_reason(&held));
                }
                _ => return None,
            };
        }
        None
    }

    /// What an operand is called: the name of its value, or the constant it is.
    fn operand_name(&self, i: u32, f: &naga::Function, handle: Handle<Expression>) -> String {
        match self.operand(i, handle) {
            Some(s) => self.name(s),
            None => constant(self.module, f, handle)
                .map(|v| number(f64::from(v)))
                .unwrap_or_default(),
        }
    }

    fn blend_reason(&self, blend: &Blend) -> Reason {
        let step = self.step(blend.step);
        let toward = ExprRef {
            function: step.expr.function,
            expr: blend.toward.index() as u32,
        };
        let toward = self
            .module
            .span_of(toward)
            .map(|span| self.code(Some(span)))
            .unwrap_or_else(|| "a setting".to_string());
        Reason::at(
            step.span,
            format!(
                "On line {} the color is blended toward `{toward}` by {}.",
                self.line(step.span),
                value_text(&blend.weight)
            ),
        )
    }

    /// A branch that didn't run, whose code would have changed the variable the color came
    /// from. Only worth saying when the color is a default left in place.
    fn kept(&self, walk: &Walk) -> Option<Reason> {
        let (branch, root, kept) = self.kept_branch(walk)?;
        Some(Reason::at(
            branch.span,
            format!(
                "`{}` was {}, so the code inside it that changes `{root}` didn't run, and `{root}` kept {kept}.",
                self.code(branch.span),
                branch.taken,
            ),
        ))
    }

    /// The branch that left a default color in place, with the variable and what it kept.
    fn kept_branch(&self, walk: &Walk) -> Option<(&Branch, String, String)> {
        if !matches!(
            walk.producer,
            Some(Producer::Settings(_) | Producer::Constant)
        ) {
            return None;
        }
        let (root, after, kept) = match (walk.stores.last(), &walk.initial) {
            (Some(&last), _) => {
                let store = &self.trace.stores[last];
                let root = store.target.split(['.', '[']).next()?.to_string();
                let kept = format!("its value from line {}", self.line(store.span));
                (root, Some(store.at), kept)
            }
            (None, Some(root)) => (root.clone(), None, "its initial value".to_string()),
            (None, None) => return None,
        };
        let branch = self.trace.branches.iter().find(|b| {
            after.is_none_or(|at| b.at > at)
                && b.kind == texelscope_interp::BranchKind::If
                && !self.module.is_uniform(b.condition)
                && self.skipped_store(b, &root)
        })?;
        Some((branch, root, kept))
    }

    /// Whether the side of an `if` that didn't run assigns to the variable `root`.
    fn skipped_store(&self, branch: &Branch, root: &str) -> bool {
        let Some((f, condition)) = self.module.expression(branch.condition) else {
            return false;
        };
        let Some((accept, reject)) = find_if(&f.body, condition) else {
            return false;
        };
        let skipped = if branch.taken { reject } else { accept };
        stores_to(self.module, branch.condition, f, skipped, root)
    }

    fn held_reason(&self, held: &Held) -> Reason {
        let span = self.place(held.step);
        let after = match held.upstream {
            Some(u) => format!(
                " So `{}` ({}) does not change the color here.",
                self.name(u),
                value_text(&self.step(u).value)
            ),
            None => " So the values before it do not change the color here.".to_string(),
        };
        Reason::at(
            span,
            format!(
                "`{}` is held at {} by `{}` on line {}, because the value going in is {}.{after}",
                self.name(held.step),
                number(held.bound),
                held.function,
                self.line(span),
                number(held.input),
            ),
        )
    }

    /// The per-pixel branches that chose the path the color took, as a sentence.
    fn path(&self, walk: &Walk, exit: Option<&Branch>) -> Option<Reason> {
        let mut branches = self.path_branches(walk, exit);
        if branches.is_empty() {
            return None;
        }
        branches.truncate(3);
        let parts: Vec<String> = branches
            .iter()
            .map(|b| format!("`{}` was {}", self.code(b.span), b.taken))
            .collect();
        Some(Reason::at(
            branches[0].span,
            format!("This path ran because {}.", join(&parts)),
        ))
    }

    /// The per-pixel branches that chose the path the color took, in the order they ran.
    fn path_branches(&self, walk: &Walk, exit: Option<&Branch>) -> Vec<&Branch> {
        let changed = |s: &u32| !walk.unchanged.contains(s);
        // A load reads whatever was stored, so the branch around it explains nothing.
        let computes = |&s: &u32| {
            !self
                .module
                .expression(self.step(s).expr)
                .is_some_and(|(f, h)| matches!(f.expressions[h], Expression::Load { .. }))
        };
        let mut guards: Vec<u32> = walk
            .steps
            .iter()
            .filter(|s| changed(s) && computes(s))
            .filter_map(|&s| self.step(s).guard)
            .chain(
                walk.stores
                    .iter()
                    .map(|&s| &self.trace.stores[s])
                    .filter(|store| store.step.as_ref().is_none_or(changed))
                    .filter_map(|store| store.guard),
            )
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut branches: Vec<&Branch> = Vec::new();
        while let Some(g) = guards.pop() {
            if !seen.insert(g) {
                continue;
            }
            if let Some(outer) = self.step(g).guard {
                guards.push(outer);
            }
            let Some(b) = self.branch_of(Some(g)) else {
                continue;
            };
            let repeat = branches.iter().any(|o| o.condition == b.condition);
            let same_exit = exit.is_some_and(|e| e.condition == b.condition);
            if !repeat && !same_exit && !self.module.is_uniform(b.condition) {
                branches.push(b);
            }
        }
        branches.sort_by_key(|b| b.at);
        branches
    }

    fn branch_of(&self, guard: Option<u32>) -> Option<&Branch> {
        let g = guard?;
        self.trace.branches.iter().rev().find(|b| b.step == Some(g))
    }

    fn outcome(&self, b: &Branch) -> String {
        let margin = b
            .margin
            .map(|m| format!(" ({} from flipping)", number(m)))
            .unwrap_or_default();
        format!("`{}` was {}{margin}", self.code(b.span), b.taken)
    }

    fn name(&self, i: u32) -> String {
        let step = self.step(i);
        step.name
            .clone()
            .or_else(|| self.stored_into(i).map(|s| s.target.clone()))
            .unwrap_or_else(|| self.code(step.span))
    }

    /// The assignment that stored a step's value in a variable, if one did.
    fn stored_into(&self, i: u32) -> Option<&texelscope_interp::Store> {
        self.trace.stores.iter().rev().find(|s| s.step == Some(i))
    }

    /// Where a step is in the source: its own span, or the assignment that stored it.
    fn place(&self, i: u32) -> Option<SourceSpan> {
        self.step(i)
            .span
            .or_else(|| self.stored_into(i).and_then(|s| s.span))
    }

    fn code(&self, span: Option<SourceSpan>) -> String {
        let Some(span) = span else {
            return String::new();
        };
        let text: String = self
            .module
            .text(span)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.chars().count() > 72 {
            let cut: String = text.chars().take(70).collect();
            format!("{cut}...")
        } else {
            text
        }
    }

    fn line(&self, span: Option<SourceSpan>) -> u32 {
        span.map(|s| self.module.line_col(s.start).line)
            .unwrap_or(0)
    }
}

/// The name a store's target uses for what a pointer points at, such as `h` or `h.tl`.
fn pointer_path(
    module: &ShaderModule,
    at: ExprRef,
    f: &naga::Function,
    pointer: Handle<Expression>,
) -> Option<String> {
    match f.expressions[pointer] {
        Expression::LocalVariable(v) => f.local_variables[v].name.clone(),
        Expression::GlobalVariable(g) => module.naga().global_variables[g].name.clone(),
        Expression::AccessIndex { base, index } => {
            let path = pointer_path(module, at, f, base)?;
            let base_ref = ExprRef {
                function: at.function,
                expr: base.index() as u32,
            };
            let types = &module.naga().types;
            let inner = match module.type_of(base_ref)? {
                TypeInner::Pointer { base, .. } => &types[*base].inner,
                other => other,
            };
            Some(match inner {
                TypeInner::Struct { members, .. } => {
                    format!("{path}.{}", members.get(index as usize)?.name.clone()?)
                }
                TypeInner::Vector { .. } | TypeInner::ValuePointer { .. } => {
                    format!("{path}.{}", ["x", "y", "z", "w"].get(index as usize)?)
                }
                _ => format!("{path}[{index}]"),
            })
        }
        Expression::Access { base, .. } => pointer_path(module, at, f, base),
        _ => None,
    }
}

/// Whether a pointer leads back to a global variable rather than a local.
fn root_is_global(f: &naga::Function, pointer: Handle<Expression>) -> bool {
    match f.expressions[pointer] {
        Expression::GlobalVariable(_) => true,
        Expression::AccessIndex { base, .. } | Expression::Access { base, .. } => {
            root_is_global(f, base)
        }
        _ => false,
    }
}

/// A constant operand's value, for literals the front end folded.
fn constant(module: &ShaderModule, f: &naga::Function, handle: Handle<Expression>) -> Option<f32> {
    let literal = |l: &Literal| -> Option<f32> {
        Some(match *l {
            Literal::F32(v) => v,
            Literal::F64(v) | Literal::AbstractFloat(v) => v as f32,
            Literal::I32(v) => v as f32,
            Literal::U32(v) => v as f32,
            Literal::I64(v) | Literal::AbstractInt(v) => v as f32,
            Literal::U64(v) => v as f32,
            _ => return None,
        })
    };
    match &f.expressions[handle] {
        Expression::Literal(l) => literal(l),
        Expression::ZeroValue(_) => Some(0.0),
        Expression::Constant(c) => {
            let naga = module.naga();
            match &naga.global_expressions[naga.constants[*c].init] {
                Expression::Literal(l) => literal(l),
                _ => None,
            }
        }
        _ => None,
    }
}

fn scalar(value: &Value) -> Option<f64> {
    match *value {
        Value::F32(v) => Some(f64::from(v)),
        Value::I32(v) => Some(f64::from(v)),
        Value::U32(v) => Some(f64::from(v)),
        _ => None,
    }
}

/// The one number a scalar or a vector with equal components holds.
fn all_equal(value: &Value) -> Option<f64> {
    match value {
        Value::Vector(lanes) => {
            let first = scalar(lanes.first()?)?;
            lanes
                .iter()
                .all(|l| scalar(l) == Some(first))
                .then_some(first)
        }
        other => scalar(other),
    }
}

fn value_text(value: &Value) -> String {
    match scalar(value) {
        Some(v) => number(v),
        None => value.to_string(),
    }
}

fn texel_text(name: &str, texels: &[[i32; 3]]) -> String {
    match texels {
        [] => format!("texture `{name}`"),
        [[x, y, _]] => format!("texel ({x}, {y}) of `{name}`"),
        [[x, y, _], ..] => format!(
            "a blend of {} texels of `{name}` near ({x}, {y})",
            texels.len()
        ),
    }
}

fn join(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The two sides of the `if` whose condition is `condition`.
fn find_if(
    block: &naga::Block,
    condition: Handle<Expression>,
) -> Option<(&naga::Block, &naga::Block)> {
    use naga::Statement as S;
    for statement in block.iter() {
        let found = match statement {
            S::If {
                condition: c,
                accept,
                reject,
            } if *c == condition => return Some((accept, reject)),
            S::If { accept, reject, .. } => {
                find_if(accept, condition).or_else(|| find_if(reject, condition))
            }
            S::Block(inner) => find_if(inner, condition),
            S::Loop {
                body, continuing, ..
            } => find_if(body, condition).or_else(|| find_if(continuing, condition)),
            S::Switch { cases, .. } => cases.iter().find_map(|c| find_if(&c.body, condition)),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Whether a block assigns to the variable `root` or one of its parts.
fn stores_to(
    module: &ShaderModule,
    at: ExprRef,
    f: &naga::Function,
    block: &naga::Block,
    root: &str,
) -> bool {
    use naga::Statement as S;
    block.iter().any(|statement| match statement {
        S::Store { pointer, .. } => pointer_path(module, at, f, *pointer)
            .is_some_and(|path| path.split(['.', '[']).next() == Some(root)),
        S::Block(inner) => stores_to(module, at, f, inner, root),
        S::If { accept, reject, .. } => {
            stores_to(module, at, f, accept, root) || stores_to(module, at, f, reject, root)
        }
        S::Loop {
            body, continuing, ..
        } => stores_to(module, at, f, body, root) || stores_to(module, at, f, continuing, root),
        S::Switch { cases, .. } => cases
            .iter()
            .any(|c| stores_to(module, at, f, &c.body, root)),
        _ => false,
    })
}
