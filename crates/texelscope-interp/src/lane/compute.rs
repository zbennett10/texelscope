//! Evaluating one expression.

use texelscope_ir::naga::{
    self, BinaryOperator, DerivativeAxis, DerivativeControl, Expression, Handle,
};

use super::{Computed, Extra, Lane};
use crate::resources::Global;
use crate::trace::BranchKind;
use crate::val::{Pointer, Root, Scalar, Val, Vector};
use crate::{InterpError, math, ops};

impl<'a> Lane<'a> {
    pub(super) fn compute(&mut self, handle: Handle<Expression>) -> Result<Computed, InterpError> {
        let interp = self.interp;
        let module = self.module();
        let ir = self.top().ir;
        let plain = |v: Val| Ok(Computed::Value(v, Extra::default()));
        match ir.expressions[handle] {
            Expression::Literal(l) => plain(Val::Scalar(Scalar::from_literal(l)?)),
            Expression::Constant(c) => plain(interp.constant(c)?),
            Expression::Override(o) => plain(interp.pipeline_constant(o)?),
            Expression::ZeroValue(ty) => plain(Val::zero(module, ty)?),
            Expression::Compose { ty, ref components } => {
                let items = components
                    .iter()
                    .map(|&c| self.owned(c))
                    .collect::<Result<_, _>>()?;
                plain(ops::compose(module, ty, items)?)
            }
            Expression::Access { base, index } => {
                self.ensure(base)?;
                let index = self.owned(index)?.scalar()?.as_i64();
                self.access(base, index)
            }
            Expression::AccessIndex { base, index } => {
                self.ensure(base)?;
                self.access(base, i64::from(index))
            }
            Expression::Splat { size, value } => {
                let value = self.owned(value)?.scalar()?;
                plain(Val::Vector(Vector::splat(value, size as u8)))
            }
            Expression::Swizzle {
                size,
                vector,
                pattern,
            } => {
                self.ensure(vector)?;
                let Val::Vector(v) = self.value(vector)? else {
                    return Err(InterpError::Internal("swizzle of a non-vector".into()));
                };
                let lanes: Vec<Scalar> = pattern[..size as usize]
                    .iter()
                    .map(|&c| v.lanes[c as usize])
                    .collect();
                plain(Val::Vector(Vector::new(&lanes)))
            }
            Expression::FunctionArgument(i) => plain(self.top().args[i as usize].clone()),
            Expression::GlobalVariable(g) => plain(match &interp.globals[g.index()] {
                Global::Texture { .. } => Val::Image(g),
                Global::Sampler(_) => Val::Sampler(g),
                Global::Missing { .. }
                    if module.global_variables[g].space == naga::AddressSpace::Handle =>
                {
                    match module.types[module.global_variables[g].ty].inner {
                        naga::TypeInner::Sampler { .. } => Val::Sampler(g),
                        _ => Val::Image(g),
                    }
                }
                _ => Val::Pointer(Pointer {
                    root: Root::Global(g),
                    path: Vec::new(),
                }),
            }),
            Expression::LocalVariable(var) => plain(Val::Pointer(Pointer {
                root: Root::Local {
                    frame: (self.frames.len() - 1) as u32,
                    var,
                },
                path: Vec::new(),
            })),
            Expression::Load { pointer } => {
                let pointer = self.owned(pointer)?;
                self.load(pointer.pointer()?, handle)
            }
            Expression::ImageLoad {
                image,
                coordinate,
                array_index,
                sample: _,
                level,
            } => self.image_load(image, coordinate, array_index, level),
            Expression::ImageSample {
                image,
                sampler,
                gather,
                coordinate,
                array_index,
                offset,
                level,
                depth_ref,
                clamp_to_edge,
            } => {
                if depth_ref.is_some() {
                    return Err(InterpError::Unsupported("depth comparison sampling".into()));
                }
                self.image_sample(
                    image,
                    sampler,
                    gather,
                    coordinate,
                    array_index,
                    offset,
                    level,
                    clamp_to_edge,
                )
            }
            Expression::ImageQuery { image, query } => self.image_query(image, query),
            Expression::Unary { op, expr } => {
                self.ensure(expr)?;
                plain(ops::unary(op, self.value(expr)?)?)
            }
            Expression::Binary { op, left, right } => {
                self.ensure(left)?;
                self.ensure(right)?;
                plain(ops::binary(op, self.value(left)?, self.value(right)?)?)
            }
            Expression::Select {
                condition,
                accept,
                reject,
            } => {
                self.ensure(condition)?;
                self.ensure(accept)?;
                self.ensure(reject)?;
                let mut cond = self.value(condition)?.clone();
                if let Val::Scalar(Scalar::Bool(taken)) = cond {
                    let decided = self.note_decision(condition, taken, BranchKind::Select);
                    cond = Val::Scalar(Scalar::Bool(decided));
                }
                plain(ops::select(
                    &cond,
                    self.value(accept)?,
                    self.value(reject)?,
                )?)
            }
            Expression::Derivative { axis, ctrl, expr } => match self.resume.take() {
                Some(quad) => plain(self.derivative(&quad, axis, ctrl)?),
                None => Ok(Computed::Wait(self.owned(expr)?)),
            },
            Expression::Relational { fun, argument } => {
                self.ensure(argument)?;
                plain(ops::relational(fun, self.value(argument)?)?)
            }
            Expression::Math {
                fun,
                arg,
                arg1,
                arg2,
                arg3,
            } => {
                let handles: Vec<Handle<Expression>> = [Some(arg), arg1, arg2, arg3]
                    .into_iter()
                    .flatten()
                    .collect();
                for &h in &handles {
                    self.ensure(h)?;
                }
                let args = handles
                    .iter()
                    .map(|&h| self.value(h))
                    .collect::<Result<Vec<_>, _>>()?;
                plain(math::apply(module, fun, &args)?)
            }
            Expression::As {
                expr,
                kind,
                convert,
            } => {
                self.ensure(expr)?;
                plain(ops::convert(self.value(expr)?, kind, convert)?)
            }
            Expression::CallResult(_) => self.top().values[handle.index()]
                .clone()
                .map(|v| Computed::Value(v, Extra::default()))
                .ok_or_else(|| InterpError::Internal("a call result before its call".into())),
            Expression::ArrayLength(pointer) => {
                let pointer = self.owned(pointer)?;
                let value = self.read(pointer.pointer()?)?.0;
                match value {
                    Val::Composite(c) => plain(Val::Scalar(Scalar::U32(c.items.len() as u32))),
                    _ => Err(InterpError::Internal("arrayLength of a non-array".into())),
                }
            }
            ref other => Err(InterpError::Unsupported(format!(
                "{} is not interpreted yet",
                expression_name(other)
            ))),
        }
    }

    pub(super) fn access(
        &mut self,
        base: Handle<Expression>,
        index: i64,
    ) -> Result<Computed, InterpError> {
        let base = self.value(base)?;
        if let Val::Pointer(p) = base {
            let mut path = p.path.clone();
            path.push(index.max(0) as u32);
            let pointer = Val::Pointer(Pointer { root: p.root, path });
            let note = (index < 0).then(|| format!("index {index} was clamped to 0"));
            return Ok(Computed::Value(
                pointer,
                Extra {
                    note,
                    ..Extra::default()
                },
            ));
        }
        let (value, clamped) = ops::index(base, index)?;
        let note = clamped.then(|| format!("index {index} was out of range and clamped"));
        Ok(Computed::Value(
            value,
            Extra {
                note,
                ..Extra::default()
            },
        ))
    }

    pub(super) fn derivative(
        &self,
        quad: &[Option<Val>; 4],
        axis: DerivativeAxis,
        ctrl: DerivativeControl,
    ) -> Result<Val, InterpError> {
        let own = quad[self.quad]
            .clone()
            .ok_or_else(|| InterpError::Internal("a lane resumed without its operand".into()))?;
        // A lane that has finished contributes this lane's value, so its difference is zero.
        let at = |i: usize| quad[i].as_ref().unwrap_or(&own);
        let (x, y) = (self.quad & 1, self.quad >> 1);
        let coarse = ctrl == DerivativeControl::Coarse;
        let dx = || {
            let row = if coarse { 0 } else { y * 2 };
            ops::binary(BinaryOperator::Subtract, at(row + 1), at(row))
        };
        let dy = || {
            let column = if coarse { 0 } else { x };
            // WebGL counts window y up, so its y derivative runs the other way.
            if self.interp.capture().target.y_up {
                ops::binary(BinaryOperator::Subtract, at(column), at(column + 2))
            } else {
                ops::binary(BinaryOperator::Subtract, at(column + 2), at(column))
            }
        };
        match axis {
            DerivativeAxis::X => dx(),
            DerivativeAxis::Y => dy(),
            DerivativeAxis::Width => {
                let abs = |v: Val| math::apply(self.module(), naga::MathFunction::Abs, &[&v]);
                ops::binary(BinaryOperator::Add, &abs(dx()?)?, &abs(dy()?)?)
            }
        }
    }
}

fn expression_name(expr: &Expression) -> String {
    let debug = format!("{expr:?}");
    debug
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("this expression")
        .to_string()
}
