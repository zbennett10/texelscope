use std::fmt::Write;

use texelscope_ir::Value;

use crate::model::Explanation;

/// Renders an explanation as aligned plain text.
pub fn render_text(explanation: &Explanation) -> String {
    let mut rows: Vec<(String, String)> = Vec::new();
    for link in &explanation.links {
        let name = match &link.scope {
            Some(scope) => format!("{scope}: {}", link.name),
            None => link.name.clone(),
        };
        rows.push((format!("{name} = {}", link.value), link.detail.clone()));
    }
    for decision in &explanation.decisions {
        let margin = decision
            .margin
            .map(|m| format!(", {} from flipping", number(m)))
            .unwrap_or_default();
        let count = if decision.count > 1 {
            format!(", {} times", decision.count)
        } else {
            String::new()
        };
        let forced = if decision.forced { ", forced" } else { "" };
        rows.push((
            decision.condition.clone(),
            format!("{}{margin}{count}{forced}", decision.taken),
        ));
    }
    if !explanation.sensitivities.is_empty() {
        let names: Vec<&str> = explanation
            .sensitivities
            .iter()
            .take(3)
            .map(|s| s.input.as_str())
            .collect();
        rows.push(("moves most with".to_string(), names.join(", then ")));
    }

    let width = rows
        .iter()
        .map(|(left, _)| left.chars().count())
        .max()
        .unwrap_or(0);
    let [x, y] = explanation.pixel;
    let shown = if explanation.discarded {
        "discarded".to_string()
    } else {
        color(&explanation.color)
    };
    let mut out = format!("pixel ({x}, {y}) = {shown}\n");
    for note in &explanation.notes {
        let _ = writeln!(out, "  note: {note}");
    }
    for (left, right) in rows {
        let line = format!("  {left:<width$}  {right}");
        let _ = writeln!(out, "{}", line.trim_end());
    }
    out
}

/// Four floats print as an rgba color with two decimals. Anything else prints as WGSL would.
fn color(value: &Value) -> String {
    if let Value::Vector(items) = value {
        let channels: Vec<String> = items
            .iter()
            .filter_map(|v| match v {
                Value::F32(c) => Some(format!("{c:.2}")),
                _ => None,
            })
            .collect();
        if items.len() == 4 && channels.len() == 4 {
            return format!("rgba({})", channels.join(", "));
        }
    }
    value.to_string()
}

/// Up to three decimals, without trailing zeros.
fn number(value: f64) -> String {
    let fixed = format!("{value:.3}");
    fixed
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use texelscope_ir::Value;

    use super::render_text;
    use crate::model::{Decision, Explanation, Link, LinkKind, Sensitivity};

    #[test]
    fn renders_an_aligned_explanation() {
        let rgba = |c: [f32; 4]| Value::Vector(c.into_iter().map(Value::F32).collect());
        let link = |name: &str, value: Value, detail: &str| Link {
            kind: LinkKind::Let,
            scope: None,
            name: name.to_string(),
            value,
            detail: detail.to_string(),
            span: None,
            expr: None,
            step: None,
        };
        let sensitivity = |input: &str, d_color: [f64; 4]| Sensitivity {
            input: input.to_string(),
            value: 0.0,
            d_color,
            perturbation: None,
        };
        let explanation = Explanation {
            pixel: [128, 64],
            color: rgba([0.95, 0.85, 0.1, 0.8]),
            discarded: false,
            links: vec![
                link("value", Value::F32(54.8), "bilinear of 4 texels"),
                link("bin", Value::U32(2), "floor((54.8 - 10) / 20)"),
            ],
            decisions: vec![Decision {
                condition: "value > params.visible_max".to_string(),
                taken: false,
                margin: Some(35.2),
                span: None,
                expr: None,
                count: 1,
                forced: false,
            }],
            sensitivities: vec![
                sensitivity("params.opacity", [0.0, 0.0, 0.0, 1.0]),
                sensitivity("texel (3, 3)", [0.0; 4]),
            ],
            notes: Vec::new(),
        };

        assert_eq!(
            render_text(&explanation),
            "pixel (128, 64) = rgba(0.95, 0.85, 0.10, 0.80)\n\
             \x20 value = 54.8                bilinear of 4 texels\n\
             \x20 bin = 2u                    floor((54.8 - 10) / 20)\n\
             \x20 value > params.visible_max  false, 35.2 from flipping\n\
             \x20 moves most with             params.opacity, then texel (3, 3)\n"
        );
    }
}
