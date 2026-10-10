//! WGSL's built-in math, bit and packing functions, computed in f32 as a GPU would.

use texelscope_ir::naga::{self, BinaryOperator as B, MathFunction as M, PredeclaredType};

use crate::InterpError;
use crate::ops::{self, map1, map2, map3};
use crate::val::{CompositeKind, Scalar, Val, Vector};

pub(crate) fn apply(module: &naga::Module, fun: M, args: &[&Val]) -> Result<Val, InterpError> {
    let arg = |i: usize| -> Result<&Val, InterpError> {
        args.get(i)
            .copied()
            .ok_or_else(|| InterpError::Internal(format!("{fun:?} is missing argument {i}")))
    };
    let f1 = |f: fn(f32) -> f32| map1(arg(0)?, move |s| float1(s, f));
    let f2 = |f: fn(f32, f32) -> f32| map2(arg(0)?, arg(1)?, move |a, b| float2(a, b, f));
    match fun {
        M::Abs => map1(arg(0)?, |s| {
            Ok(match s {
                Scalar::F32(v) => Scalar::F32(v.abs()),
                Scalar::I32(v) => Scalar::I32(v.wrapping_abs()),
                Scalar::U32(v) => Scalar::U32(v),
                Scalar::AbstractInt(v) => Scalar::AbstractInt(v.wrapping_abs()),
                Scalar::AbstractFloat(v) => Scalar::AbstractFloat(v.abs()),
                other => return Err(ops::bad_operands("abs".into(), &[other])),
            })
        }),
        M::Min => map2(arg(0)?, arg(1)?, min),
        M::Max => map2(arg(0)?, arg(1)?, max),
        M::Clamp => map3(arg(0)?, arg(1)?, arg(2)?, |e, low, high| {
            min(max(e, low)?, high)
        }),
        M::Saturate => f1(|x| x.clamp(0.0, 1.0)),
        M::Cos => f1(f32::cos),
        M::Cosh => f1(f32::cosh),
        M::Sin => f1(f32::sin),
        M::Sinh => f1(f32::sinh),
        M::Tan => f1(f32::tan),
        M::Tanh => f1(f32::tanh),
        M::Acos => f1(f32::acos),
        M::Asin => f1(f32::asin),
        M::Atan => f1(f32::atan),
        M::Atan2 => f2(f32::atan2),
        M::Asinh => f1(f32::asinh),
        M::Acosh => f1(f32::acosh),
        M::Atanh => f1(f32::atanh),
        M::Radians => f1(|x| x * (std::f32::consts::PI / 180.0)),
        M::Degrees => f1(|x| x * (180.0 / std::f32::consts::PI)),
        M::Ceil => f1(f32::ceil),
        M::Floor => f1(f32::floor),
        M::Round => f1(f32::round_ties_even),
        M::Fract => f1(|x| x - x.floor()),
        M::Trunc => f1(f32::trunc),
        M::Modf => {
            let value = arg(0)?;
            let fract = map1(value, |s| float1(s, |x| x - x.trunc()))?;
            let whole = map1(value, |s| float1(s, f32::trunc))?;
            predeclared(module, value, false, vec![fract, whole])
        }
        M::Frexp => {
            let value = arg(0)?;
            let fract = map1(value, |s| float1(s, |x| frexp(x).0))?;
            let exp = map1(value, |s| Ok(Scalar::I32(frexp(s.as_f32()).1)))?;
            predeclared(module, value, true, vec![fract, exp])
        }
        M::Ldexp => map2(arg(0)?, arg(1)?, |x, e| {
            Ok(Scalar::F32(
                x.as_f32() * 2f32.powi(e.as_i64().clamp(-300, 300) as i32),
            ))
        }),
        M::Exp => f1(f32::exp),
        M::Exp2 => f1(f32::exp2),
        M::Log => f1(f32::ln),
        M::Log2 => f1(f32::log2),
        M::Pow => f2(f32::powf),
        M::Dot => ops::dot(arg(0)?, arg(1)?),
        M::Cross => {
            let (a, b) = (floats(arg(0)?)?, floats(arg(1)?)?);
            Ok(Val::vec_f32(&[
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]))
        }
        M::Distance => {
            let diff = ops::binary(B::Subtract, arg(0)?, arg(1)?)?;
            Ok(Val::f32(length(&floats(&diff)?)))
        }
        M::Length => Ok(Val::f32(length(&floats(arg(0)?)?))),
        M::Normalize => {
            let v = floats(arg(0)?)?;
            let len = length(&v);
            Ok(Val::vec_f32(&v.iter().map(|x| x / len).collect::<Vec<_>>()))
        }
        M::FaceForward => {
            let facing = ops::dot(arg(1)?, arg(2)?)?.scalar()?.as_f32();
            if facing < 0.0 {
                Ok(arg(0)?.clone())
            } else {
                ops::unary(naga::UnaryOperator::Negate, arg(0)?)
            }
        }
        M::Reflect => {
            let (e1, e2) = (floats(arg(0)?)?, floats(arg(1)?)?);
            let d = 2.0 * dot(&e2, &e1);
            Ok(Val::vec_f32(
                &e1.iter()
                    .zip(&e2)
                    .map(|(a, b)| a - d * b)
                    .collect::<Vec<_>>(),
            ))
        }
        M::Refract => {
            let (e1, e2) = (floats(arg(0)?)?, floats(arg(1)?)?);
            let eta = arg(2)?.scalar()?.as_f32();
            let d = dot(&e2, &e1);
            let k = 1.0 - eta * eta * (1.0 - d * d);
            let out: Vec<f32> = if k < 0.0 {
                vec![0.0; e1.len()]
            } else {
                e1.iter()
                    .zip(&e2)
                    .map(|(a, b)| eta * a - (eta * d + k.sqrt()) * b)
                    .collect()
            };
            Ok(Val::vec_f32(&out))
        }
        M::Sign => map1(arg(0)?, |s| {
            Ok(match s {
                Scalar::F32(v) => Scalar::F32(if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    0.0
                }),
                Scalar::I32(v) => Scalar::I32(v.signum()),
                Scalar::AbstractInt(v) => Scalar::AbstractInt(v.signum()),
                Scalar::AbstractFloat(v) => {
                    Scalar::AbstractFloat(if v == 0.0 { 0.0 } else { v.signum() })
                }
                other => return Err(ops::bad_operands("sign".into(), &[other])),
            })
        }),
        M::Fma => map3(arg(0)?, arg(1)?, arg(2)?, |a, b, c| {
            Ok(Scalar::F32(a.as_f32().mul_add(b.as_f32(), c.as_f32())))
        }),
        // The spec's formula, e1 * (1 - e3) + e2 * e3.
        M::Mix => map3(arg(0)?, arg(1)?, arg(2)?, |a, b, t| {
            let (a, b, t) = (a.as_f32(), b.as_f32(), t.as_f32());
            Ok(Scalar::F32(a * (1.0 - t) + b * t))
        }),
        M::Step => map2(arg(0)?, arg(1)?, |edge, x| {
            Ok(Scalar::F32(if edge.as_f32() <= x.as_f32() {
                1.0
            } else {
                0.0
            }))
        }),
        M::SmoothStep => map3(arg(0)?, arg(1)?, arg(2)?, |low, high, x| {
            let (low, high, x) = (low.as_f32(), high.as_f32(), x.as_f32());
            let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
            Ok(Scalar::F32(t * t * (3.0 - 2.0 * t)))
        }),
        M::Sqrt => f1(f32::sqrt),
        M::InverseSqrt => f1(|x| 1.0 / x.sqrt()),
        M::Transpose => {
            let columns = matrix(arg(0)?)?;
            let rows = columns.first().map_or(0, Vec::len);
            let out = (0..rows)
                .map(|r| Val::vec_f32(&columns.iter().map(|c| c[r]).collect::<Vec<_>>()))
                .collect();
            Ok(Val::composite(CompositeKind::Matrix, out))
        }
        M::Determinant => Ok(Val::f32(determinant(&matrix(arg(0)?)?))),
        M::QuantizeToF16 => {
            f1(|x| texelscope_capture::half_to_f32(texelscope_capture::f32_to_half(x)))
        }
        M::CountTrailingZeros => bits1(arg(0)?, |b| b.trailing_zeros()),
        M::CountLeadingZeros => bits1(arg(0)?, |b| b.leading_zeros()),
        M::CountOneBits => bits1(arg(0)?, |b| b.count_ones()),
        M::ReverseBits => bits1(arg(0)?, |b| b.reverse_bits()),
        M::ExtractBits => {
            let offset = arg(1)?.scalar()?.as_i64().clamp(0, 32) as u32;
            let count = (arg(2)?.scalar()?.as_i64().clamp(0, 32) as u32).min(32 - offset);
            map1(arg(0)?, move |s| Ok(extract_bits(s, offset, count)))
        }
        M::InsertBits => {
            let offset = arg(2)?.scalar()?.as_i64().clamp(0, 32) as u32;
            let count = (arg(3)?.scalar()?.as_i64().clamp(0, 32) as u32).min(32 - offset);
            let mask = if count == 0 {
                0
            } else {
                (u32::MAX >> (32 - count)) << offset
            };
            map2(arg(0)?, arg(1)?, move |e, new| {
                let (e_bits, new_bits) = (to_bits(e)?, to_bits(new)?);
                Ok(from_bits(
                    e,
                    (e_bits & !mask) | ((new_bits << offset) & mask),
                ))
            })
        }
        M::FirstTrailingBit => map1(arg(0)?, |s| {
            let bits = to_bits(s)?;
            Ok(from_bits(
                s,
                if bits == 0 {
                    u32::MAX
                } else {
                    bits.trailing_zeros()
                },
            ))
        }),
        M::FirstLeadingBit => map1(arg(0)?, |s| {
            let bits = to_bits(s)?;
            // Signed values look for the first bit that differs from the sign.
            let search = match s {
                Scalar::I32(v) if v < 0 => !bits,
                _ => bits,
            };
            Ok(from_bits(
                s,
                if search == 0 {
                    u32::MAX
                } else {
                    31 - search.leading_zeros()
                },
            ))
        }),
        M::Pack4x8snorm => pack(arg(0)?, 8, |v| {
            ((v.clamp(-1.0, 1.0) * 127.0).round_ties_even() as i32 as u32) & 0xff
        }),
        M::Pack4x8unorm => pack(arg(0)?, 8, |v| {
            (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u32
        }),
        M::Pack2x16snorm => pack(arg(0)?, 16, |v| {
            ((v.clamp(-1.0, 1.0) * 32767.0).round_ties_even() as i32 as u32) & 0xffff
        }),
        M::Pack2x16unorm => pack(arg(0)?, 16, |v| {
            (v.clamp(0.0, 1.0) * 65535.0).round_ties_even() as u32
        }),
        M::Pack2x16float => pack(arg(0)?, 16, |v| {
            u32::from(texelscope_capture::f32_to_half(v))
        }),
        M::Pack4xI8 => pack_ints(arg(0)?, |v| (v as u32) & 0xff),
        M::Pack4xU8 => pack_ints(arg(0)?, |v| (v as u32) & 0xff),
        M::Pack4xI8Clamp => pack_ints(arg(0)?, |v| (v.clamp(-128, 127) as u32) & 0xff),
        M::Pack4xU8Clamp => pack_ints(arg(0)?, |v| v.clamp(0, 255) as u32),
        M::Unpack4x8snorm => unpack(arg(0)?, 8, |b| ((b as u8 as i8) as f32 / 127.0).max(-1.0)),
        M::Unpack4x8unorm => unpack(arg(0)?, 8, |b| b as f32 / 255.0),
        M::Unpack2x16snorm => unpack(arg(0)?, 16, |b| {
            ((b as u16 as i16) as f32 / 32767.0).max(-1.0)
        }),
        M::Unpack2x16unorm => unpack(arg(0)?, 16, |b| b as f32 / 65535.0),
        M::Unpack2x16float => unpack(arg(0)?, 16, |b| texelscope_capture::half_to_f32(b as u16)),
        M::Unpack4xI8 => {
            let bits = to_bits(arg(0)?.scalar()?)?;
            let lanes: Vec<Scalar> = (0..4)
                .map(|i| Scalar::I32(i32::from((bits >> (8 * i)) as u8 as i8)))
                .collect();
            Ok(Val::Vector(Vector::new(&lanes)))
        }
        M::Unpack4xU8 => {
            let bits = to_bits(arg(0)?.scalar()?)?;
            let lanes: Vec<Scalar> = (0..4)
                .map(|i| Scalar::U32((bits >> (8 * i)) & 0xff))
                .collect();
            Ok(Val::Vector(Vector::new(&lanes)))
        }
        M::Dot4I8Packed | M::Dot4U8Packed => {
            let (a, b) = (to_bits(arg(0)?.scalar()?)?, to_bits(arg(1)?.scalar()?)?);
            let signed = fun == M::Dot4I8Packed;
            let byte = |w: u32, i: u32| -> i32 {
                let b = (w >> (8 * i)) as u8;
                if signed {
                    i32::from(b as i8)
                } else {
                    i32::from(b)
                }
            };
            let sum = (0..4).fold(0i32, |acc, i| acc.wrapping_add(byte(a, i) * byte(b, i)));
            Ok(Val::Scalar(if signed {
                Scalar::I32(sum)
            } else {
                Scalar::U32(sum as u32)
            }))
        }
        other => Err(InterpError::Unsupported(format!(
            "the {other:?} builtin is not interpreted yet"
        ))),
    }
}

fn float1(s: Scalar, f: fn(f32) -> f32) -> Result<Scalar, InterpError> {
    match s {
        Scalar::F32(v) => Ok(Scalar::F32(f(v))),
        Scalar::AbstractFloat(v) => Ok(Scalar::AbstractFloat(f64::from(f(v as f32)))),
        other => Err(ops::bad_operands("a float builtin".into(), &[other])),
    }
}

fn float2(a: Scalar, b: Scalar, f: fn(f32, f32) -> f32) -> Result<Scalar, InterpError> {
    Ok(Scalar::F32(f(a.as_f32(), b.as_f32())))
}

fn min(a: Scalar, b: Scalar) -> Result<Scalar, InterpError> {
    pick(a, b, true)
}

fn max(a: Scalar, b: Scalar) -> Result<Scalar, InterpError> {
    pick(a, b, false)
}

fn pick(a: Scalar, b: Scalar, smaller: bool) -> Result<Scalar, InterpError> {
    Ok(match (a, b) {
        (Scalar::F32(x), Scalar::F32(y)) => Scalar::F32(if smaller { x.min(y) } else { x.max(y) }),
        (Scalar::I32(x), Scalar::I32(y)) => Scalar::I32(if smaller { x.min(y) } else { x.max(y) }),
        (Scalar::U32(x), Scalar::U32(y)) => Scalar::U32(if smaller { x.min(y) } else { x.max(y) }),
        _ => {
            let less = ops::scalar_binary(B::Less, a, b)?.as_bool()?;
            if less == smaller { a } else { b }
        }
    })
}

fn floats(value: &Val) -> Result<Vec<f32>, InterpError> {
    value
        .lanes()
        .map(|lanes| lanes.iter().map(|s| s.as_f32()).collect())
        .ok_or_else(|| InterpError::Internal("expected a float vector".into()))
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn length(v: &[f32]) -> f32 {
    dot(v, v).sqrt()
}

fn matrix(value: &Val) -> Result<Vec<Vec<f32>>, InterpError> {
    match value {
        Val::Composite(c) if c.kind == CompositeKind::Matrix => {
            c.items.iter().map(floats).collect()
        }
        other => Err(InterpError::Internal(format!(
            "expected a matrix, found {other:?}"
        ))),
    }
}

fn determinant(m: &[Vec<f32>]) -> f32 {
    match m.len() {
        2 => m[0][0] * m[1][1] - m[1][0] * m[0][1],
        n => (0..n)
            .map(|c| {
                let minor: Vec<Vec<f32>> = m
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| i != c)
                    .map(|(_, col)| col[1..].to_vec())
                    .collect();
                let sign = if c % 2 == 0 { 1.0 } else { -1.0 };
                sign * m[c][0] * determinant(&minor)
            })
            .sum(),
    }
}

/// Splits a float into a fraction in [0.5, 1) and a power of two.
fn frexp(x: f32) -> (f32, i32) {
    if x == 0.0 || !x.is_finite() {
        return (x, 0);
    }
    let exp = x.abs().log2().floor() as i32 + 1;
    let mut fract = x / 2f32.powi(exp);
    let mut exp = exp;
    // log2 can land one off near powers of two.
    if fract.abs() >= 1.0 {
        fract /= 2.0;
        exp += 1;
    } else if fract.abs() < 0.5 {
        fract *= 2.0;
        exp -= 1;
    }
    (fract, exp)
}

fn to_bits(s: Scalar) -> Result<u32, InterpError> {
    match s {
        Scalar::I32(v) => Ok(v as u32),
        Scalar::U32(v) => Ok(v),
        Scalar::AbstractInt(v) => Ok(v as u32),
        other => Err(ops::bad_operands("a bit builtin".into(), &[other])),
    }
}

fn from_bits(like: Scalar, bits: u32) -> Scalar {
    match like {
        Scalar::I32(_) => Scalar::I32(bits as i32),
        _ => Scalar::U32(bits),
    }
}

fn bits1(value: &Val, f: fn(u32) -> u32) -> Result<Val, InterpError> {
    map1(value, move |s| Ok(from_bits(s, f(to_bits(s)?))))
}

fn extract_bits(s: Scalar, offset: u32, count: u32) -> Scalar {
    let bits = match s {
        Scalar::I32(v) => v as u32,
        Scalar::U32(v) => v,
        other => return other,
    };
    if count == 0 {
        return from_bits(s, 0);
    }
    let field = (bits >> offset) & (u32::MAX >> (32 - count));
    match s {
        // Signed extraction copies the field's top bit upward.
        Scalar::I32(_) => {
            let shift = 32 - count;
            Scalar::I32(((field << shift) as i32) >> shift)
        }
        _ => Scalar::U32(field),
    }
}

fn pack(value: &Val, width: u32, f: impl Fn(f32) -> u32) -> Result<Val, InterpError> {
    let packed = floats(value)?
        .iter()
        .enumerate()
        .fold(0u32, |acc, (i, &v)| acc | (f(v) << (width * i as u32)));
    Ok(Val::Scalar(Scalar::U32(packed)))
}

fn pack_ints(value: &Val, f: impl Fn(i64) -> u32) -> Result<Val, InterpError> {
    let lanes = value
        .lanes()
        .ok_or_else(|| InterpError::Internal("pack of a composite".into()))?;
    let packed = lanes
        .iter()
        .enumerate()
        .fold(0u32, |acc, (i, s)| acc | (f(s.as_i64()) << (8 * i as u32)));
    Ok(Val::Scalar(Scalar::U32(packed)))
}

fn unpack(value: &Val, width: u32, f: impl Fn(u32) -> f32) -> Result<Val, InterpError> {
    let bits = to_bits(value.scalar()?)?;
    let count = 32 / width;
    let mask = if width == 32 {
        u32::MAX
    } else {
        (1 << width) - 1
    };
    let lanes: Vec<f32> = (0..count)
        .map(|i| f((bits >> (width * i)) & mask))
        .collect();
    Ok(Val::vec_f32(&lanes))
}

/// Builds a `__modf_result` or `__frexp_result` struct.
fn predeclared(
    module: &naga::Module,
    like: &Val,
    frexp: bool,
    items: Vec<Val>,
) -> Result<Val, InterpError> {
    let size = match like {
        Val::Vector(v) => match v.len {
            2 => Some(naga::VectorSize::Bi),
            3 => Some(naga::VectorSize::Tri),
            _ => Some(naga::VectorSize::Quad),
        },
        _ => None,
    };
    let scalar = naga::Scalar::F32;
    let key = if frexp {
        PredeclaredType::FrexpResult { size, scalar }
    } else {
        PredeclaredType::ModfResult { size, scalar }
    };
    let ty = module
        .special_types
        .predeclared_types
        .get(&key)
        .copied()
        .ok_or_else(|| InterpError::Internal(format!("{key:?} is not declared")))?;
    Ok(Val::composite(CompositeKind::Struct(ty), items))
}

#[cfg(test)]
mod tests {
    use super::frexp;
    use texelscope_capture::{f32_to_half, half_to_f32};

    #[test]
    fn halves_round_trip() {
        for value in [0.0f32, 1.0, -2.5, 0.333_333, 65504.0, 6.1e-5, 1.0e-7] {
            let back = half_to_f32(f32_to_half(value));
            assert!(
                (back - value).abs() <= value.abs() * 1e-3 + 6e-8,
                "{value} came back as {back}"
            );
        }
        assert_eq!(f32_to_half(1.0e6), 0x7c00);
    }

    #[test]
    fn frexp_splits_powers_of_two() {
        assert_eq!(frexp(8.0), (0.5, 4));
        assert_eq!(frexp(-3.0), (-0.75, 2));
        assert_eq!(frexp(0.1), (0.8, -3));
    }
}
