//! Buffer contents as typed values, read from raw bytes with WGSL's memory layout or built
//! from the member values a capture names.

use texelscope_ir::naga::{self, Handle, ScalarKind, TypeInner, VectorSize};

use crate::InterpError;
use crate::val::{CompositeKind, Scalar, Val, Vector, type_name};

/// Reads a value of type `ty` that starts at `offset`.
pub(crate) fn read(
    module: &naga::Module,
    ty: Handle<naga::Type>,
    bytes: &[u8],
    offset: usize,
) -> Result<Val, InterpError> {
    let inner = &module.types[ty].inner;
    Ok(match *inner {
        TypeInner::Scalar(s) | TypeInner::Atomic(s) => {
            Val::Scalar(read_scalar(module, s, bytes, offset)?)
        }
        TypeInner::Vector { size, scalar } => {
            Val::Vector(read_vector(module, size, scalar, bytes, offset)?)
        }
        TypeInner::Matrix {
            columns,
            rows,
            scalar,
        } => {
            let stride = column_stride(rows, scalar);
            let columns = (0..columns as usize)
                .map(|c| {
                    read_vector(module, rows, scalar, bytes, offset + c * stride).map(Val::Vector)
                })
                .collect::<Result<_, _>>()?;
            Val::composite(CompositeKind::Matrix, columns)
        }
        TypeInner::Array { base, size, stride } => {
            let count = match size {
                naga::ArraySize::Constant(n) => n.get() as usize,
                naga::ArraySize::Dynamic => {
                    bytes.len().saturating_sub(offset) / stride.max(1) as usize
                }
                naga::ArraySize::Pending(_) => {
                    return Err(InterpError::Unsupported(
                        "arrays sized by pipeline overrides are not interpreted yet".into(),
                    ));
                }
            };
            let items = (0..count)
                .map(|i| read(module, base, bytes, offset + i * stride as usize))
                .collect::<Result<_, _>>()?;
            Val::composite(CompositeKind::Array, items)
        }
        TypeInner::Struct { ref members, .. } => {
            let items = members
                .iter()
                .map(|m| read(module, m.ty, bytes, offset + m.offset as usize))
                .collect::<Result<_, _>>()?;
            Val::composite(CompositeKind::Struct(ty), items)
        }
        ref other => {
            return Err(InterpError::Unsupported(format!(
                "{} is not host-shareable",
                type_name(module, other)
            )));
        }
    })
}

/// Writes a value of type `ty` at `offset`, growing `bytes` as needed. Padding stays zero.
pub(crate) fn write(
    module: &naga::Module,
    ty: Handle<naga::Type>,
    value: &Val,
    bytes: &mut Vec<u8>,
    offset: usize,
) -> Result<(), InterpError> {
    let mut put = |at: usize, s: Scalar| -> Result<(), InterpError> {
        let word = match s {
            Scalar::F32(v) => v.to_bits(),
            Scalar::I32(v) => v as u32,
            Scalar::U32(v) => v,
            Scalar::Bool(v) => u32::from(v),
            other => return Err(InterpError::Internal(format!("{other:?} in a buffer"))),
        };
        if bytes.len() < at + 4 {
            bytes.resize(at + 4, 0);
        }
        bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
        Ok(())
    };
    match (&module.types[ty].inner, value) {
        (TypeInner::Scalar(_) | TypeInner::Atomic(_), Val::Scalar(s)) => put(offset, *s),
        (TypeInner::Vector { .. }, Val::Vector(v)) => {
            for (i, lane) in v.lanes().iter().enumerate() {
                put(offset + i * 4, *lane)?;
            }
            Ok(())
        }
        (TypeInner::Matrix { rows, scalar, .. }, Val::Composite(c)) => {
            let stride = column_stride(*rows, *scalar);
            for (i, column) in c.items.iter().enumerate() {
                for (j, lane) in column.lanes().unwrap_or_default().iter().enumerate() {
                    put(offset + i * stride + j * 4, *lane)?;
                }
            }
            Ok(())
        }
        (TypeInner::Array { base, stride, .. }, Val::Composite(c)) => {
            for (i, item) in c.items.iter().enumerate() {
                write(module, *base, item, bytes, offset + i * *stride as usize)?;
            }
            Ok(())
        }
        (TypeInner::Struct { members, .. }, Val::Composite(c)) => {
            for (member, item) in members.iter().zip(c.items.iter()) {
                write(
                    module,
                    member.ty,
                    item,
                    bytes,
                    offset + member.offset as usize,
                )?;
            }
            Ok(())
        }
        (other, value) => Err(InterpError::Internal(format!(
            "writing {value:?} as {}",
            type_name(module, other)
        ))),
    }
}

/// Builds a value of type `ty` from the JSON a capture gives. `path` names it in errors.
pub(crate) fn from_json(
    module: &naga::Module,
    ty: Handle<naga::Type>,
    json: &serde_json::Value,
    path: &str,
) -> Result<Val, InterpError> {
    let inner = &module.types[ty].inner;
    let wrong = || {
        InterpError::BadData(format!(
            "{path} should be a {}, found {json}",
            type_name(module, inner)
        ))
    };
    Ok(match *inner {
        TypeInner::Scalar(s) | TypeInner::Atomic(s) => {
            Val::Scalar(json_scalar(json, s.kind).ok_or_else(wrong)?)
        }
        TypeInner::Vector { size, scalar } => {
            let items = json
                .as_array()
                .filter(|a| a.len() == size as usize)
                .ok_or_else(wrong)?;
            let lanes = items
                .iter()
                .map(|v| json_scalar(v, scalar.kind))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(wrong)?;
            Val::Vector(Vector::new(&lanes))
        }
        TypeInner::Matrix {
            columns,
            rows,
            scalar,
        } => {
            let items = json.as_array().ok_or_else(wrong)?;
            // Either one array per column or every component in column order.
            let flat: Vec<&serde_json::Value> = if items.iter().all(|v| v.is_array()) {
                items
                    .iter()
                    .flat_map(|c| c.as_array().into_iter().flatten())
                    .collect()
            } else {
                items.iter().collect()
            };
            if flat.len() != columns as usize * rows as usize {
                return Err(wrong());
            }
            let scalars = flat
                .iter()
                .map(|v| json_scalar(v, scalar.kind))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(wrong)?;
            let columns = scalars
                .chunks(rows as usize)
                .map(|c| Val::Vector(Vector::new(c)))
                .collect();
            Val::composite(CompositeKind::Matrix, columns)
        }
        TypeInner::Array { base, size, .. } => {
            let items = json.as_array().ok_or_else(wrong)?;
            if let naga::ArraySize::Constant(n) = size
                && items.len() != n.get() as usize
            {
                return Err(InterpError::BadData(format!(
                    "{path} should have {n} elements, found {}",
                    items.len()
                )));
            }
            let values = items
                .iter()
                .enumerate()
                .map(|(i, v)| from_json(module, base, v, &format!("{path}[{i}]")))
                .collect::<Result<_, _>>()?;
            Val::composite(CompositeKind::Array, values)
        }
        TypeInner::Struct { ref members, .. } => {
            let object = json.as_object().ok_or_else(wrong)?;
            let missing: Vec<&str> = members
                .iter()
                .filter_map(|m| m.name.as_deref())
                .filter(|name| !object.contains_key(*name))
                .collect();
            if !missing.is_empty() {
                return Err(InterpError::BadData(format!(
                    "{path} is missing {}",
                    missing.join(", ")
                )));
            }
            let items = members
                .iter()
                .map(|m| {
                    let name = m.name.as_deref().unwrap_or_default();
                    from_json(module, m.ty, &object[name], &format!("{path}.{name}"))
                })
                .collect::<Result<_, _>>()?;
            Val::composite(CompositeKind::Struct(ty), items)
        }
        _ => return Err(wrong()),
    })
}

fn json_scalar(json: &serde_json::Value, kind: ScalarKind) -> Option<Scalar> {
    let number = match json {
        serde_json::Value::Number(n) => n.as_f64()?,
        serde_json::Value::Bool(b) if kind == ScalarKind::Bool => return Some(Scalar::Bool(*b)),
        // A GLSL bool uniform is held as an unsigned int.
        serde_json::Value::Bool(b) => f64::from(u8::from(*b)),
        serde_json::Value::String(s) => match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            _ => return None,
        },
        _ => return None,
    };
    Some(match kind {
        ScalarKind::Float | ScalarKind::AbstractFloat => Scalar::F32(number as f32),
        ScalarKind::Sint | ScalarKind::AbstractInt if number.fract() == 0.0 => {
            Scalar::I32(number as i32)
        }
        ScalarKind::Uint if number.fract() == 0.0 && number >= 0.0 => Scalar::U32(number as u32),
        ScalarKind::Bool => Scalar::Bool(number != 0.0),
        _ => return None,
    })
}

fn read_scalar(
    module: &naga::Module,
    scalar: naga::Scalar,
    bytes: &[u8],
    offset: usize,
) -> Result<Scalar, InterpError> {
    let width = usize::from(scalar.width);
    let Some(raw) = bytes.get(offset..offset + width) else {
        return Err(InterpError::BadData(format!(
            "the buffer is {} bytes, but the shader reads a {} at byte {offset}",
            bytes.len(),
            type_name(module, &TypeInner::Scalar(scalar))
        )));
    };
    let word = || u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    Ok(match (scalar.kind, scalar.width) {
        (ScalarKind::Float, 4) => Scalar::F32(f32::from_bits(word())),
        (ScalarKind::Sint, 4) => Scalar::I32(word() as i32),
        (ScalarKind::Uint, 4) => Scalar::U32(word()),
        (kind, width) => {
            return Err(InterpError::Unsupported(format!(
                "reading {width}-byte {kind:?} values from buffers"
            )));
        }
    })
}

fn read_vector(
    module: &naga::Module,
    size: VectorSize,
    scalar: naga::Scalar,
    bytes: &[u8],
    offset: usize,
) -> Result<Vector, InterpError> {
    let width = usize::from(scalar.width);
    let lanes = (0..size as usize)
        .map(|i| read_scalar(module, scalar, bytes, offset + i * width))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Vector::new(&lanes))
}

/// Matrix columns are laid out as vectors, so three-row columns take the room of four.
fn column_stride(rows: VectorSize, scalar: naga::Scalar) -> usize {
    let width = usize::from(scalar.width);
    match rows {
        VectorSize::Bi => 2 * width,
        VectorSize::Tri | VectorSize::Quad => 4 * width,
    }
}
