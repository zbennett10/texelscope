//! What sets two pixels apart: the first decision where their paths through the shader part,
//! or, on the same path, the value that differs most on the way to the color.

use std::collections::{HashMap, HashSet};

use texelscope_interp::{BranchKind, Interpreter, Trace};
use texelscope_ir::{ExprRef, Override, ShaderModule, SourceSpan, Value};

use crate::ExplainError;
use crate::model::{Comparison, DecisionPair, Reason, ValuePair};
use crate::text::number;
use crate::why::{self, Analysis};

/// Traces two pixels and says what sets them apart.
pub fn compare_pixels(
    interp: &Interpreter,
    a: [u32; 2],
    b: [u32; 2],
    overrides: &[Override],
) -> Result<Comparison, ExplainError> {
    let ta = interp.trace_pixel(a, overrides)?;
    let tb = interp.trace_pixel(b, overrides)?;
    Ok(compare(interp.module(), &ta, &tb))
}

/// Compares two traces of the same shader.
pub fn compare(module: &ShaderModule, a: &Trace, b: &Trace) -> Comparison {
    let why_a = why::analyze(module, a);
    let why_b = why::analyze(module, b);
    let decisions = decision_pairs(module, a, b);
    let values = value_pairs(module, a, b, &why_a, &why_b);
    let summary = summary(module, a, b, &decisions, &values, &why_a, &why_b);
    Comparison {
        a: a.pixel,
        b: b.pixel,
        color_a: color(a),
        color_b: color(b),
        summary,
        decisions,
        values,
        why_a: why_a.reasons,
        why_b: why_b.reasons,
    }
}

fn color(trace: &Trace) -> Value {
    trace.color().cloned().unwrap_or(Value::Vector(Vec::new()))
}

/// Each branch keyed by its condition and how many times that condition ran before it, so the
/// nth time in one pixel lines up with the nth time in the other.
fn keyed(trace: &Trace) -> Vec<((ExprRef, u32), &texelscope_interp::Branch)> {
    let mut seen: HashMap<ExprRef, u32> = HashMap::new();
    trace
        .branches
        .iter()
        .filter(|b| b.kind != BranchKind::Switch)
        .map(|b| {
            let n = seen.entry(b.condition).or_default();
            let key = (b.condition, *n);
            *n += 1;
            (key, b)
        })
        .collect()
}

/// Decisions that went differently, or ran in one pixel only, in the order A made them. A
/// condition that differs several times, as in a loop, is listed once.
fn decision_pairs(module: &ShaderModule, a: &Trace, b: &Trace) -> Vec<DecisionPair> {
    let ka = keyed(a);
    let kb = keyed(b);
    let in_b: HashMap<_, _> = kb.iter().copied().collect();
    let in_a: HashSet<_> = ka.iter().map(|(k, _)| *k).collect();
    let counts = |trace: &Trace, condition: ExprRef| {
        trace
            .branches
            .iter()
            .filter(|b| b.condition == condition)
            .count() as u32
    };
    let mut pairs: Vec<DecisionPair> = Vec::new();
    let mut listed: HashSet<ExprRef> = HashSet::new();
    let mut push = |condition: ExprRef, span: Option<SourceSpan>, pair: DecisionPair| {
        if listed.insert(condition) {
            pairs.push(DecisionPair { span, ..pair });
        }
    };
    for (key, da) in &ka {
        let db = in_b.get(key);
        if db.is_some_and(|db| db.taken == da.taken) {
            continue;
        }
        push(
            da.condition,
            da.span,
            DecisionPair {
                condition: text(module, da.span),
                expr: Some(da.condition),
                span: None,
                a: Some(da.taken),
                b: db.map(|db| db.taken),
                times_a: counts(a, da.condition),
                times_b: counts(b, da.condition),
            },
        );
    }
    for (key, db) in &kb {
        if in_a.contains(key) {
            continue;
        }
        push(
            db.condition,
            db.span,
            DecisionPair {
                condition: text(module, db.span),
                expr: Some(db.condition),
                span: None,
                a: None,
                b: Some(db.taken),
                times_a: counts(a, db.condition),
                times_b: counts(b, db.condition),
            },
        );
    }
    pairs
}

/// Named values on the way to each pixel's color, from the data to the color, with the other
/// pixel's value of each.
fn value_pairs(
    module: &ShaderModule,
    a: &Trace,
    b: &Trace,
    why_a: &Analysis,
    why_b: &Analysis,
) -> Vec<ValuePair> {
    let ia = occurrences(a);
    let ib = occurrences(b);
    let mut pairs = Vec::new();
    let mut listed = HashSet::new();
    for (trace, other, mine, theirs, chain, a_side) in [
        (a, b, &ia, &ib, &why_a.chain, true),
        (b, a, &ib, &ia, &why_b.chain, false),
    ] {
        for &s in chain.iter().rev() {
            let key = mine[s as usize];
            if !listed.insert(key) {
                continue;
            }
            let step = &trace.steps[s as usize];
            let there = theirs
                .iter()
                .position(|k| *k == key)
                .map(|i| other.steps[i].value.clone());
            let here = Some(step.value.clone());
            let (value_a, value_b) = if a_side { (here, there) } else { (there, here) };
            pairs.push(ValuePair {
                name: step.name.clone().unwrap_or_else(|| text(module, step.span)),
                scope: match step.expr.function {
                    texelscope_ir::FunctionRef::EntryPoint(_) => None,
                    f => module.function(f).map(|(name, _)| name.to_string()),
                },
                span: step.span,
                a: value_a,
                b: value_b,
            });
        }
    }
    pairs
}

/// Each step keyed by its expression and how many times that expression ran before it.
fn occurrences(trace: &Trace) -> Vec<(ExprRef, u32)> {
    let mut seen: HashMap<ExprRef, u32> = HashMap::new();
    trace
        .steps
        .iter()
        .map(|s| {
            let n = seen.entry(s.expr).or_default();
            let key = (s.expr, *n);
            *n += 1;
            key
        })
        .collect()
}

fn summary(
    module: &ShaderModule,
    a: &Trace,
    b: &Trace,
    decisions: &[DecisionPair],
    values: &[ValuePair],
    why_a: &Analysis,
    why_b: &Analysis,
) -> Vec<Reason> {
    let mut out = Vec::new();
    if a.discarded != b.discarded {
        let (gone, kept) = if a.discarded { ("A", "B") } else { ("B", "A") };
        out.push(Reason::at(
            None,
            format!("{gone} was discarded and {kept} was not."),
        ));
    } else if color(a) == color(b) {
        out.push(Reason::at(
            None,
            "Both pixels end up the same color.".to_string(),
        ));
    }
    // Only a decision that shaped either pixel's path to its color is where they part ways.
    let shaped = |d: &&DecisionPair| {
        d.expr
            .is_some_and(|e| why_a.path.contains(&e) || why_b.path.contains(&e))
    };
    let fork = decisions
        .iter()
        .filter(shaped)
        .find(|d| d.a.is_some() && d.b.is_some() && d.a != d.b);
    let other = decisions.iter().find(|d| !shaped(d));
    if let Some(d) = fork {
        out.push(Reason::at(
            d.span,
            format!(
                "They part ways on line {}, where `{}` was {} for A and {} for B.",
                line(module, d.span),
                d.condition,
                d.a.unwrap_or_default(),
                d.b.unwrap_or_default()
            ),
        ));
    } else if let Some(d) = decisions.iter().find(shaped) {
        out.push(Reason::at(
            d.span,
            format!(
                "They make the same choices, but `{}` ran {} for A and {} for B.",
                d.condition,
                times(d.times_a),
                times(d.times_b)
            ),
        ));
    } else {
        let inside = other
            .map(|d| {
                format!(
                    " Decisions inside the work that picks the values still differ, starting with `{}` on line {}.",
                    d.condition,
                    line(module, d.span)
                )
            })
            .unwrap_or_default();
        out.push(Reason::at(
            other.and_then(|d| d.span),
            format!("Both took the same path to the color.{inside}"),
        ));
    }
    let both = match (why_a.held, why_b.held) {
        (Some((sa, function, bound)), Some((sb, _, other)))
            if a.steps[sa as usize].expr == b.steps[sb as usize].expr && bound == other =>
        {
            let step = &a.steps[sa as usize];
            let name = step.name.clone().unwrap_or_else(|| text(module, step.span));
            out.push(Reason::at(
                step.span,
                format!(
                    "In both, `{name}` is held at {} by `{function}`, so the differences before it don't change the color.",
                    number(bound)
                ),
            ));
            true
        }
        _ => false,
    };
    for (mine, me, them) in [(why_a, "A", "B"), (why_b, "B", "A")] {
        if both {
            break;
        }
        let Some((step, function, bound)) = mine.held else {
            continue;
        };
        let trace = if me == "A" { a } else { b };
        let held = &trace.steps[step as usize];
        let name = held.name.clone().unwrap_or_else(|| text(module, held.span));
        let other_value = values
            .iter()
            .find(|v| v.span == held.span && v.name == name)
            .and_then(|v| if me == "A" { v.b.clone() } else { v.a.clone() });
        let Some(other_value) = other_value.filter(|v| *v != held.value) else {
            continue;
        };
        out.push(Reason::at(
            held.span,
            format!(
                "In {me}, `{name}` is held at {} by `{function}`, so the values before it don't change {me}'s color. In {them}, `{name}` is {}.",
                number(bound),
                value_text(&other_value)
            ),
        ));
        break;
    }
    if fork.is_none()
        && let Some((pair, _)) = values
            .iter()
            .filter_map(|v| Some((v, relative(v.a.as_ref()?, v.b.as_ref()?)?)))
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .filter(|(_, d)| *d > 1e-6)
    {
        out.push(Reason::at(
            pair.span,
            format!(
                "On the way to the color they differ most in `{}`: {} for A and {} for B.",
                pair.name,
                pair.a.as_ref().map(value_text).unwrap_or_default(),
                pair.b.as_ref().map(value_text).unwrap_or_default()
            ),
        ));
    }
    out
}

/// How different two scalars are, relative to their size.
fn relative(a: &Value, b: &Value) -> Option<f64> {
    let (x, y) = (scalar(a)?, scalar(b)?);
    Some((x - y).abs() / x.abs().max(y.abs()).max(1e-3))
}

fn scalar(value: &Value) -> Option<f64> {
    match *value {
        Value::F32(v) => Some(f64::from(v)),
        Value::I32(v) => Some(f64::from(v)),
        Value::U32(v) => Some(f64::from(v)),
        _ => None,
    }
}

fn value_text(value: &Value) -> String {
    scalar(value).map_or_else(|| value.to_string(), number)
}

fn times(n: u32) -> String {
    if n == 1 {
        "once".to_string()
    } else {
        format!("{n} times")
    }
}

fn text(module: &ShaderModule, span: Option<SourceSpan>) -> String {
    span.map(|s| {
        module
            .text(s)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    })
    .unwrap_or_default()
}

fn line(module: &ShaderModule, span: Option<SourceSpan>) -> u32 {
    span.map(|s| module.line_col(s.start).line).unwrap_or(0)
}
