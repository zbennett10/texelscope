//! Runtime values. Vectors live inline and composites share their items, so copying a
//! value is cheap.

use std::sync::Arc;

use texelscope_ir::Value;
use texelscope_ir::naga::{self, Handle, ScalarKind, TypeInner};

use crate::InterpError;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Scalar {
    Bool(bool),
    I32(i32),
    U32(u32),
    F32(f32),
    AbstractInt(i64),
    AbstractFloat(f64),
}

impl Scalar {
    pub(crate) fn zero(scalar: naga::Scalar) -> Result<Self, InterpError> {
        Ok(match (scalar.kind, scalar.width) {
            (ScalarKind::Bool, _) => Self::Bool(false),
            (ScalarKind::Sint, 4) => Self::I32(0),
            (ScalarKind::Uint, 4) => Self::U32(0),
            (ScalarKind::Float, 4) => Self::F32(0.0),
            (ScalarKind::AbstractInt, _) => Self::AbstractInt(0),
            (ScalarKind::AbstractFloat, _) => Self::AbstractFloat(0.0),
            (kind, width) => return Err(unsupported_scalar(kind, width)),
        })
    }

    pub(crate) fn from_literal(literal: naga::Literal) -> Result<Self, InterpError> {
        use naga::Literal as L;
        Ok(match literal {
            L::Bool(v) => Self::Bool(v),
            L::I32(v) => Self::I32(v),
            L::U32(v) => Self::U32(v),
            L::F32(v) => Self::F32(v),
            L::AbstractInt(v) => Self::AbstractInt(v),
            L::AbstractFloat(v) => Self::AbstractFloat(v),
            other => {
                return Err(InterpError::Unsupported(format!(
                    "{other:?} literals are not interpreted yet"
                )));
            }
        })
    }

    pub(crate) fn as_f64(self) -> f64 {
        match self {
            Self::Bool(v) => f64::from(u8::from(v)),
            Self::I32(v) => f64::from(v),
            Self::U32(v) => f64::from(v),
            Self::F32(v) => f64::from(v),
            Self::AbstractInt(v) => v as f64,
            Self::AbstractFloat(v) => v,
        }
    }

    pub(crate) fn as_f32(self) -> f32 {
        self.as_f64() as f32
    }

    pub(crate) fn as_i64(self) -> i64 {
        match self {
            Self::Bool(v) => i64::from(v),
            Self::I32(v) => i64::from(v),
            Self::U32(v) => i64::from(v),
            Self::F32(v) => v as i64,
            Self::AbstractInt(v) => v,
            Self::AbstractFloat(v) => v as i64,
        }
    }

    pub(crate) fn as_bool(self) -> Result<bool, InterpError> {
        match self {
            Self::Bool(v) => Ok(v),
            other => Err(InterpError::Internal(format!(
                "expected a bool, found {other:?}"
            ))),
        }
    }

    pub(crate) fn to_value(self) -> Value {
        match self {
            Self::Bool(v) => Value::Bool(v),
            Self::I32(v) => Value::I32(v),
            Self::U32(v) => Value::U32(v),
            Self::F32(v) => Value::F32(v),
            Self::AbstractInt(v) => Value::I32(v as i32),
            Self::AbstractFloat(v) => Value::F32(v as f32),
        }
    }
}

/// Up to four lanes of one scalar kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Vector {
    pub(crate) len: u8,
    pub(crate) lanes: [Scalar; 4],
}

impl Vector {
    pub(crate) fn new(items: &[Scalar]) -> Self {
        let mut lanes = [Scalar::Bool(false); 4];
        lanes[..items.len()].copy_from_slice(items);
        Self {
            len: items.len() as u8,
            lanes,
        }
    }

    pub(crate) fn splat(value: Scalar, len: u8) -> Self {
        Self {
            len,
            lanes: [value; 4],
        }
    }

    pub(crate) fn lanes(&self) -> &[Scalar] {
        &self.lanes[..usize::from(self.len)]
    }

    pub(crate) fn map(
        &self,
        mut f: impl FnMut(Scalar) -> Result<Scalar, InterpError>,
    ) -> Result<Self, InterpError> {
        let mut out = *self;
        for lane in &mut out.lanes[..usize::from(self.len)] {
            *lane = f(*lane)?;
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompositeKind {
    /// Columns, each a vector.
    Matrix,
    Array,
    Struct(Handle<naga::Type>),
}

#[derive(Clone, Debug)]
pub(crate) struct Composite {
    pub(crate) kind: CompositeKind,
    pub(crate) items: Arc<Vec<Val>>,
}

/// Where a pointer starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Root {
    Local {
        frame: u32,
        var: Handle<naga::LocalVariable>,
    },
    Global(Handle<naga::GlobalVariable>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pointer {
    pub(crate) root: Root,
    /// Member, element or lane indices from the root.
    pub(crate) path: Vec<u32>,
}

#[derive(Clone, Debug)]
pub(crate) enum Val {
    Scalar(Scalar),
    Vector(Vector),
    Composite(Composite),
    Pointer(Pointer),
    Image(Handle<naga::GlobalVariable>),
    Sampler(Handle<naga::GlobalVariable>),
}

impl Val {
    pub(crate) fn f32(v: f32) -> Self {
        Self::Scalar(Scalar::F32(v))
    }

    pub(crate) fn composite(kind: CompositeKind, items: Vec<Val>) -> Self {
        Self::Composite(Composite {
            kind,
            items: Arc::new(items),
        })
    }

    pub(crate) fn vec_f32(items: &[f32]) -> Self {
        let scalars: Vec<Scalar> = items.iter().map(|&v| Scalar::F32(v)).collect();
        Self::Vector(Vector::new(&scalars))
    }

    pub(crate) fn scalar(&self) -> Result<Scalar, InterpError> {
        match self {
            Self::Scalar(s) => Ok(*s),
            other => Err(InterpError::Internal(format!(
                "expected a scalar, found {other:?}"
            ))),
        }
    }

    pub(crate) fn as_bool(&self) -> Result<bool, InterpError> {
        self.scalar()?.as_bool()
    }

    pub(crate) fn pointer(&self) -> Result<&Pointer, InterpError> {
        match self {
            Self::Pointer(p) => Ok(p),
            other => Err(InterpError::Internal(format!(
                "expected a pointer, found {other:?}"
            ))),
        }
    }

    /// Scalar or vector lanes, for values that have them.
    pub(crate) fn lanes(&self) -> Option<&[Scalar]> {
        match self {
            Self::Scalar(s) => Some(std::slice::from_ref(s)),
            Self::Vector(v) => Some(v.lanes()),
            _ => None,
        }
    }

    pub(crate) fn zero(module: &naga::Module, ty: Handle<naga::Type>) -> Result<Self, InterpError> {
        Self::zero_inner(module, &module.types[ty].inner, Some(ty))
    }

    pub(crate) fn zero_inner(
        module: &naga::Module,
        inner: &TypeInner,
        handle: Option<Handle<naga::Type>>,
    ) -> Result<Self, InterpError> {
        Ok(match *inner {
            TypeInner::Scalar(scalar) | TypeInner::Atomic(scalar) => {
                Self::Scalar(Scalar::zero(scalar)?)
            }
            TypeInner::Vector { size, scalar } => {
                Self::Vector(Vector::splat(Scalar::zero(scalar)?, size as u8))
            }
            TypeInner::Matrix {
                columns,
                rows,
                scalar,
            } => {
                let column = Self::Vector(Vector::splat(Scalar::zero(scalar)?, rows as u8));
                Self::composite(CompositeKind::Matrix, vec![column; columns as usize])
            }
            TypeInner::Array { base, size, .. } => {
                let len = array_len(module, size)?;
                let item = Self::zero(module, base)?;
                Self::composite(CompositeKind::Array, vec![item; len])
            }
            TypeInner::Struct { ref members, .. } => {
                let Some(handle) = handle else {
                    return Err(InterpError::Internal(
                        "a struct type without a handle".into(),
                    ));
                };
                let items = members
                    .iter()
                    .map(|m| Self::zero(module, m.ty))
                    .collect::<Result<_, _>>()?;
                Self::composite(CompositeKind::Struct(handle), items)
            }
            ref other => {
                return Err(InterpError::Unsupported(format!(
                    "zero values of {other:?} are not interpreted"
                )));
            }
        })
    }

    /// The value as Texelscope reports it, or `None` for pointers and resource handles.
    pub(crate) fn to_value(&self, module: &naga::Module) -> Option<Value> {
        Some(match self {
            Self::Scalar(s) => s.to_value(),
            Self::Vector(v) => Value::Vector(v.lanes().iter().map(|s| s.to_value()).collect()),
            Self::Composite(c) => {
                let items = c.items.iter().map(|v| v.to_value(module));
                match c.kind {
                    CompositeKind::Matrix => Value::Matrix(items.collect::<Option<_>>()?),
                    CompositeKind::Array => Value::Array(items.collect::<Option<_>>()?),
                    CompositeKind::Struct(ty) => {
                        let TypeInner::Struct { members, .. } = &module.types[ty].inner else {
                            return None;
                        };
                        let named = members
                            .iter()
                            .zip(items)
                            .map(|(m, v)| Some((m.name.clone().unwrap_or_default(), v?)));
                        Value::Struct(named.collect::<Option<_>>()?)
                    }
                }
            }
            Self::Pointer(_) | Self::Image(_) | Self::Sampler(_) => return None,
        })
    }

    /// Builds a runtime value from a reported one, shaped by the type it replaces.
    pub(crate) fn from_value(
        module: &naga::Module,
        value: &Value,
        inner: &TypeInner,
    ) -> Result<Self, InterpError> {
        let mismatch = || {
            InterpError::BadData(format!(
                "{value} does not fit a value of type {}",
                type_name(module, inner)
            ))
        };
        let scalar = |value: &Value, kind: ScalarKind| -> Result<Scalar, InterpError> {
            let s = match value {
                Value::Bool(v) => Scalar::Bool(*v),
                Value::I32(v) => Scalar::I32(*v),
                Value::U32(v) => Scalar::U32(*v),
                Value::F32(v) => Scalar::F32(*v),
                _ => return Err(mismatch()),
            };
            // Numbers convert to the kind they replace, so a pin can be typed loosely.
            crate::ops::convert_scalar(s, kind)
        };
        Ok(match (value, inner) {
            (_, TypeInner::Scalar(s)) => Self::Scalar(scalar(value, s.kind)?),
            (Value::Vector(items), TypeInner::Vector { size, scalar: s })
                if items.len() == *size as usize =>
            {
                let lanes = items
                    .iter()
                    .map(|v| scalar(v, s.kind))
                    .collect::<Result<Vec<_>, _>>()?;
                Self::Vector(Vector::new(&lanes))
            }
            (
                Value::Matrix(columns),
                TypeInner::Matrix {
                    columns: n,
                    rows,
                    scalar: s,
                },
            ) if columns.len() == *n as usize => {
                let column = TypeInner::Vector {
                    size: *rows,
                    scalar: *s,
                };
                let items = columns
                    .iter()
                    .map(|c| Self::from_value(module, c, &column))
                    .collect::<Result<_, _>>()?;
                Self::composite(CompositeKind::Matrix, items)
            }
            (Value::Array(items), TypeInner::Array { base, .. }) => {
                let base = &module.types[*base].inner;
                let items = items
                    .iter()
                    .map(|v| Self::from_value(module, v, base))
                    .collect::<Result<_, _>>()?;
                Self::composite(CompositeKind::Array, items)
            }
            _ => return Err(mismatch()),
        })
    }
}

/// The element count of a fixed-size array.
pub(crate) fn array_len(
    module: &naga::Module,
    size: naga::ArraySize,
) -> Result<usize, InterpError> {
    match size {
        naga::ArraySize::Constant(n) => Ok(n.get() as usize),
        naga::ArraySize::Pending(_) => Err(InterpError::Unsupported(
            "arrays sized by pipeline overrides are not interpreted yet".into(),
        )),
        naga::ArraySize::Dynamic => {
            let _ = module;
            Err(InterpError::Internal(
                "runtime-sized arrays only live in storage buffers".into(),
            ))
        }
    }
}

/// A WGSL-like name for a type, for messages.
pub(crate) fn type_name(module: &naga::Module, inner: &TypeInner) -> String {
    let scalar = |s: naga::Scalar| match (s.kind, s.width) {
        (ScalarKind::Bool, _) => "bool".to_string(),
        (ScalarKind::Sint, w) => format!("i{}", u32::from(w) * 8),
        (ScalarKind::Uint, w) => format!("u{}", u32::from(w) * 8),
        (ScalarKind::Float, w) => format!("f{}", u32::from(w) * 8),
        (ScalarKind::AbstractInt, _) => "abstract-int".to_string(),
        (ScalarKind::AbstractFloat, _) => "abstract-float".to_string(),
    };
    match *inner {
        TypeInner::Scalar(s) => scalar(s),
        TypeInner::Vector { size, scalar: s } => format!("vec{}<{}>", size as u8, scalar(s)),
        TypeInner::Matrix {
            columns,
            rows,
            scalar: s,
        } => format!("mat{}x{}<{}>", columns as u8, rows as u8, scalar(s)),
        TypeInner::Array { base, size, .. } => {
            let base = type_name(module, &module.types[base].inner);
            match size {
                naga::ArraySize::Constant(n) => format!("array<{base}, {n}>"),
                _ => format!("array<{base}>"),
            }
        }
        TypeInner::Struct { .. } => "struct".to_string(),
        ref other => format!("{other:?}"),
    }
}

fn unsupported_scalar(kind: ScalarKind, width: u8) -> InterpError {
    InterpError::Unsupported(format!(
        "{width}-byte {kind:?} values are not interpreted yet"
    ))
}
