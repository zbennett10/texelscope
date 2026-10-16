//! One fragment invocation. A lane runs the flattened ops of its call stack until it
//! finishes or reaches an operation that needs the other lanes of its quad.

mod compute;
mod control;
mod memory;
mod texture;

pub(crate) use memory::path_label;

use std::collections::HashMap;

use texelscope_ir::naga::{self, Expression, Handle};
use texelscope_ir::{ExprRef, FunctionRef};

use crate::program::{Compiled, Op};
use crate::record::Record;
use crate::trace::{BranchKind, Origin};
use crate::val::{Scalar, Val, Vector};
use crate::{InterpError, Interpreter, Perturbation, ops};

pub(crate) const NO_STEP: u32 = u32::MAX;

/// Pins and forced branches, keyed by function and expression index.
#[derive(Debug, Default)]
pub(crate) struct Overrides {
    pub(crate) pins: HashMap<(FunctionRef, u32), Val>,
    pub(crate) branches: HashMap<(FunctionRef, u32), bool>,
}

impl Overrides {
    fn is_empty(&self) -> bool {
        self.pins.is_empty() && self.branches.is_empty()
    }
}

#[derive(Debug)]
pub(crate) enum State {
    Running,
    /// Stopped at an operation that needs this operand from every lane of the quad.
    Waiting(Val),
    Done,
}

struct Frame<'a> {
    compiled: &'a Compiled,
    ir: &'a naga::Function,
    pc: usize,
    /// The next expression to evaluate inside the current emit, when resuming.
    cursor: Option<usize>,
    values: Vec<Option<Val>>,
    steps: Vec<u32>,
    locals: Vec<Val>,
    /// Steps that last stored into each local, by path.
    local_stores: Vec<Vec<(Vec<u32>, u32)>>,
    args: Vec<Val>,
    arg_steps: Vec<u32>,
    /// For each branch op, the step of the condition it last decided on.
    branch_steps: Vec<u32>,
    call_guard: Option<u32>,
    /// Where the caller wants the return value.
    result: Option<Handle<Expression>>,
    /// This call's number within the lane.
    call: u32,
}

/// Extra facts about a computed value, for the trace.
#[derive(Default)]
struct Extra {
    origin: Option<Origin>,
    note: Option<String>,
    inputs: Option<Vec<u32>>,
}

enum Computed {
    Value(Val, Extra),
    /// Needs the operand from every lane of the quad.
    Wait(Val),
}

pub(crate) struct Lane<'a> {
    interp: &'a Interpreter,
    pub(crate) pixel: [u32; 2],
    /// This lane's place in its quad: bit 0 is x and bit 1 is y.
    pub(crate) quad: usize,
    frames: Vec<Frame<'a>>,
    privates: HashMap<Handle<naga::GlobalVariable>, Val>,
    private_stores: HashMap<Handle<naga::GlobalVariable>, Vec<(Vec<u32>, u32)>>,
    writes: HashMap<Handle<naga::GlobalVariable>, Val>,
    overrides: &'a Overrides,
    nudge: Option<&'a Perturbation>,
    pub(crate) record: Record,
    /// The quad's operands, handed back when the lane resumes.
    resume: Option<[Option<Val>; 4]>,
    executed: u64,
    /// How many times the tests of limited loops ran.
    turns: u32,
    pub(crate) state: State,
    pub(crate) notes: Vec<String>,
    pub(crate) outputs: Vec<crate::inputs::RawOutput>,
    pub(crate) discarded: bool,
    pub(crate) result_step: Option<u32>,
    pub(crate) result_guard: Option<u32>,
    calls: u32,
}

impl<'a> Lane<'a> {
    pub(crate) fn new(
        interp: &'a Interpreter,
        pixel: [u32; 2],
        overrides: &'a Overrides,
        nudge: Option<&'a Perturbation>,
        record: Record,
    ) -> Self {
        Self {
            interp,
            pixel,
            quad: ((pixel[1] & 1) * 2 + (pixel[0] & 1)) as usize,
            frames: Vec::new(),
            privates: HashMap::new(),
            private_stores: HashMap::new(),
            writes: HashMap::new(),
            overrides,
            nudge,
            record,
            resume: None,
            executed: 0,
            turns: 0,
            state: State::Running,
            notes: Vec::new(),
            outputs: Vec::new(),
            discarded: false,
            result_step: None,
            result_guard: None,
            calls: 0,
        }
    }

    /// Pushes the entry point's frame with its inputs.
    pub(crate) fn start(&mut self) -> Result<(), InterpError> {
        let interp = self.interp;
        let (args, origins) =
            crate::inputs::entry_arguments(interp, self.pixel, self.nudge, &mut self.notes)?;
        let compiled = interp.entry_compiled();
        let ir = interp.entry_ir();
        let count = args.len();
        self.push_frame(compiled, ir, args, vec![NO_STEP; count], None, None)?;
        // Inputs become the first steps, so values computed from them link back.
        for (index, origin) in origins.into_iter().enumerate() {
            let Some(handle) = compiled.arguments.get(index).copied().flatten() else {
                continue;
            };
            if self.overridden(handle).is_some() {
                continue;
            }
            let value = self.frames[0].args[index].clone();
            let extra = Extra {
                origin: Some(origin),
                inputs: Some(Vec::new()),
                note: None,
            };
            self.frames[0].arg_steps[index] = self.record_step(handle, &value, extra);
        }
        self.override_arguments();
        self.record_call();
        Ok(())
    }

    /// Hands the quad's operands back to a waiting lane.
    pub(crate) fn resume(&mut self, operands: [Option<Val>; 4]) {
        self.resume = Some(operands);
        self.state = State::Running;
    }

    /// Runs until the lane finishes or waits on its quad.
    pub(crate) fn run(&mut self) -> Result<(), InterpError> {
        while let Some(frame) = self.frames.last() {
            let (compiled, pc, cursor) = (frame.compiled, frame.pc, frame.cursor);
            self.tick()?;
            let Some(op) = compiled.ops.get(pc) else {
                self.finish_call(None)?;
                continue;
            };
            match op {
                Op::Emit { start, end } => {
                    let from = cursor.unwrap_or(*start as usize);
                    for index in from..*end as usize {
                        let handle = compiled.handles[index];
                        if let Some(operand) = self.emit(handle)? {
                            self.top_mut().cursor = Some(index);
                            self.state = State::Waiting(operand);
                            return Ok(());
                        }
                    }
                    let frame = self.top_mut();
                    frame.cursor = None;
                    frame.pc += 1;
                }
                Op::Jump(target) => self.top_mut().pc = *target as usize,
                Op::Branch {
                    condition,
                    otherwise,
                    internal,
                    exit,
                } => {
                    let kind = if *internal {
                        None
                    } else {
                        Some(BranchKind::If)
                    };
                    let mut taken = self.decide(*condition, pc, kind)?;
                    if let Some(leaves) = *exit
                        && self.turn()
                    {
                        taken = leaves;
                    }
                    self.top_mut().pc = if taken { pc + 1 } else { *otherwise as usize };
                }
                Op::BreakIf {
                    condition,
                    start,
                    limited,
                } => {
                    let mut leave = self.decide(*condition, pc, Some(BranchKind::BreakIf))?;
                    if *limited && self.turn() {
                        leave = true;
                    }
                    self.top_mut().pc = if leave { pc + 1 } else { *start as usize };
                }
                Op::Switch { selector, cases } => {
                    let target = self.switch(*selector, cases, pc)?;
                    self.top_mut().pc = target;
                }
                Op::Store { pointer, value } => {
                    self.store(*pointer, *value, pc)?;
                    self.top_mut().pc += 1;
                }
                Op::Call {
                    function,
                    arguments,
                    result,
                } => self.call(*function, arguments, *result, pc)?,
                Op::Return(value) => {
                    if self.frames.len() == 1 {
                        self.result_guard = self.guard_at(pc);
                    }
                    self.finish_call(*value)?
                }
                Op::Kill => {
                    self.result_guard = self.guard_at(pc);
                    self.discarded = true;
                    self.frames.clear();
                }
                Op::Unsupported(what) => {
                    return Err(InterpError::Unsupported(format!(
                        "{what} is not interpreted yet"
                    )));
                }
            }
        }
        self.state = State::Done;
        Ok(())
    }

    /// Counts a turn of a limited loop. Returns whether the loops have used up `LOOP_LIMIT`,
    /// which ends the loop, as the GLSL a WebGL2 app runs for an edit does.
    fn turn(&mut self) -> bool {
        self.turns += 1;
        let over = self.turns > texelscope_ir::LOOP_LIMIT;
        if self.turns == texelscope_ir::LOOP_LIMIT + 1 {
            self.notes.push(format!(
                "The loops stopped after {} turns, as they do when your app runs an edit.",
                texelscope_ir::LOOP_LIMIT
            ));
        }
        over
    }

    fn tick(&mut self) -> Result<(), InterpError> {
        self.executed += 1;
        if self.executed > self.interp.options.max_ops {
            return Err(InterpError::StepLimit(self.interp.options.max_ops));
        }
        Ok(())
    }

    fn top(&self) -> &Frame<'a> {
        self.frames
            .last()
            .expect("a lane with no frame does not run")
    }

    fn top_mut(&mut self) -> &mut Frame<'a> {
        self.frames
            .last_mut()
            .expect("a lane with no frame does not run")
    }

    fn module(&self) -> &'a naga::Module {
        self.interp.module.naga()
    }

    fn push_frame(
        &mut self,
        compiled: &'a Compiled,
        ir: &'a naga::Function,
        args: Vec<Val>,
        arg_steps: Vec<u32>,
        call_guard: Option<u32>,
        result: Option<Handle<Expression>>,
    ) -> Result<(), InterpError> {
        let module = self.module();
        let mut locals = Vec::with_capacity(ir.local_variables.len());
        for (_, var) in ir.local_variables.iter() {
            locals.push(match var.init {
                // Initializers are constant, so they can be evaluated before the frame exists.
                Some(init) => self.constant_in(ir, init)?,
                None => Val::zero(module, var.ty)?,
            });
        }
        let count = compiled.handles.len();
        self.frames.push(Frame {
            compiled,
            ir,
            pc: 0,
            cursor: None,
            values: vec![None; count],
            steps: vec![NO_STEP; count],
            local_stores: vec![Vec::new(); locals.len()],
            locals,
            args,
            arg_steps,
            branch_steps: vec![NO_STEP; compiled.ops.len()],
            call_guard,
            result,
            call: self.calls,
        });
        self.calls += 1;
        Ok(())
    }

    /// Evaluates a constant expression of a function without a frame for it.
    fn constant_in(
        &self,
        ir: &naga::Function,
        handle: Handle<Expression>,
    ) -> Result<Val, InterpError> {
        let module = self.module();
        Ok(match ir.expressions[handle] {
            Expression::Literal(l) => Val::Scalar(Scalar::from_literal(l)?),
            Expression::Constant(c) => self.interp.constant(c)?,
            Expression::ZeroValue(ty) => Val::zero(module, ty)?,
            Expression::Compose { ty, ref components } => {
                let items = components
                    .iter()
                    .map(|&c| self.constant_in(ir, c))
                    .collect::<Result<_, _>>()?;
                ops::compose(module, ty, items)?
            }
            Expression::Splat { size, value } => {
                let value = self.constant_in(ir, value)?.scalar()?;
                Val::Vector(Vector::splat(value, size as u8))
            }
            ref other => {
                return Err(InterpError::Unsupported(format!(
                    "{other:?} as a variable initializer"
                )));
            }
        })
    }

    /// Evaluates one emitted expression. Returns the operand to share when the lane must
    /// wait for its quad.
    fn emit(&mut self, handle: Handle<Expression>) -> Result<Option<Val>, InterpError> {
        self.tick()?;
        if let Some((value, extra)) = self.overridden(handle) {
            self.set(handle, value, extra);
            return Ok(None);
        }
        match self.compute(handle)? {
            Computed::Value(value, extra) => {
                self.set(handle, value, extra);
                Ok(None)
            }
            Computed::Wait(operand) => Ok(Some(operand)),
        }
    }

    /// The value an override gives an expression of the top frame. A forced condition is
    /// pinned like any value, so every use of it sees the same side.
    fn overridden(&self, handle: Handle<Expression>) -> Option<(Val, Extra)> {
        if self.overrides.is_empty() {
            return None;
        }
        let key = (self.top().compiled.function, handle.index() as u32);
        let (value, note) = match self.overrides.pins.get(&key) {
            Some(value) => (value.clone(), "pinned"),
            None => {
                let taken = *self.overrides.branches.get(&key)?;
                (Val::Scalar(Scalar::Bool(taken)), "forced")
            }
        };
        let extra = Extra {
            note: Some(note.to_string()),
            inputs: Some(Vec::new()),
            ..Extra::default()
        };
        Some((value, extra))
    }

    /// Notes the top frame's call with the steps of its arguments.
    fn record_call(&mut self) {
        let frame = self.top();
        let (function, args) = (frame.compiled.function, frame.arg_steps.clone());
        self.record.call(function, &args, NO_STEP);
    }

    /// Gives the top frame's overridden parameters their values.
    fn override_arguments(&mut self) {
        if self.overrides.is_empty() {
            return;
        }
        let compiled = self.top().compiled;
        for (index, &handle) in compiled.arguments.iter().enumerate() {
            let Some(handle) = handle else { continue };
            let Some((value, extra)) = self.overridden(handle) else {
                continue;
            };
            let step = self.record_step(handle, &value, extra);
            let frame = self.top_mut();
            frame.args[index] = value;
            frame.arg_steps[index] = step;
        }
    }

    fn set(&mut self, handle: Handle<Expression>, value: Val, extra: Extra) {
        let step = self.record_step(handle, &value, extra);
        let frame = self.top_mut();
        frame.steps[handle.index()] = step;
        frame.values[handle.index()] = Some(value);
    }

    /// Makes sure an operand has a value, computing constants on first use.
    fn ensure(&mut self, handle: Handle<Expression>) -> Result<(), InterpError> {
        if self.top().values[handle.index()].is_some() {
            return Ok(());
        }
        match self.compute(handle)? {
            Computed::Value(value, _) => {
                self.top_mut().values[handle.index()] = Some(value);
                Ok(())
            }
            Computed::Wait(_) => Err(InterpError::Internal(
                "a quad operation outside of an emit".into(),
            )),
        }
    }

    fn value(&self, handle: Handle<Expression>) -> Result<&Val, InterpError> {
        self.top().values[handle.index()].as_ref().ok_or_else(|| {
            InterpError::Internal(format!(
                "expression {} was used before it was evaluated",
                handle.index()
            ))
        })
    }

    fn owned(&mut self, handle: Handle<Expression>) -> Result<Val, InterpError> {
        self.ensure(handle)?;
        self.value(handle).cloned()
    }

    fn call(
        &mut self,
        function: Handle<naga::Function>,
        arguments: &[Handle<Expression>],
        result: Option<Handle<Expression>>,
        pc: usize,
    ) -> Result<(), InterpError> {
        // The patched source drops a pinned call, so the function doesn't run.
        if let Some(result) = result
            && let Some((value, extra)) = self.overridden(result)
        {
            self.set(result, value, extra);
            self.top_mut().pc += 1;
            return Ok(());
        }
        let args = arguments
            .iter()
            .map(|&a| self.owned(a))
            .collect::<Result<Vec<_>, _>>()?;
        let arg_steps = arguments
            .iter()
            .map(|&a| self.step_of(a).unwrap_or(NO_STEP))
            .collect();
        let guard = self.guard_at(pc);
        self.top_mut().pc += 1;
        let compiled = self.interp.function_compiled(function);
        let ir = &self.module().functions[function];
        self.push_frame(compiled, ir, args, arg_steps, guard, result)?;
        self.override_arguments();
        self.record_call();
        Ok(())
    }

    fn finish_call(&mut self, value: Option<Handle<Expression>>) -> Result<(), InterpError> {
        let returned = value.map(|h| self.owned(h)).transpose()?;
        let step = value.and_then(|h| self.step_of(h));
        let frame = self.frames.pop().expect("returning from a frame");
        if self.frames.is_empty() {
            self.result_step = step;
            if let Some(value) = returned {
                self.outputs = crate::inputs::entry_outputs(self.interp, value)?;
            }
            return Ok(());
        }
        if let (Some(result), Some(value)) = (frame.result, returned) {
            let extra = Extra {
                inputs: Some(step.into_iter().collect()),
                ..Extra::default()
            };
            self.set(result, value, extra);
        }
        Ok(())
    }

    fn step_of(&self, handle: Handle<Expression>) -> Option<u32> {
        let frame = self.top();
        let step = frame.steps[handle.index()];
        if step != NO_STEP {
            return Some(step);
        }
        match frame.ir.expressions[handle] {
            Expression::FunctionArgument(i) => frame
                .arg_steps
                .get(i as usize)
                .copied()
                .filter(|&s| s != NO_STEP),
            _ => None,
        }
    }

    /// The condition step that the op at `pc` of the top frame runs under.
    fn guard_at(&self, pc: usize) -> Option<u32> {
        let frame = self.top();
        match frame.compiled.guards.get(pc).copied().flatten() {
            Some(branch) => Some(frame.branch_steps[branch as usize]).filter(|&s| s != NO_STEP),
            None => frame.call_guard,
        }
    }

    fn record_step(&mut self, handle: Handle<Expression>, value: &Val, extra: Extra) -> u32 {
        if !self.record.is_on() {
            return NO_STEP;
        }
        let module = self.module();
        let frame = self.top();
        let compiled = frame.compiled;
        let index = handle.index();
        let inputs = match extra.inputs {
            Some(inputs) => inputs,
            None if self.record.wants_inputs() => {
                let mut inputs = Vec::new();
                crate::record::for_each_operand(&frame.ir.expressions[handle], |h| {
                    if let Some(step) = self.step_of(h) {
                        inputs.push(step);
                    }
                });
                inputs
            }
            None => Vec::new(),
        };
        let guard = self.guard_at(frame.pc);
        let depth = self.frames.len() as u32 - 1;
        let call = frame.call;
        self.record.step(
            module,
            crate::record::StepFacts {
                expr: ExprRef {
                    function: compiled.function,
                    expr: index as u32,
                },
                span: compiled.spans[index],
                name: compiled.names[index].clone(),
                inputs,
                guard,
                depth,
                call,
                origin: extra.origin,
                note: extra.note,
            },
            value,
        )
    }
}
