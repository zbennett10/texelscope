//! A trace as text: one line per value, decision or assignment, with its source line.

use std::fmt::Write;

use texelscope_interp::{BranchKind, Trace};
use texelscope_ir::{ShaderModule, SourceSpan, Value};

enum Event<'a> {
    Step(usize),
    Branch(&'a texelscope_interp::Branch),
    Store(&'a texelscope_interp::Store),
}

/// Renders a trace. Without `all`, only named values, decisions and assignments show.
pub fn render(module: &ShaderModule, trace: &Trace, all: bool) -> String {
    let mut events: Vec<(u32, u8, Event)> = Vec::new();
    for (i, step) in trace.steps.iter().enumerate() {
        if all || step.name.is_some() || step.origin.is_some() {
            events.push((i as u32, 1, Event::Step(i)));
        }
    }
    for branch in &trace.branches {
        if all || branch.kind != BranchKind::Select {
            events.push((branch.at, 0, Event::Branch(branch)));
        }
    }
    for store in &trace.stores {
        events.push((store.at, 0, Event::Store(store)));
    }
    // A branch or store at `n` happened after step n - 1 and before step n.
    events.sort_by_key(|(at, order, _)| (*at, *order));

    let [x, y] = trace.pixel;
    let mut out = format!("pixel ({x}, {y}) of {}\n", trace.entry_point);
    match trace.color() {
        _ if trace.discarded => out.push_str("discarded\n"),
        Some(color) => {
            let _ = writeln!(out, "color {}", short(color));
        }
        None => out.push_str("no color output\n"),
    }
    for note in &trace.notes {
        let _ = writeln!(out, "note  {note}");
    }
    out.push('\n');

    for (_, _, event) in events {
        let (span, depth, left, right) = match event {
            Event::Step(i) => {
                let step = &trace.steps[i];
                let text = source(module, step.span);
                let left = match &step.name {
                    Some(name) if *name != text => format!("{name} = {text}"),
                    Some(name) => name.clone(),
                    None => text,
                };
                let mut right = short(&step.value);
                if let Some(note) = &step.note {
                    let _ = write!(right, "  ({note})");
                }
                (step.span, step.depth, left, right)
            }
            Event::Branch(branch) => {
                let keyword = match branch.kind {
                    BranchKind::If => "if",
                    BranchKind::Select => "select",
                    BranchKind::BreakIf => "break if",
                    BranchKind::Switch => "switch",
                };
                let mut right = if branch.taken { "true" } else { "false" }.to_string();
                if let Some(margin) = branch.margin {
                    let _ = write!(right, ", {} from flipping", number(margin));
                }
                if branch.forced {
                    right.push_str(", forced");
                }
                (
                    branch.span,
                    branch.depth,
                    format!("{keyword} {}", source(module, branch.span)),
                    right,
                )
            }
            Event::Store(store) => (
                store.span,
                store.depth,
                store.target.clone(),
                short(&store.value),
            ),
        };
        let line = span.map(|s| module.line_col(s.start).line).unwrap_or(0);
        let indent = "  ".repeat(depth as usize);
        let left = clip(&format!("{indent}{left}"), 56);
        let _ = writeln!(out, "{line:>4}  {left:<56}  {right}");
    }
    if trace.truncated {
        out.push_str("\nthe trace stopped recording at its step limit\n");
    }
    out
}

/// The source a span covers, on one line.
fn source(module: &ShaderModule, span: Option<SourceSpan>) -> String {
    let Some(span) = span else {
        return "?".to_string();
    };
    module
        .text(span)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width - 3).collect();
    format!("{kept}...")
}

/// A value with floats rounded to four decimals.
pub fn short(value: &Value) -> String {
    match value {
        Value::F32(v) => number(f64::from(*v)),
        Value::Vector(items) => {
            let inner: Vec<String> = items.iter().map(short).collect();
            format!("vec{}({})", items.len(), inner.join(", "))
        }
        other => other.to_string(),
    }
}

fn number(value: f64) -> String {
    if !value.is_finite() {
        return format!("{value}");
    }
    let fixed = format!("{value:.4}");
    let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}
