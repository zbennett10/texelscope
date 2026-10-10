//! The reference interpreter. It runs a capture's fragment shader on the CPU one pixel at a
//! time and records every value it computes, so a pixel can be traced, explained and
//! compared with the GPU.

mod image;
mod inputs;
mod lane;
mod layout;
mod math;
mod ops;
mod program;
mod record;
mod region;
mod resources;
mod trace;
mod val;

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use texelscope_capture::{Capture, Rect};
use texelscope_ir::naga::{self, Handle};
use texelscope_ir::{ExprRef, FunctionRef, Override, ShaderModule, Stage, Value};

pub use region::{BranchLayer, Layer, Region};
pub use resources::CaptureData;
pub use trace::{Branch, BranchKind, Origin, Output, Step, Store, Trace};

use crate::lane::{Lane, Overrides, State};
use crate::program::Compiled;
use crate::record::{Record, TraceBuilder};
use crate::resources::Global;
use crate::val::Val;

/// Why a pixel could not be traced.
#[derive(Debug, thiserror::Error)]
pub enum InterpError {
    #[error("the shader has no entry point named {0:?}")]
    NoEntryPoint(String),
    #[error("entry point {0:?} is not a fragment shader")]
    NotFragment(String),
    #[error("pixel ({x}, {y}) is outside the {width}x{height} target")]
    PixelOutOfBounds {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    BadData(String),
    #[error("the invocation ran more than {0} operations, so it may never end")]
    StepLimit(u64),
    #[error("@group({group}) @binding({binding}): {reason}")]
    Binding {
        group: u32,
        binding: u32,
        reason: String,
    },
    #[error("an override does not apply: {0}")]
    Override(String),
    #[error("interpreter bug: {0}")]
    Internal(String),
}

/// Limits for one run.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Operations one invocation may run before it is stopped.
    pub max_ops: u64,
    /// Steps a trace records before it stops recording.
    pub trace_limit: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_ops: 20_000_000,
            trace_limit: 250_000,
        }
    }
}

/// A small change to one input, used to measure how much the output depends on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Perturbation {
    /// Adds `delta` to the f32 a buffer holds at `indices`.
    Buffer {
        group: u32,
        binding: u32,
        indices: Vec<u32>,
        delta: f32,
    },
    /// Adds `delta` to one component of one texel.
    Texel {
        group: u32,
        binding: u32,
        level: u32,
        texel: [i32; 3],
        component: u32,
        delta: f32,
    },
    /// Adds `delta` to one component of a fragment input, named as in [`Origin::Input`].
    Input {
        name: String,
        component: u32,
        delta: f32,
    },
}

/// What one pixel wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shaded {
    pub outputs: Vec<Output>,
    pub discarded: bool,
}

impl Shaded {
    /// Location 0 as four floats, with missing channels as 0 and alpha as 1.
    pub fn color(&self) -> Option<[f32; 4]> {
        let value = self.outputs.iter().find(|o| o.location == Some(0))?;
        Some(color_of(&value.value))
    }
}

/// Four channels of a color output, padded the way a render target stores them.
pub fn color_of(value: &Value) -> [f32; 4] {
    let mut out = [0.0, 0.0, 0.0, 1.0];
    let lanes: Vec<f32> = match value {
        Value::Vector(items) => items.iter().filter_map(scalar_f32).collect(),
        other => scalar_f32(other).into_iter().collect(),
    };
    for (o, v) in out.iter_mut().zip(lanes) {
        *o = v;
    }
    out
}

fn scalar_f32(value: &Value) -> Option<f32> {
    Some(match *value {
        Value::F32(v) => v,
        Value::I32(v) => v as f32,
        Value::U32(v) => v as f32,
        Value::Bool(v) => f32::from(u8::from(v)),
        _ => return None,
    })
}

/// Runs one capture's fragment entry point.
pub struct Interpreter {
    module: Arc<ShaderModule>,
    data: Arc<CaptureData>,
    entry: usize,
    entry_compiled: Compiled,
    functions: Vec<Compiled>,
    globals: Vec<Global>,
    constants: Vec<Val>,
    uses_quads: bool,
    pub options: Options,
}

impl Interpreter {
    /// Matches the capture's bindings to the shader and prepares its entry point.
    pub fn new(module: Arc<ShaderModule>, data: Arc<CaptureData>) -> Result<Self, InterpError> {
        let name = data.capture().shader.entry_point.clone();
        let info = module
            .entry_point(&name)
            .ok_or_else(|| InterpError::NoEntryPoint(name.clone()))?;
        if info.stage != Stage::Fragment {
            return Err(InterpError::NotFragment(name));
        }
        let FunctionRef::EntryPoint(entry) = info.function else {
            return Err(InterpError::Internal(
                "an entry point that is a helper".into(),
            ));
        };
        let naga = module.naga();
        let entry_ir = &naga.entry_points[entry as usize].function;
        let entry_compiled = program::compile(&module, info.function, &name, entry_ir);
        let functions: Vec<Compiled> = naga
            .functions
            .iter()
            .map(|(h, f)| {
                program::compile(
                    &module,
                    FunctionRef::Function(h.index() as u32),
                    f.name.as_deref().unwrap_or(""),
                    f,
                )
            })
            .collect();
        let uses_quads = entry_compiled.uses_quads || functions.iter().any(|f| f.uses_quads);

        let mut constants = Vec::with_capacity(naga.constants.len());
        for (_, constant) in naga.constants.iter() {
            constants.push(eval_global(naga, constant.init, &constants)?);
        }
        let globals = resources::resolve(naga, &data, &|init| eval_global(naga, init, &constants))?;
        Ok(Self {
            entry: entry as usize,
            entry_compiled,
            functions,
            globals,
            constants,
            uses_quads,
            module,
            data,
            options: Options::default(),
        })
    }

    pub fn module(&self) -> &ShaderModule {
        &self.module
    }

    pub fn capture(&self) -> &Capture {
        self.data.capture()
    }

    pub fn data(&self) -> &Arc<CaptureData> {
        &self.data
    }

    /// Whether the shader takes derivatives, so each pixel runs with its whole quad.
    pub fn uses_quads(&self) -> bool {
        self.uses_quads
    }

    /// Runs one pixel and records every value it computes.
    pub fn trace_pixel(
        &self,
        pixel: [u32; 2],
        overrides: &[Override],
    ) -> Result<Trace, InterpError> {
        self.check_pixel(pixel)?;
        let overrides = self.overrides(overrides)?;
        let record = Record::Trace(Box::new(TraceBuilder::new(self.options.trace_limit)));
        let lane = self.run_pixel(pixel, &overrides, None, record)?;
        let Record::Trace(builder) = lane.record else {
            unreachable!("the traced lane records a trace");
        };
        let module = self.module.naga();
        Ok(Trace {
            entry_point: self.capture().shader.entry_point.clone(),
            pixel,
            steps: builder.steps,
            branches: builder.branches,
            stores: builder.stores,
            outputs: outputs(module, &lane.outputs, lane.discarded),
            result: lane.result_step,
            result_guard: lane.result_guard,
            discarded: lane.discarded,
            truncated: builder.truncated,
            notes: lane.notes,
        })
    }

    /// Runs one pixel without recording, optionally with one input nudged.
    pub fn shade_pixel(
        &self,
        pixel: [u32; 2],
        overrides: &[Override],
        perturbation: Option<&Perturbation>,
    ) -> Result<Shaded, InterpError> {
        self.check_pixel(pixel)?;
        let overrides = self.overrides(overrides)?;
        let lane = self.run_pixel(pixel, &overrides, perturbation, Record::Off)?;
        Ok(Shaded {
            outputs: outputs(self.module.naga(), &lane.outputs, lane.discarded),
            discarded: lane.discarded,
        })
    }

    /// Runs every pixel of a rectangle and keeps the last value of each named value, each
    /// decision and the color. Overrides apply inside `scope`, or everywhere without one.
    pub fn inspect_region(
        &self,
        rect: Rect,
        overrides: &[Override],
        scope: Option<Rect>,
    ) -> Result<Region, InterpError> {
        region::inspect(self, rect, overrides, scope, true)
    }

    /// Shades every pixel of the target without recording and returns RGBA floats, rows
    /// from the top, NaN where an invocation discarded.
    pub fn render(&self, overrides: &[Override]) -> Result<Vec<f32>, InterpError> {
        let target = self.capture().target;
        let rect = Rect {
            x: 0,
            y: 0,
            width: target.width,
            height: target.height,
        };
        Ok(region::inspect(self, rect, overrides, None, false)?.colors)
    }

    /// The bytes a buffer binding holds, laid out as the shader declares it, for replaying
    /// the capture on a GPU. Member values are packed, raw data comes back as resolved.
    pub fn buffer_bytes(&self, group: u32, binding: u32) -> Result<Vec<u8>, InterpError> {
        let naga = self.module.naga();
        for ((_, var), global) in naga.global_variables.iter().zip(&self.globals) {
            let Some(rb) = &var.binding else { continue };
            if (rb.group, rb.binding) != (group, binding) {
                continue;
            }
            let Global::Buffer { value, .. } = global else {
                return Err(self.global_error_by_binding(group, binding, global));
            };
            let mut bytes = Vec::new();
            layout::write(naga, var.ty, value, &mut bytes, 0)?;
            let mut layouter = naga::proc::Layouter::default();
            layouter
                .update(naga.to_ctx())
                .map_err(|e| InterpError::Internal(e.to_string()))?;
            let size = layouter[var.ty].size as usize;
            bytes.resize(bytes.len().max(size).next_multiple_of(16), 0);
            return Ok(bytes);
        }
        Err(InterpError::Binding {
            group,
            binding,
            reason: "the shader declares no buffer there".into(),
        })
    }

    fn global_error_by_binding(&self, group: u32, binding: u32, global: &Global) -> InterpError {
        match global {
            Global::Missing { reason, .. } => InterpError::Binding {
                group,
                binding,
                reason: reason.clone(),
            },
            _ => InterpError::Binding {
                group,
                binding,
                reason: "this binding is not a buffer".into(),
            },
        }
    }

    fn check_pixel(&self, [x, y]: [u32; 2]) -> Result<(), InterpError> {
        let target = self.capture().target;
        if x >= target.width || y >= target.height {
            return Err(InterpError::PixelOutOfBounds {
                x,
                y,
                width: target.width,
                height: target.height,
            });
        }
        Ok(())
    }

    /// Runs the lane for one pixel, with its quad when the shader needs one.
    fn run_pixel<'a>(
        &'a self,
        pixel: [u32; 2],
        overrides: &'a Overrides,
        perturbation: Option<&'a Perturbation>,
        record: Record,
    ) -> Result<Lane<'a>, InterpError> {
        if !self.uses_quads {
            let mut lanes = [Lane::new(self, pixel, overrides, perturbation, record)];
            self.run_quad(&mut lanes)?;
            let [lane] = lanes;
            return Ok(lane);
        }
        let mut record = Some(record);
        let [x0, y0] = [pixel[0] & !1, pixel[1] & !1];
        let mut lanes: Vec<Lane<'a>> = (0..4u32)
            .map(|i| {
                let at = [x0 + (i & 1), y0 + (i >> 1)];
                let record = if at == pixel {
                    record.take().unwrap_or(Record::Off)
                } else {
                    Record::Off
                };
                Lane::new(self, at, overrides, perturbation, record)
            })
            .collect();
        self.run_quad(&mut lanes)?;
        Ok(lanes.swap_remove(((pixel[1] & 1) * 2 + (pixel[0] & 1)) as usize))
    }

    /// Runs lanes of one quad together, meeting at every derivative.
    fn run_quad(&self, lanes: &mut [Lane<'_>]) -> Result<(), InterpError> {
        for lane in lanes.iter_mut() {
            lane.start()?;
        }
        loop {
            for lane in lanes.iter_mut() {
                if matches!(lane.state, State::Running) {
                    lane.run()?;
                }
            }
            let mut operands: [Option<Val>; 4] = Default::default();
            let mut waiting = false;
            for lane in lanes.iter() {
                if let State::Waiting(operand) = &lane.state {
                    operands[lane.quad] = Some(operand.clone());
                    waiting = true;
                }
            }
            if !waiting {
                return Ok(());
            }
            for lane in lanes.iter_mut() {
                if matches!(lane.state, State::Waiting(_)) {
                    lane.resume(operands.clone());
                }
            }
        }
    }

    /// Converts overrides to runtime values keyed for quick lookup.
    fn overrides(&self, list: &[Override]) -> Result<Overrides, InterpError> {
        let naga = self.module.naga();
        let mut overrides = Overrides::default();
        for o in list {
            match o {
                Override::Pin { expr, value } => {
                    let inner = self.expr_type(*expr)?;
                    let value = Val::from_value(naga, value, inner)?;
                    overrides.pins.insert((expr.function, expr.expr), value);
                }
                Override::ForceBranch { condition, taken } => {
                    self.expr_type(*condition)?;
                    overrides
                        .branches
                        .insert((condition.function, condition.expr), *taken);
                }
                Override::Replace { .. } => {
                    return Err(InterpError::Override(
                        "replacements change the source, so apply them before interpreting".into(),
                    ));
                }
            }
        }
        Ok(overrides)
    }

    fn expr_type(&self, expr: ExprRef) -> Result<&naga::TypeInner, InterpError> {
        self.module.type_of(expr).ok_or_else(|| {
            InterpError::Override(format!(
                "no expression {} in {:?}",
                expr.expr, expr.function
            ))
        })
    }

    /// Function names by reference, for labeling layers.
    pub(crate) fn function_names(&self) -> HashMap<FunctionRef, String> {
        std::iter::once(&self.entry_compiled)
            .chain(&self.functions)
            .map(|c| (c.function, c.name.clone()))
            .collect()
    }

    pub(crate) fn entry_ir(&self) -> &naga::Function {
        &self.module.naga().entry_points[self.entry].function
    }

    pub(crate) fn entry_compiled(&self) -> &Compiled {
        &self.entry_compiled
    }

    pub(crate) fn function_compiled(&self, function: Handle<naga::Function>) -> &Compiled {
        &self.functions[function.index()]
    }

    pub(crate) fn constant(&self, constant: Handle<naga::Constant>) -> Result<Val, InterpError> {
        self.constants
            .get(constant.index())
            .cloned()
            .ok_or_else(|| InterpError::Internal("an unknown constant".into()))
    }

    pub(crate) fn pipeline_constant(
        &self,
        handle: Handle<naga::Override>,
    ) -> Result<Val, InterpError> {
        let naga = self.module.naga();
        let o = &naga.overrides[handle];
        match o.init {
            Some(init) => eval_global(naga, init, &self.constants),
            None => Err(InterpError::Unsupported(format!(
                "override {} has no default, and captures do not carry override values yet",
                o.name.as_deref().unwrap_or("?")
            ))),
        }
    }

    /// A label such as `params.palette[2]` for a path into a global.
    pub(crate) fn global_path_label(
        &self,
        global: Handle<naga::GlobalVariable>,
        path: &[u32],
    ) -> String {
        let naga = self.module.naga();
        let var = &naga.global_variables[global];
        lane::path_label(naga, var.name.as_deref().unwrap_or("_"), var.ty, path)
    }

    pub(crate) fn global_error(
        &self,
        global: Handle<naga::GlobalVariable>,
        resolved: &Global,
    ) -> InterpError {
        let naga = self.module.naga();
        let name = naga.global_variables[global].name.as_deref().unwrap_or("?");
        match resolved {
            Global::Missing {
                group,
                binding,
                reason,
            } => InterpError::Binding {
                group: *group,
                binding: *binding,
                reason: reason.clone(),
            },
            _ => InterpError::Unsupported(format!("{name} cannot be used this way")),
        }
    }
}

/// Evaluates a constant expression from the module's global arena.
fn eval_global(
    module: &naga::Module,
    handle: Handle<naga::Expression>,
    constants: &[Val],
) -> Result<Val, InterpError> {
    use naga::Expression as E;
    let eval = |h| eval_global(module, h, constants);
    Ok(match module.global_expressions[handle] {
        E::Literal(l) => Val::Scalar(val::Scalar::from_literal(l)?),
        E::Constant(c) => constants
            .get(c.index())
            .cloned()
            .ok_or_else(|| InterpError::Internal("a constant used before it is defined".into()))?,
        E::ZeroValue(ty) => Val::zero(module, ty)?,
        E::Compose { ty, ref components } => {
            let items = components
                .iter()
                .map(|&c| eval(c))
                .collect::<Result<_, _>>()?;
            ops::compose(module, ty, items)?
        }
        E::Splat { size, value } => {
            Val::Vector(val::Vector::splat(eval(value)?.scalar()?, size as u8))
        }
        E::Unary { op, expr } => ops::unary(op, &eval(expr)?)?,
        E::Binary { op, left, right } => ops::binary(op, &eval(left)?, &eval(right)?)?,
        E::As {
            expr,
            kind,
            convert,
        } => ops::convert(&eval(expr)?, kind, convert)?,
        E::Override(o) => match module.overrides[o].init {
            Some(init) => eval(init)?,
            None => {
                return Err(InterpError::Unsupported(
                    "an override without a default".into(),
                ));
            }
        },
        ref other => {
            return Err(InterpError::Unsupported(format!(
                "{other:?} in a constant expression"
            )));
        }
    })
}

fn outputs(module: &naga::Module, raw: &[inputs::RawOutput], discarded: bool) -> Vec<Output> {
    if discarded {
        return Vec::new();
    }
    raw.iter()
        .filter_map(|(location, builtin, value)| {
            Some(Output {
                location: *location,
                builtin: builtin.clone(),
                value: value.to_value(module)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
