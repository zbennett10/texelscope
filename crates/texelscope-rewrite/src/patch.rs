use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use texelscope_capture::Rect;
use texelscope_ir::naga::{Expression, ScalarKind, TypeInner};
use texelscope_ir::{ExprRef, Override, ShaderModule, SourceSpan, Value};

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
/// A pinned parameter is renamed, and a `let` with its name opens the function body.
pub fn patch_source(module: &ShaderModule, overrides: &[Override]) -> Result<String, RewriteError> {
    Ok(apply_edits(module.source(), &edits(module, overrides)?))
}

/// One replacement of a span of the source, the unit a patch is made of.
pub(crate) type Edit = (SourceSpan, String);

/// The edits that make each override hold, in source order and not overlapping.
pub(crate) fn edits(
    module: &ShaderModule,
    overrides: &[Override],
) -> Result<Vec<Edit>, RewriteError> {
    let mut edits: Vec<(SourceSpan, String)> = Vec::new();
    let mut lets: BTreeMap<u32, String> = BTreeMap::new();
    let mut names: HashSet<String> = HashSet::new();
    for o in latest(overrides) {
        let (expr, text) = match o {
            Override::Pin { expr, value } => {
                let ty = module.type_of(*expr).ok_or_else(|| unknown(expr.expr))?;
                (*expr, literal(module, value, ty)?)
            }
            Override::ForceBranch { condition, taken } => {
                let ty = module
                    .type_of(*condition)
                    .ok_or_else(|| unknown(condition.expr))?;
                if !matches!(ty, TypeInner::Scalar(s) if s.kind == ScalarKind::Bool) {
                    return Err(RewriteError::Unsupported(format!(
                        "expression {} is not a condition, so it can't be flipped",
                        condition.expr
                    )));
                }
                (*condition, taken.to_string())
            }
            Override::Replace { expr, wgsl } => (*expr, format!("({wgsl})")),
        };
        let span = module.span_of(expr).ok_or_else(|| {
            RewriteError::Unsupported(format!(
                "expression {} has no place in the source to patch",
                expr.expr
            ))
        })?;
        if !is_parameter(module, expr) {
            edits.push((span, text));
            continue;
        }
        let source = module.source();
        let brace = body_start(source, span.end as usize).ok_or_else(|| {
            RewriteError::Unsupported(format!("no function body follows parameter {}", expr.expr))
        })?;
        let name = module.text(span);
        let renamed = fresh_name(source, &format!("{name}_unpinned"), &mut names);
        edits.push((span, renamed));
        lets.entry(brace)
            .or_insert_with(|| "{".to_string())
            .push_str(&format!(" let {name} = {text};"));
    }
    for (brace, text) in lets {
        edits.push((
            SourceSpan {
                start: brace,
                end: brace + 1,
            },
            text,
        ));
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
    Ok(kept)
}

/// The source with edits applied. Later edits go first so earlier offsets stay valid.
pub(crate) fn apply_edits(source: &str, edits: &[Edit]) -> String {
    let mut source = source.to_string();
    for (span, text) in edits.iter().rev() {
        source.replace_range(span.start as usize..span.end as usize, text);
    }
    source
}

/// Where an offset of the original source lands once edits are applied, or `None` when an
/// edit replaced it.
pub(crate) fn shift(edits: &[Edit], offset: u32) -> Option<u32> {
    let mut moved = i64::from(offset);
    for (span, text) in edits {
        if span.end <= offset {
            moved += text.len() as i64 - i64::from(span.end - span.start);
        } else if span.start < offset {
            return None;
        }
    }
    u32::try_from(moved).ok()
}

/// The expression of the patched module that is `expr` of the original: the same function,
/// the same kind of expression and the same span once moved past the edits.
pub(crate) fn follow(
    original: &ShaderModule,
    patched: &ShaderModule,
    edits: &[Edit],
    expr: ExprRef,
) -> Option<ExprRef> {
    let (f, h) = original.expression(expr)?;
    let range = f.expressions.get_span(h).to_range()?;
    let start = shift(edits, range.start as u32)?;
    let end = shift(edits, range.end as u32)?;
    let kind = std::mem::discriminant(&f.expressions[h]);
    let (_, g) = patched.function(expr.function)?;
    let found = g.expressions.iter().position(|(handle, e)| {
        std::mem::discriminant(e) == kind
            && g.expressions.get_span(handle).to_range() == Some(start as usize..end as usize)
    })?;
    Some(ExprRef {
        function: expr.function,
        expr: found as u32,
    })
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

/// The last override of each expression, in their original order.
fn latest(overrides: &[Override]) -> Vec<&Override> {
    let mut kept: Vec<&Override> = Vec::new();
    for o in overrides.iter().rev() {
        if kept.iter().all(|k| k.target() != o.target()) {
            kept.push(o);
        }
    }
    kept.reverse();
    kept
}

fn is_parameter(module: &ShaderModule, expr: ExprRef) -> bool {
    module
        .expression(expr)
        .is_some_and(|(f, h)| matches!(f.expressions[h], Expression::FunctionArgument(_)))
}

/// The offset of the `{` that opens the body of the function whose parameter list
/// contains `from`.
fn body_start(source: &str, from: usize) -> Option<u32> {
    let bytes = source.as_bytes();
    let mut depth = 1u32;
    let mut at = from;
    while let Some(&byte) = bytes.get(at) {
        if let Some(end) = comment_end(bytes, at) {
            at = end;
            continue;
        }
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'{' if depth == 0 => return u32::try_from(at).ok(),
            _ => {}
        }
        at += 1;
    }
    None
}

/// Where the comment starting at `at` ends, if one starts there. Block comments nest.
fn comment_end(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes.get(at..at + 2)? {
        b"//" => Some(
            bytes[at..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(bytes.len(), |n| at + n),
        ),
        b"/*" => {
            let (mut depth, mut i) = (0u32, at);
            while let Some(pair) = bytes.get(i..i + 2) {
                match pair {
                    b"/*" => (depth, i) = (depth + 1, i + 2),
                    b"*/" if depth == 1 => return Some(i + 2),
                    b"*/" => (depth, i) = (depth - 1, i + 2),
                    _ => i += 1,
                }
            }
            Some(bytes.len())
        }
        _ => None,
    }
}

/// `base`, numbered if need be, so that it names nothing in the source or in `taken`.
fn fresh_name(source: &str, base: &str, taken: &mut HashSet<String>) -> String {
    let mut name = base.to_string();
    let mut n = 1;
    while taken.contains(&name) || mentions(source, &name) {
        name = format!("{base}_{n}");
        n += 1;
    }
    taken.insert(name.clone());
    name
}

/// Whether `word` appears in the source as a whole identifier.
fn mentions(source: &str, word: &str) -> bool {
    let part = |b: &u8| b.is_ascii_alphanumeric() || *b == b'_';
    let bytes = source.as_bytes();
    source.match_indices(word).any(|(at, _)| {
        let before = at.checked_sub(1).and_then(|i| bytes.get(i));
        !before.is_some_and(part) && !bytes.get(at + word.len()).is_some_and(part)
    })
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
