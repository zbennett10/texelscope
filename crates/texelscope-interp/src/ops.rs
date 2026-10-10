//! Operators with WGSL semantics. Integer math wraps, integer division by zero follows the
//! spec, and float remainder truncates as GPUs compute it.

use texelscope_ir::naga::{
    self, BinaryOperator as B, Handle, RelationalFunction, ScalarKind, TypeInner, UnaryOperator,
};

use crate::InterpError;
use crate::val::{CompositeKind, Scalar, Val, Vector};

pub(crate) fn unary(op: UnaryOperator, value: &Val) -> Result<Val, InterpError> {
    let f = |s: Scalar| -> Result<Scalar, InterpError> {
        Ok(match (op, s) {
            (UnaryOperator::Negate, Scalar::F32(v)) => Scalar::F32(-v),
            (UnaryOperator::Negate, Scalar::I32(v)) => Scalar::I32(v.wrapping_neg()),
            (UnaryOperator::Negate, Scalar::AbstractInt(v)) => {
                Scalar::AbstractInt(v.wrapping_neg())
            }
            (UnaryOperator::Negate, Scalar::AbstractFloat(v)) => Scalar::AbstractFloat(-v),
            (UnaryOperator::LogicalNot, Scalar::Bool(v)) => Scalar::Bool(!v),
            (UnaryOperator::BitwiseNot, Scalar::I32(v)) => Scalar::I32(!v),
            (UnaryOperator::BitwiseNot, Scalar::U32(v)) => Scalar::U32(!v),
            (UnaryOperator::BitwiseNot, Scalar::AbstractInt(v)) => Scalar::AbstractInt(!v),
            _ => return Err(bad_operands(format!("{op:?}"), &[s])),
        })
    };
    map1(value, f)
}

pub(crate) fn binary(op: B, left: &Val, right: &Val) -> Result<Val, InterpError> {
    if let (Val::Composite(a), Val::Composite(b)) = (left, right)
        && a.kind == CompositeKind::Matrix
        && b.kind == CompositeKind::Matrix
    {
        return match op {
            B::Multiply => {
                let columns = b
                    .items
                    .iter()
                    .map(|column| matrix_times_vector(&a.items, column))
                    .collect::<Result<_, _>>()?;
                Ok(Val::composite(CompositeKind::Matrix, columns))
            }
            B::Add | B::Subtract => {
                let columns = a
                    .items
                    .iter()
                    .zip(b.items.iter())
                    .map(|(x, y)| binary(op, x, y))
                    .collect::<Result<_, _>>()?;
                Ok(Val::composite(CompositeKind::Matrix, columns))
            }
            _ => Err(InterpError::Internal(format!("{op:?} on two matrices"))),
        };
    }
    match (left, right) {
        (Val::Composite(m), Val::Vector(_))
            if m.kind == CompositeKind::Matrix && op == B::Multiply =>
        {
            matrix_times_vector(&m.items, right)
        }
        (Val::Vector(_), Val::Composite(m))
            if m.kind == CompositeKind::Matrix && op == B::Multiply =>
        {
            let lanes = m
                .items
                .iter()
                .map(|column| dot(left, column).and_then(|d| d.scalar()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::Vector(Vector::new(&lanes)))
        }
        (Val::Composite(m), Val::Scalar(_)) | (Val::Scalar(_), Val::Composite(m))
            if m.kind == CompositeKind::Matrix && op == B::Multiply =>
        {
            let scale = if matches!(left, Val::Scalar(_)) {
                left
            } else {
                right
            };
            let columns = m
                .items
                .iter()
                .map(|column| binary(op, column, scale))
                .collect::<Result<_, _>>()?;
            Ok(Val::composite(CompositeKind::Matrix, columns))
        }
        _ => map2(left, right, |a, b| scalar_binary(op, a, b)),
    }
}

pub(crate) fn scalar_binary(op: B, a: Scalar, b: Scalar) -> Result<Scalar, InterpError> {
    use Scalar as S;
    let (a, b) = unify(a, b);
    let compare = |ord: Option<std::cmp::Ordering>| -> Option<Scalar> {
        use std::cmp::Ordering::*;
        Some(S::Bool(match op {
            B::Equal => ord == Some(Equal),
            B::NotEqual => ord != Some(Equal),
            B::Less => ord == Some(Less),
            B::LessEqual => matches!(ord, Some(Less | Equal)),
            B::Greater => ord == Some(Greater),
            B::GreaterEqual => matches!(ord, Some(Greater | Equal)),
            _ => return None,
        }))
    };
    let result = match (a, b) {
        (S::F32(x), S::F32(y)) => match op {
            B::Add => Some(S::F32(x + y)),
            B::Subtract => Some(S::F32(x - y)),
            B::Multiply => Some(S::F32(x * y)),
            B::Divide => Some(S::F32(x / y)),
            B::Modulo => Some(S::F32(x - y * (x / y).trunc())),
            _ => compare(x.partial_cmp(&y)),
        },
        (S::AbstractFloat(x), S::AbstractFloat(y)) => match op {
            B::Add => Some(S::AbstractFloat(x + y)),
            B::Subtract => Some(S::AbstractFloat(x - y)),
            B::Multiply => Some(S::AbstractFloat(x * y)),
            B::Divide => Some(S::AbstractFloat(x / y)),
            B::Modulo => Some(S::AbstractFloat(x - y * (x / y).trunc())),
            _ => compare(x.partial_cmp(&y)),
        },
        (S::I32(x), S::I32(y)) => match op {
            B::Add => Some(S::I32(x.wrapping_add(y))),
            B::Subtract => Some(S::I32(x.wrapping_sub(y))),
            B::Multiply => Some(S::I32(x.wrapping_mul(y))),
            // Division by zero, or of the minimum by -1, gives the dividend.
            B::Divide => Some(S::I32(if y == 0 || (x == i32::MIN && y == -1) {
                x
            } else {
                x / y
            })),
            B::Modulo => Some(S::I32(if y == 0 || (x == i32::MIN && y == -1) {
                0
            } else {
                x % y
            })),
            B::And => Some(S::I32(x & y)),
            B::InclusiveOr => Some(S::I32(x | y)),
            B::ExclusiveOr => Some(S::I32(x ^ y)),
            _ => compare(Some(x.cmp(&y))),
        },
        (S::U32(x), S::U32(y)) => match op {
            B::Add => Some(S::U32(x.wrapping_add(y))),
            B::Subtract => Some(S::U32(x.wrapping_sub(y))),
            B::Multiply => Some(S::U32(x.wrapping_mul(y))),
            B::Divide => Some(S::U32(if y == 0 { x } else { x / y })),
            B::Modulo => Some(S::U32(if y == 0 { 0 } else { x % y })),
            B::And => Some(S::U32(x & y)),
            B::InclusiveOr => Some(S::U32(x | y)),
            B::ExclusiveOr => Some(S::U32(x ^ y)),
            B::ShiftLeft => Some(S::U32(x.wrapping_shl(y))),
            B::ShiftRight => Some(S::U32(x.wrapping_shr(y))),
            _ => compare(Some(x.cmp(&y))),
        },
        (S::I32(x), S::U32(y)) => match op {
            B::ShiftLeft => Some(S::I32(x.wrapping_shl(y))),
            B::ShiftRight => Some(S::I32(x.wrapping_shr(y))),
            _ => None,
        },
        (S::AbstractInt(x), S::AbstractInt(y)) => match op {
            B::Add => Some(S::AbstractInt(x.wrapping_add(y))),
            B::Subtract => Some(S::AbstractInt(x.wrapping_sub(y))),
            B::Multiply => Some(S::AbstractInt(x.wrapping_mul(y))),
            B::Divide if y != 0 => Some(S::AbstractInt(x / y)),
            B::Modulo if y != 0 => Some(S::AbstractInt(x % y)),
            B::And => Some(S::AbstractInt(x & y)),
            B::InclusiveOr => Some(S::AbstractInt(x | y)),
            B::ExclusiveOr => Some(S::AbstractInt(x ^ y)),
            B::ShiftLeft => Some(S::AbstractInt(x.wrapping_shl(y as u32))),
            B::ShiftRight => Some(S::AbstractInt(x.wrapping_shr(y as u32))),
            _ => compare(Some(x.cmp(&y))),
        },
        (S::Bool(x), S::Bool(y)) => match op {
            B::Equal => Some(S::Bool(x == y)),
            B::NotEqual => Some(S::Bool(x != y)),
            B::LogicalAnd | B::And => Some(S::Bool(x && y)),
            B::LogicalOr | B::InclusiveOr => Some(S::Bool(x || y)),
            B::ExclusiveOr => Some(S::Bool(x ^ y)),
            _ => None,
        },
        _ => None,
    };
    result.ok_or_else(|| bad_operands(format!("{op:?}"), &[a, b]))
}

/// Brings an abstract operand to the concrete kind of the other one.
fn unify(a: Scalar, b: Scalar) -> (Scalar, Scalar) {
    fn lift(x: Scalar, to: Scalar) -> Scalar {
        use Scalar as S;
        match (x, to) {
            (S::AbstractInt(v), S::I32(_)) => S::I32(v as i32),
            (S::AbstractInt(v), S::U32(_)) => S::U32(v as u32),
            (S::AbstractInt(v), S::F32(_)) => S::F32(v as f32),
            (S::AbstractInt(v), S::AbstractFloat(_)) => S::AbstractFloat(v as f64),
            (S::AbstractFloat(v), S::F32(_)) => S::F32(v as f32),
            _ => x,
        }
    }
    (lift(a, b), lift(b, a))
}

pub(crate) fn select(condition: &Val, accept: &Val, reject: &Val) -> Result<Val, InterpError> {
    match condition {
        Val::Scalar(c) => Ok(if c.as_bool()? { accept } else { reject }.clone()),
        Val::Vector(mask) => match (accept, reject) {
            (Val::Vector(a), Val::Vector(r)) => {
                let mut out = *a;
                for i in 0..usize::from(mask.len) {
                    if !mask.lanes[i].as_bool()? {
                        out.lanes[i] = r.lanes[i];
                    }
                }
                Ok(Val::Vector(out))
            }
            _ => Err(InterpError::Internal(
                "select with a vector mask on scalars".into(),
            )),
        },
        other => Err(InterpError::Internal(format!("select on {other:?}"))),
    }
}

/// `convert` is the target width for a value conversion and `None` for a bitcast.
pub(crate) fn convert(
    value: &Val,
    kind: ScalarKind,
    convert: Option<u8>,
) -> Result<Val, InterpError> {
    if let Some(width) = convert
        && kind != ScalarKind::Bool
        && width != 4
    {
        return Err(InterpError::Unsupported(format!(
            "conversions to {width}-byte values are not interpreted yet"
        )));
    }
    match value {
        Val::Composite(c) if c.kind == CompositeKind::Matrix => {
            let columns = c
                .items
                .iter()
                .map(|col| self::convert(col, kind, convert))
                .collect::<Result<_, _>>()?;
            Ok(Val::composite(CompositeKind::Matrix, columns))
        }
        _ => map1(value, |s| match convert {
            Some(_) => convert_scalar(s, kind),
            None => bitcast(s, kind),
        }),
    }
}

/// Value conversion between scalar kinds.
pub(crate) fn convert_scalar(s: Scalar, kind: ScalarKind) -> Result<Scalar, InterpError> {
    use Scalar as S;
    Ok(match kind {
        ScalarKind::Float => S::F32(match s {
            S::Bool(v) => f32::from(u8::from(v)),
            other => other.as_f64() as f32,
        }),
        // Rust's float casts saturate and send NaN to zero, which WGSL allows.
        ScalarKind::Sint => S::I32(match s {
            S::Bool(v) => i32::from(v),
            S::I32(v) => v,
            S::U32(v) => v as i32,
            S::F32(v) => v as i32,
            S::AbstractInt(v) => v as i32,
            S::AbstractFloat(v) => v as i32,
        }),
        ScalarKind::Uint => S::U32(match s {
            S::Bool(v) => u32::from(v),
            S::I32(v) => v as u32,
            S::U32(v) => v,
            S::F32(v) => v as u32,
            S::AbstractInt(v) => v as u32,
            S::AbstractFloat(v) => v as u32,
        }),
        ScalarKind::Bool => S::Bool(match s {
            S::Bool(v) => v,
            S::F32(v) => v != 0.0,
            S::AbstractFloat(v) => v != 0.0,
            other => other.as_i64() != 0,
        }),
        ScalarKind::AbstractInt => S::AbstractInt(s.as_i64()),
        ScalarKind::AbstractFloat => S::AbstractFloat(s.as_f64()),
    })
}

fn bitcast(s: Scalar, kind: ScalarKind) -> Result<Scalar, InterpError> {
    let bits = match s {
        Scalar::I32(v) => v as u32,
        Scalar::U32(v) => v,
        Scalar::F32(v) => v.to_bits(),
        other => return Err(bad_operands("bitcast".into(), &[other])),
    };
    Ok(match kind {
        ScalarKind::Sint => Scalar::I32(bits as i32),
        ScalarKind::Uint => Scalar::U32(bits),
        ScalarKind::Float => Scalar::F32(f32::from_bits(bits)),
        other => {
            return Err(InterpError::Unsupported(format!("bitcast to {other:?}")));
        }
    })
}

pub(crate) fn relational(fun: RelationalFunction, value: &Val) -> Result<Val, InterpError> {
    match fun {
        RelationalFunction::All | RelationalFunction::Any => {
            let lanes = value
                .lanes()
                .ok_or_else(|| InterpError::Internal("all or any on a composite".into()))?;
            let mut bools = lanes.iter().map(|s| s.as_bool());
            let result = if fun == RelationalFunction::All {
                bools.try_fold(true, |acc, b| b.map(|b| acc && b))?
            } else {
                bools.try_fold(false, |acc, b| b.map(|b| acc || b))?
            };
            Ok(Val::Scalar(Scalar::Bool(result)))
        }
        RelationalFunction::IsNan => map1(value, |s| Ok(Scalar::Bool(s.as_f64().is_nan()))),
        RelationalFunction::IsInf => map1(value, |s| Ok(Scalar::Bool(s.as_f64().is_infinite()))),
    }
}

pub(crate) fn compose(
    module: &naga::Module,
    ty: Handle<naga::Type>,
    components: Vec<Val>,
) -> Result<Val, InterpError> {
    match module.types[ty].inner {
        TypeInner::Vector { size, .. } => {
            let mut lanes = Vec::with_capacity(size as usize);
            for component in &components {
                lanes.extend_from_slice(component.lanes().ok_or_else(|| {
                    InterpError::Internal("vector built from a composite".into())
                })?);
            }
            Ok(Val::Vector(Vector::new(&lanes)))
        }
        TypeInner::Matrix { columns, rows, .. } => {
            if components.len() == columns as usize {
                return Ok(Val::composite(CompositeKind::Matrix, components));
            }
            let scalars: Vec<Scalar> = components
                .iter()
                .filter_map(|c| c.lanes())
                .flatten()
                .copied()
                .collect();
            let columns = scalars
                .chunks(rows as usize)
                .map(|chunk| Val::Vector(Vector::new(chunk)))
                .collect();
            Ok(Val::composite(CompositeKind::Matrix, columns))
        }
        TypeInner::Array { .. } => Ok(Val::composite(CompositeKind::Array, components)),
        TypeInner::Struct { .. } => Ok(Val::composite(CompositeKind::Struct(ty), components)),
        ref other => Err(InterpError::Internal(format!("compose into {other:?}"))),
    }
}

/// Indexes a value. Out-of-range indices clamp, as WebGPU's robust access does.
pub(crate) fn index(base: &Val, index: i64) -> Result<(Val, bool), InterpError> {
    let clamp = |len: usize| -> (usize, bool) {
        let last = len.saturating_sub(1) as i64;
        let i = index.clamp(0, last);
        (i as usize, i != index)
    };
    match base {
        Val::Vector(v) => {
            let (i, clamped) = clamp(usize::from(v.len));
            Ok((Val::Scalar(v.lanes[i]), clamped))
        }
        Val::Composite(c) => {
            let (i, clamped) = clamp(c.items.len());
            Ok((c.items[i].clone(), clamped))
        }
        other => Err(InterpError::Internal(format!("indexing {other:?}"))),
    }
}

pub(crate) fn dot(a: &Val, b: &Val) -> Result<Val, InterpError> {
    let (Some(x), Some(y)) = (a.lanes(), b.lanes()) else {
        return Err(InterpError::Internal("dot of composites".into()));
    };
    let mut sum = scalar_binary(B::Multiply, x[0], y[0])?;
    for (p, q) in x.iter().zip(y).skip(1) {
        sum = scalar_binary(B::Add, sum, scalar_binary(B::Multiply, *p, *q)?)?;
    }
    Ok(Val::Scalar(sum))
}

fn matrix_times_vector(columns: &[Val], vector: &Val) -> Result<Val, InterpError> {
    let lanes = vector
        .lanes()
        .ok_or_else(|| InterpError::Internal("matrix times a composite".into()))?;
    let mut sum: Option<Val> = None;
    for (column, lane) in columns.iter().zip(lanes) {
        let term = binary(B::Multiply, column, &Val::Scalar(*lane))?;
        sum = Some(match sum {
            None => term,
            Some(acc) => binary(B::Add, &acc, &term)?,
        });
    }
    sum.ok_or_else(|| InterpError::Internal("an empty matrix".into()))
}

pub(crate) fn map1(
    value: &Val,
    f: impl Fn(Scalar) -> Result<Scalar, InterpError>,
) -> Result<Val, InterpError> {
    map1_dyn(value, &f)
}

fn map1_dyn(
    value: &Val,
    f: &dyn Fn(Scalar) -> Result<Scalar, InterpError>,
) -> Result<Val, InterpError> {
    match value {
        Val::Scalar(s) => Ok(Val::Scalar(f(*s)?)),
        Val::Vector(v) => Ok(Val::Vector(v.map(f)?)),
        Val::Composite(c) if c.kind == CompositeKind::Matrix => {
            let columns = c
                .items
                .iter()
                .map(|col| map1_dyn(col, f))
                .collect::<Result<_, _>>()?;
            Ok(Val::composite(CompositeKind::Matrix, columns))
        }
        other => Err(InterpError::Internal(format!(
            "component-wise op on {other:?}"
        ))),
    }
}

/// Applies `f` lane by lane, splatting a scalar against a vector.
pub(crate) fn map2(
    a: &Val,
    b: &Val,
    f: impl Fn(Scalar, Scalar) -> Result<Scalar, InterpError>,
) -> Result<Val, InterpError> {
    match (a, b) {
        (Val::Scalar(x), Val::Scalar(y)) => Ok(Val::Scalar(f(*x, *y)?)),
        (Val::Vector(x), Val::Vector(y)) => {
            let mut out = *x;
            for i in 0..usize::from(x.len) {
                out.lanes[i] = f(x.lanes[i], y.lanes[i])?;
            }
            Ok(Val::Vector(out))
        }
        (Val::Vector(x), Val::Scalar(y)) => Ok(Val::Vector(x.map(|l| f(l, *y))?)),
        (Val::Scalar(x), Val::Vector(y)) => Ok(Val::Vector(y.map(|l| f(*x, l))?)),
        _ => Err(InterpError::Internal(format!(
            "component-wise op on {a:?} and {b:?}"
        ))),
    }
}

/// Applies `f` lane by lane over three operands, splatting scalars.
pub(crate) fn map3(
    a: &Val,
    b: &Val,
    c: &Val,
    f: impl Fn(Scalar, Scalar, Scalar) -> Result<Scalar, InterpError>,
) -> Result<Val, InterpError> {
    let len = [a, b, c].iter().find_map(|v| match v {
        Val::Vector(x) => Some(x.len),
        _ => None,
    });
    let lane = |v: &Val, i: usize| -> Result<Scalar, InterpError> {
        match v {
            Val::Scalar(s) => Ok(*s),
            Val::Vector(x) => Ok(x.lanes[i]),
            other => Err(InterpError::Internal(format!(
                "component-wise op on {other:?}"
            ))),
        }
    };
    match len {
        None => Ok(Val::Scalar(f(lane(a, 0)?, lane(b, 0)?, lane(c, 0)?)?)),
        Some(len) => {
            let lanes = (0..usize::from(len))
                .map(|i| f(lane(a, i)?, lane(b, i)?, lane(c, i)?))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::Vector(Vector::new(&lanes)))
        }
    }
}

pub(crate) fn bad_operands(what: String, operands: &[Scalar]) -> InterpError {
    InterpError::Internal(format!("{what} is not defined for {operands:?}"))
}
