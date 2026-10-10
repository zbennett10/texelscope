use serde::{Deserialize, Serialize};
use texelscope_capture::Rect;
use texelscope_ir::naga::TypeInner;
use texelscope_ir::{Override, ShaderModule, SourceSpan, Value};

use crate::RewriteError;

/// A set of overrides, optionally limited to one region of the target.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub overrides: Vec<Override>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Rect>,
}

/// Applies a patch and returns the rewritten module, which still validates. The GPU sees
/// the whole frame patched, so a region limits only the interpreter.
pub fn apply(module: &ShaderModule, patch: &Patch) -> Result<ShaderModule, RewriteError> {
    let source = patch_source(module, &patch.overrides)?;
    ShaderModule::from_wgsl(source).map_err(|e| RewriteError::Emit {
        target: "patched WGSL",
        message: e.to_string(),
    })
}

/// Rewrites the source so each override holds: a pinned expression becomes a literal, a
/// forced condition becomes `true` or `false` and a replacement takes the expression's place.
pub fn patch_source(module: &ShaderModule, overrides: &[Override]) -> Result<String, RewriteError> {
    let mut edits: Vec<(SourceSpan, String)> = Vec::new();
    for o in overrides {
        let (expr, text) = match o {
            Override::Pin { expr, value } => {
                let ty = module.type_of(*expr).ok_or_else(|| unknown(expr.expr))?;
                (*expr, literal(module, value, ty)?)
            }
            Override::ForceBranch { condition, taken } => (*condition, taken.to_string()),
            Override::Replace { expr, wgsl } => (*expr, format!("({wgsl})")),
        };
        let span = module.span_of(expr).ok_or_else(|| {
            RewriteError::Unsupported(format!(
                "expression {} has no place in the source to patch",
                expr.expr
            ))
        })?;
        edits.push((span, text));
    }
    // Later edits go first so earlier offsets stay valid. Edits inside another one drop.
    edits.sort_by_key(|(span, _)| (span.start, std::cmp::Reverse(span.end)));
    let mut kept: Vec<(SourceSpan, String)> = Vec::new();
    for (span, text) in edits {
        match kept.last() {
            Some((last, _)) if span.start < last.end => continue,
            _ => kept.push((span, text)),
        }
    }
    let mut source = module.source().to_string();
    for (span, text) in kept.into_iter().rev() {
        source.replace_range(span.start as usize..span.end as usize, &text);
    }
    Ok(source)
}

/// A WGSL expression for a constant of the given type.
fn literal(module: &ShaderModule, value: &Value, ty: &TypeInner) -> Result<String, RewriteError> {
    let scalar = |v: &Value| -> Result<String, RewriteError> {
        Ok(match v {
            Value::Bool(b) => b.to_string(),
            Value::I32(i) => format!("i32({i})"),
            Value::U32(u) => format!("{u}u"),
            Value::F32(f) if f.is_finite() => format!("f32({f:?})"),
            Value::F32(f) => format!("bitcast<f32>({}u)", f.to_bits()),
            other => {
                return Err(RewriteError::Unsupported(format!(
                    "{other} is not a scalar"
                )));
            }
        })
    };
    match (value, ty) {
        (_, TypeInner::Scalar(_)) => scalar(value),
        (Value::Vector(items), TypeInner::Vector { .. })
        | (Value::Matrix(items), TypeInner::Matrix { .. }) => {
            let parts = items
                .iter()
                .map(|item| match item {
                    Value::Vector(lanes) => lanes
                        .iter()
                        .map(scalar)
                        .collect::<Result<Vec<_>, _>>()
                        .map(|l| l.join(", ")),
                    other => scalar(other),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("{}({})", module.wgsl_type(ty), parts.join(", ")))
        }
        _ => Err(RewriteError::Unsupported(format!(
            "pinning a {} is not supported yet",
            module.wgsl_type(ty)
        ))),
    }
}

fn unknown(expr: u32) -> RewriteError {
    RewriteError::Unsupported(format!("there is no expression {expr} to override"))
}

#[cfg(test)]
mod tests {
    use texelscope_ir::{ExprRef, FunctionRef, Override, ShaderModule, Value};

    use super::patch_source;

    const SHADER: &str = "@fragment fn fs(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let level = clamp(uv.x * 2.0, 0.0, 1.0);
    if (level > 0.5) { return vec4<f32>(1.0); }
    return vec4<f32>(level);
}";

    fn find(module: &ShaderModule, text: &str) -> ExprRef {
        let function = FunctionRef::EntryPoint(0);
        let count = module.function(function).unwrap().1.expressions.len();
        (0..count as u32)
            .map(|expr| ExprRef { function, expr })
            .find(|e| module.span_of(*e).is_some_and(|s| module.text(s) == text))
            .unwrap_or_else(|| panic!("no expression spells {text}"))
    }

    #[test]
    fn pins_and_forces_by_editing_the_source() {
        let module = ShaderModule::from_wgsl(SHADER).unwrap();
        let level = find(&module, "clamp(uv.x * 2.0, 0.0, 1.0)");
        let condition = find(&module, "level > 0.5");
        let patched = patch_source(
            &module,
            &[
                Override::Pin {
                    expr: level,
                    value: Value::F32(0.25),
                },
                Override::ForceBranch {
                    condition,
                    taken: true,
                },
            ],
        )
        .unwrap();
        assert!(patched.contains("let level = f32(0.25);"), "{patched}");
        assert!(patched.contains("if (true)"), "{patched}");
        ShaderModule::from_wgsl(patched).unwrap();
    }
}
