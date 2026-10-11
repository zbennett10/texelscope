//! What a lane writes down as it runs: a full trace for one pixel, the last value of each
//! named value for a region, or nothing.

use std::collections::HashMap;
use std::sync::Arc;

use texelscope_ir::naga::{self, Expression, Handle, SampleLevel};
use texelscope_ir::{ExprRef, FunctionRef, SourceSpan};

use crate::lane::NO_STEP;
use crate::trace::{Branch, BranchKind, Origin, Step, Store};
use crate::val::{Scalar, Val};

pub(crate) enum Record {
    Off,
    Trace(Box<TraceBuilder>),
    Layers(Box<LayerSink>),
}

pub(crate) struct StepFacts {
    pub(crate) expr: ExprRef,
    pub(crate) span: Option<SourceSpan>,
    pub(crate) name: Option<Arc<str>>,
    pub(crate) inputs: Vec<u32>,
    pub(crate) guard: Option<u32>,
    pub(crate) depth: u32,
    pub(crate) call: u32,
    pub(crate) origin: Option<Origin>,
    pub(crate) note: Option<String>,
}

pub(crate) struct BranchFacts {
    pub(crate) condition: ExprRef,
    pub(crate) kind: BranchKind,
    pub(crate) step: Option<u32>,
    pub(crate) taken: bool,
    pub(crate) margin: Option<f64>,
    pub(crate) forced: bool,
    pub(crate) span: Option<SourceSpan>,
    pub(crate) depth: u32,
}

pub(crate) struct StoreTarget {
    pub(crate) function: FunctionRef,
    pub(crate) label: String,
    pub(crate) span: Option<SourceSpan>,
    pub(crate) depth: u32,
    pub(crate) call: u32,
    /// The condition step the assignment ran under, if any.
    pub(crate) guard: Option<u32>,
}

impl Record {
    pub(crate) fn is_on(&self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether steps need the indices of the steps they were computed from.
    pub(crate) fn wants_inputs(&self) -> bool {
        matches!(self, Self::Trace(_))
    }

    /// Records a computed value and returns its step index.
    pub(crate) fn step(&mut self, module: &naga::Module, facts: StepFacts, value: &Val) -> u32 {
        match self {
            Self::Off => NO_STEP,
            Self::Trace(trace) => {
                if trace.steps.len() >= trace.limit {
                    trace.truncated = true;
                    return NO_STEP;
                }
                let Some(value) = value.to_value(module) else {
                    return NO_STEP;
                };
                trace.steps.push(Step {
                    expr: facts.expr,
                    span: facts.span,
                    value,
                    inputs: facts.inputs,
                    guard: facts.guard,
                    depth: facts.depth,
                    call: facts.call,
                    name: facts.name.map(|n| n.to_string()),
                    origin: facts.origin,
                    note: facts.note,
                });
                (trace.steps.len() - 1) as u32
            }
            Self::Layers(sink) => {
                if let Some(name) = &facts.name {
                    sink.sample(
                        module,
                        LayerKey::Expr(facts.expr),
                        name,
                        facts.span,
                        facts.expr.function,
                        value,
                    );
                }
                NO_STEP
            }
        }
    }

    pub(crate) fn branch(&mut self, facts: BranchFacts) {
        match self {
            Self::Off => {}
            Self::Trace(trace) => {
                let at = trace.steps.len() as u32;
                trace.branches.push(Branch {
                    at,
                    kind: facts.kind,
                    condition: facts.condition,
                    step: facts.step,
                    taken: facts.taken,
                    margin: facts.margin,
                    forced: facts.forced,
                    span: facts.span,
                    depth: facts.depth,
                });
            }
            Self::Layers(sink) => {
                if facts.kind == BranchKind::If || facts.kind == BranchKind::Select {
                    sink.branch(facts.condition, facts.taken, facts.span);
                }
            }
        }
    }

    pub(crate) fn store(
        &mut self,
        module: &naga::Module,
        target: StoreTarget,
        value: &Val,
        step: Option<u32>,
    ) {
        match self {
            Self::Off => {}
            Self::Trace(trace) => {
                let Some(reported) = value.to_value(module) else {
                    return;
                };
                let at = trace.steps.len() as u32;
                trace.stores.push(Store {
                    at,
                    target: target.label,
                    value: reported,
                    step,
                    span: target.span,
                    depth: target.depth,
                    call: target.call,
                    guard: target.guard,
                });
            }
            Self::Layers(sink) => {
                let key = LayerKey::Var(target.function, target.label.clone());
                sink.sample(
                    module,
                    key,
                    &target.label,
                    target.span,
                    target.function,
                    value,
                );
            }
        }
    }
}

impl Record {
    /// Notes a call and the steps of its arguments, in call number order.
    pub(crate) fn call(&mut self, function: FunctionRef, args: &[u32], none: u32) {
        if let Self::Trace(trace) = self {
            let args = args.iter().map(|&a| (a != none).then_some(a)).collect();
            trace.calls.push(crate::trace::Call { function, args });
        }
    }
}

/// Builds a [`crate::Trace`].
pub(crate) struct TraceBuilder {
    pub(crate) steps: Vec<Step>,
    pub(crate) branches: Vec<Branch>,
    pub(crate) stores: Vec<Store>,
    pub(crate) calls: Vec<crate::trace::Call>,
    pub(crate) limit: usize,
    pub(crate) truncated: bool,
}

impl TraceBuilder {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            steps: Vec::new(),
            branches: Vec::new(),
            stores: Vec::new(),
            calls: Vec::new(),
            limit,
            truncated: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum LayerKey {
    /// A named `let`, or one member of it when it is a struct.
    Expr(ExprRef),
    Member(ExprRef, u32),
    /// A `var` or part of one, by its label.
    Var(FunctionRef, String),
}

/// The last value one pixel computed for a named value.
#[derive(Debug, Clone)]
pub(crate) struct LayerSample {
    pub(crate) name: String,
    pub(crate) function: FunctionRef,
    pub(crate) span: Option<SourceSpan>,
    pub(crate) lanes: [f32; 4],
    pub(crate) components: u8,
    pub(crate) count: u32,
    /// When the value shown, the last one, was computed, for listing layers in that order.
    pub(crate) order: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BranchSample {
    pub(crate) taken: bool,
    pub(crate) mixed: bool,
    pub(crate) span: Option<SourceSpan>,
    pub(crate) order: u32,
}

/// The named values and decisions of one pixel.
#[derive(Debug, Default)]
pub(crate) struct LayerSink {
    pub(crate) samples: HashMap<LayerKey, LayerSample>,
    pub(crate) branches: HashMap<ExprRef, BranchSample>,
    counter: u32,
}

impl LayerSink {
    fn sample(
        &mut self,
        module: &naga::Module,
        key: LayerKey,
        name: &str,
        span: Option<SourceSpan>,
        function: FunctionRef,
        value: &Val,
    ) {
        match value {
            Val::Scalar(_) | Val::Vector(_) => {
                self.put(key, name.to_string(), span, function, value)
            }
            // Struct members become layers of their own when they are scalars or vectors.
            Val::Composite(c) => {
                let crate::val::CompositeKind::Struct(ty) = c.kind else {
                    return;
                };
                let naga::TypeInner::Struct { members, .. } = &module.types[ty].inner else {
                    return;
                };
                for (i, (item, member)) in c.items.iter().zip(members).enumerate() {
                    let label = format!("{name}.{}", member.name.as_deref().unwrap_or("_"));
                    let key = match &key {
                        LayerKey::Var(f, _) => LayerKey::Var(*f, label.clone()),
                        LayerKey::Expr(expr) | LayerKey::Member(expr, _) => {
                            LayerKey::Member(*expr, i as u32)
                        }
                    };
                    self.put(key, label, span, function, item);
                }
            }
            _ => {}
        }
    }

    fn put(
        &mut self,
        key: LayerKey,
        name: String,
        span: Option<SourceSpan>,
        function: FunctionRef,
        value: &Val,
    ) {
        let Some(lanes) = value.lanes() else {
            return;
        };
        let mut packed = [0.0f32; 4];
        for (out, lane) in packed.iter_mut().zip(lanes) {
            *out = match lane {
                Scalar::Bool(b) => f32::from(u8::from(*b)),
                other => other.as_f32(),
            };
        }
        let order = self.counter;
        self.counter += 1;
        let entry = self.samples.entry(key).or_insert(LayerSample {
            name,
            function,
            span,
            lanes: packed,
            components: lanes.len() as u8,
            count: 0,
            order,
        });
        entry.lanes = packed;
        entry.count += 1;
        // A later assignment or loop pass replaces the value, so the layer sits where it did.
        entry.order = order;
        entry.span = span;
    }

    fn branch(&mut self, condition: ExprRef, taken: bool, span: Option<SourceSpan>) {
        let order = self.counter;
        self.counter += 1;
        self.branches
            .entry(condition)
            .and_modify(|b| {
                b.mixed |= b.taken != taken;
                b.taken = taken;
            })
            .or_insert(BranchSample {
                taken,
                mixed: false,
                span,
                order,
            });
    }
}

/// Calls `f` with each expression `expr` reads, leaving out textures and samplers.
pub(crate) fn for_each_operand(expr: &Expression, mut f: impl FnMut(Handle<Expression>)) {
    let mut opt = |h: Option<Handle<Expression>>| {
        if let Some(h) = h {
            f(h);
        }
    };
    match *expr {
        Expression::Compose { ref components, .. } => components.iter().for_each(|&c| opt(Some(c))),
        Expression::Access { base, index } => {
            opt(Some(base));
            opt(Some(index));
        }
        Expression::AccessIndex { base, .. } => opt(Some(base)),
        Expression::Splat { value, .. } => opt(Some(value)),
        Expression::Swizzle { vector, .. } => opt(Some(vector)),
        Expression::Load { pointer } => opt(Some(pointer)),
        Expression::ImageSample {
            coordinate,
            array_index,
            offset,
            level,
            depth_ref,
            ..
        } => {
            opt(Some(coordinate));
            opt(array_index);
            opt(offset);
            opt(depth_ref);
            match level {
                SampleLevel::Exact(h) | SampleLevel::Bias(h) => opt(Some(h)),
                SampleLevel::Gradient { x, y } => {
                    opt(Some(x));
                    opt(Some(y));
                }
                SampleLevel::Auto | SampleLevel::Zero => {}
            }
        }
        Expression::ImageLoad {
            coordinate,
            array_index,
            sample,
            level,
            ..
        } => {
            opt(Some(coordinate));
            opt(array_index);
            opt(sample);
            opt(level);
        }
        Expression::ImageQuery {
            query: naga::ImageQuery::Size { level },
            ..
        } => opt(level),
        Expression::Unary { expr, .. } => opt(Some(expr)),
        Expression::Binary { left, right, .. } => {
            opt(Some(left));
            opt(Some(right));
        }
        Expression::Select {
            condition,
            accept,
            reject,
        } => {
            opt(Some(condition));
            opt(Some(accept));
            opt(Some(reject));
        }
        Expression::Derivative { expr, .. } => opt(Some(expr)),
        Expression::Relational { argument, .. } => opt(Some(argument)),
        Expression::Math {
            arg,
            arg1,
            arg2,
            arg3,
            ..
        } => {
            opt(Some(arg));
            opt(arg1);
            opt(arg2);
            opt(arg3);
        }
        Expression::As { expr, .. } => opt(Some(expr)),
        Expression::ArrayLength(e) => opt(Some(e)),
        _ => {}
    }
}
