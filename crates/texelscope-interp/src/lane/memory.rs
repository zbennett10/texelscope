//! Reads and writes through pointers, and what each store leaves behind for the trace.

use std::sync::Arc;

use texelscope_ir::naga::{self, Expression, Handle};

use super::{Computed, Extra, Lane, NO_STEP};
use crate::InterpError;
use crate::Perturbation;
use crate::record::StoreTarget;
use crate::resources::Global;
use crate::trace::Origin;
use crate::val::{Pointer, Root, Scalar, Val};

impl<'a> Lane<'a> {
    pub(super) fn load(
        &mut self,
        pointer: &Pointer,
        handle: Handle<Expression>,
    ) -> Result<Computed, InterpError> {
        let (value, clamped) = self.read(pointer)?;
        let mut extra = Extra {
            note: clamped.then(|| "an index was out of range and clamped".to_string()),
            ..Extra::default()
        };
        if let Root::Global(g) = pointer.root
            && let Global::Buffer { group, binding, .. } = self.interp.globals[g.index()]
        {
            extra.origin = Some(Origin::Buffer {
                group,
                binding,
                path: self.interp.global_path_label(g, &pointer.path),
                indices: pointer.path.clone(),
            });
        }
        if self.record.wants_inputs() {
            let mut inputs = self.pointer_index_steps(handle);
            inputs.extend(self.stores_into(pointer));
            inputs.sort_unstable();
            inputs.dedup();
            extra.inputs = Some(inputs);
        }
        Ok(Computed::Value(value, extra))
    }

    /// Steps of the dynamic indices along the pointer a load reads through.
    pub(super) fn pointer_index_steps(&self, load: Handle<Expression>) -> Vec<u32> {
        let frame = self.top();
        let mut steps = Vec::new();
        let mut at = match frame.ir.expressions[load] {
            Expression::Load { pointer } => pointer,
            _ => return steps,
        };
        loop {
            match frame.ir.expressions[at] {
                Expression::Access { base, index } => {
                    if let Some(step) = self.step_of(index) {
                        steps.push(step);
                    }
                    at = base;
                }
                Expression::AccessIndex { base, .. } => at = base,
                Expression::FunctionArgument(i) => {
                    if let Some(&step) = frame.arg_steps.get(i as usize).filter(|&&s| s != NO_STEP)
                    {
                        steps.push(step);
                    }
                    break;
                }
                _ => break,
            }
        }
        steps
    }

    /// Steps that stored into memory overlapping a pointer.
    pub(super) fn stores_into(&self, pointer: &Pointer) -> Vec<u32> {
        let log = match pointer.root {
            Root::Local { frame, var } => &self.frames[frame as usize].local_stores[var.index()],
            Root::Global(g) => match self.private_stores.get(&g) {
                Some(log) => log,
                None => return Vec::new(),
            },
        };
        log.iter()
            .filter(|(path, _)| path.starts_with(&pointer.path) || pointer.path.starts_with(path))
            .map(|&(_, step)| step)
            .collect()
    }

    pub(super) fn read(&self, pointer: &Pointer) -> Result<(Val, bool), InterpError> {
        let root = match pointer.root {
            Root::Local { frame, var } => &self.frames[frame as usize].locals[var.index()],
            Root::Global(g) => match &self.interp.globals[g.index()] {
                Global::Private(init) => self.privates.get(&g).unwrap_or(init),
                Global::Buffer { value, .. } => self.writes.get(&g).unwrap_or(value),
                other => return Err(self.interp.global_error(g, other)),
            },
        };
        let (mut value, clamped) = get_path(root, &pointer.path)?;
        if let (
            Root::Global(g),
            Some(Perturbation::Buffer {
                group,
                binding,
                indices,
                delta,
            }),
        ) = (pointer.root, self.nudge)
            && let Global::Buffer {
                group: bg,
                binding: bb,
                ..
            } = self.interp.globals[g.index()]
            && (bg, bb) == (*group, *binding)
            && indices.starts_with(&pointer.path)
        {
            nudge_path(&mut value, &indices[pointer.path.len()..], *delta);
        }
        Ok((value, clamped))
    }

    pub(super) fn write(&mut self, pointer: &Pointer, value: Val) -> Result<(), InterpError> {
        let slot = match pointer.root {
            Root::Local { frame, var } => &mut self.frames[frame as usize].locals[var.index()],
            Root::Global(g) => match &self.interp.globals[g.index()] {
                Global::Private(init) => self.privates.entry(g).or_insert_with(|| init.clone()),
                Global::Buffer {
                    value: initial,
                    storage: true,
                    ..
                } => self.writes.entry(g).or_insert_with(|| initial.clone()),
                other => return Err(self.interp.global_error(g, other)),
            },
        };
        set_path(slot, &pointer.path, value)
    }

    pub(super) fn store(
        &mut self,
        pointer: Handle<Expression>,
        value: Handle<Expression>,
        pc: usize,
    ) -> Result<(), InterpError> {
        let target = self.owned(pointer)?;
        let target = target.pointer()?.clone();
        let stored = self.owned(value)?;
        self.write(&target, stored.clone())?;
        let step = self.step_of(value);
        // A stored value depends on what computed it and on the branch it was stored under.
        let guard = self.guard_at(pc);
        if self.record.wants_inputs() {
            let log = match target.root {
                Root::Local { frame, var } => {
                    &mut self.frames[frame as usize].local_stores[var.index()]
                }
                Root::Global(g) => self.private_stores.entry(g).or_default(),
            };
            log.retain(|(path, _)| !path.starts_with(&target.path));
            for source in step.into_iter().chain(guard) {
                log.push((target.path.clone(), source));
            }
        }
        let named = match target.root {
            Root::Local { frame, var } => self.frames[frame as usize].ir.local_variables[var]
                .name
                .is_some(),
            Root::Global(_) => true,
        };
        if self.record.is_on() && named {
            let label = self.pointer_label(&target);
            let span = self.top().compiled.op_spans[pc];
            let depth = self.frames.len() as u32 - 1;
            let function = self.top().compiled.function;
            let call = self.top().call;
            let module = self.module();
            self.record.store(
                module,
                StoreTarget {
                    function,
                    label,
                    span,
                    depth,
                    call,
                    guard,
                },
                &stored,
                step,
            );
        }
        Ok(())
    }

    pub(super) fn pointer_label(&self, pointer: &Pointer) -> String {
        match pointer.root {
            Root::Local { frame, var } => {
                let frame = &self.frames[frame as usize];
                let local = &frame.ir.local_variables[var];
                let name = local.name.as_deref().unwrap_or("_");
                path_label(self.module(), name, local.ty, &pointer.path)
            }
            Root::Global(g) => self.interp.global_path_label(g, &pointer.path),
        }
    }
}

fn get_path(root: &Val, path: &[u32]) -> Result<(Val, bool), InterpError> {
    let mut current = root;
    let mut clamped = false;
    for (depth, &index) in path.iter().enumerate() {
        match current {
            Val::Composite(c) => {
                let i = (index as usize).min(c.items.len().saturating_sub(1));
                clamped |= i != index as usize;
                current = &c.items[i];
            }
            Val::Vector(v) if depth + 1 == path.len() => {
                let i = (index as usize).min(usize::from(v.len) - 1);
                clamped |= i != index as usize;
                return Ok((Val::Scalar(v.lanes[i]), clamped));
            }
            other => return Err(InterpError::Internal(format!("path into {other:?}"))),
        }
    }
    Ok((current.clone(), clamped))
}

fn set_path(slot: &mut Val, path: &[u32], value: Val) -> Result<(), InterpError> {
    let Some((&index, rest)) = path.split_first() else {
        *slot = value;
        return Ok(());
    };
    match slot {
        Val::Composite(c) => {
            let items = Arc::make_mut(&mut c.items);
            let i = (index as usize).min(items.len().saturating_sub(1));
            set_path(&mut items[i], rest, value)
        }
        Val::Vector(v) if rest.is_empty() => {
            let i = (index as usize).min(usize::from(v.len) - 1);
            v.lanes[i] = value.scalar()?;
            Ok(())
        }
        other => Err(InterpError::Internal(format!("storing through {other:?}"))),
    }
}

fn nudge_path(value: &mut Val, path: &[u32], delta: f32) {
    let bump = |s: &mut Scalar| {
        if let Scalar::F32(v) = s {
            *v += delta;
        }
    };
    match (value, path.split_first()) {
        (Val::Scalar(s), None) => bump(s),
        (Val::Vector(v), Some((&i, []))) => {
            if let Some(s) = v.lanes.get_mut(i as usize) {
                bump(s);
            }
        }
        (Val::Composite(c), Some((&i, rest))) => {
            if let Some(item) = Arc::make_mut(&mut c.items).get_mut(i as usize) {
                nudge_path(item, rest, delta);
            }
        }
        _ => {}
    }
}

pub(crate) fn path_label(
    module: &naga::Module,
    name: &str,
    ty: Handle<naga::Type>,
    path: &[u32],
) -> String {
    let mut label = name.to_string();
    let mut inner = Some(&module.types[ty].inner);
    for &index in path {
        match inner {
            Some(naga::TypeInner::Struct { members, .. }) => {
                let member = &members[(index as usize).min(members.len() - 1)];
                label.push('.');
                label.push_str(member.name.as_deref().unwrap_or("_"));
                inner = Some(&module.types[member.ty].inner);
            }
            Some(naga::TypeInner::Array { base, .. }) => {
                label.push_str(&format!("[{index}]"));
                inner = Some(&module.types[*base].inner);
            }
            // A matrix column is a vector, which the next index picks a lane of.
            Some(naga::TypeInner::Matrix { .. }) => {
                label.push_str(&format!("[{index}]"));
                inner = None;
            }
            _ => {
                label.push('.');
                label.push(['x', 'y', 'z', 'w'][(index as usize).min(3)]);
            }
        }
    }
    label
}
