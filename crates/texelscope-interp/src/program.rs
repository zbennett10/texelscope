//! Functions flattened into straight-line ops with jumps. A lane is then just a stack of
//! frames and program counters, so it can stop at any expression and resume there, which
//! is how the four lanes of a quad meet at each derivative.

use std::collections::HashMap;
use std::sync::Arc;

use texelscope_ir::naga::{self, Block, Expression, Handle, Span, Statement, SwitchValue};
use texelscope_ir::{FunctionRef, ShaderModule, SourceSpan};

#[derive(Debug)]
pub(crate) enum Op {
    /// Evaluates the expressions with these arena indices, in order.
    Emit {
        start: u32,
        end: u32,
    },
    Jump(u32),
    /// Falls through when the condition holds and jumps to `otherwise` when it does not.
    /// `internal` marks the branches naga adds to short-circuit `&&` and `||`.
    Branch {
        condition: Handle<Expression>,
        otherwise: u32,
        internal: bool,
    },
    /// Ends one loop iteration. Leaves the loop when the condition holds.
    BreakIf {
        condition: Handle<Expression>,
        start: u32,
    },
    Switch {
        selector: Handle<Expression>,
        cases: Vec<(SwitchValue, u32)>,
    },
    Store {
        pointer: Handle<Expression>,
        value: Handle<Expression>,
    },
    Call {
        function: Handle<naga::Function>,
        arguments: Vec<Handle<Expression>>,
        result: Option<Handle<Expression>>,
    },
    Return(Option<Handle<Expression>>),
    Kill,
    Unsupported(&'static str),
}

/// One function, flattened.
#[derive(Debug)]
pub(crate) struct Compiled {
    pub(crate) function: FunctionRef,
    pub(crate) name: String,
    pub(crate) ops: Vec<Op>,
    /// For each op, the innermost branch or switch op it runs under.
    pub(crate) guards: Vec<Option<u32>>,
    pub(crate) op_spans: Vec<Option<SourceSpan>>,
    /// Expression handles by arena index.
    pub(crate) handles: Vec<Handle<Expression>>,
    pub(crate) spans: Vec<Option<SourceSpan>>,
    /// The `let` name of each expression that has one.
    pub(crate) names: Vec<Option<Arc<str>>>,
    /// Whether the function takes derivatives.
    pub(crate) uses_quads: bool,
    /// The unnamed locals naga uses to short-circuit `&&` and `||`, with what they hold.
    pub(crate) logical: HashMap<Handle<naga::LocalVariable>, Logical>,
}

/// A short-circuited `lhs && rhs` or `lhs || rhs` held in a local.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Logical {
    pub(crate) or: bool,
    pub(crate) lhs: Handle<Expression>,
    pub(crate) rhs: Handle<Expression>,
}

pub(crate) fn compile(
    module: &ShaderModule,
    function: FunctionRef,
    name: &str,
    f: &naga::Function,
) -> Compiled {
    let mut builder = Builder {
        function: Some(f),
        ..Builder::default()
    };
    builder.block(&f.body, None);

    let handles: Vec<Handle<Expression>> = f.expressions.iter().map(|(h, _)| h).collect();
    let spans = handles
        .iter()
        .map(|&h| source_span(f.expressions.get_span(h)).map(|s| module.widen(s)))
        .collect();
    let mut names: Vec<Option<Arc<str>>> = vec![None; handles.len()];
    for (handle, name) in f.named_expressions.iter() {
        names[handle.index()] = Some(Arc::from(name.as_str()));
    }
    let uses_quads = f.expressions.iter().any(|(_, e)| match e {
        Expression::Derivative { .. } => true,
        Expression::ImageSample { level, .. } => {
            matches!(level, naga::SampleLevel::Auto | naga::SampleLevel::Bias(_))
        }
        _ => false,
    });

    Compiled {
        function,
        name: name.to_string(),
        ops: builder.ops,
        guards: builder.guards,
        op_spans: builder.spans,
        handles,
        spans,
        names,
        uses_quads,
        logical: builder.logical,
    }
}

pub(crate) fn source_span(span: Span) -> Option<SourceSpan> {
    span.to_range().map(|r| SourceSpan {
        start: r.start as u32,
        end: r.end as u32,
    })
}

#[derive(Default)]
struct Builder<'f> {
    function: Option<&'f naga::Function>,
    logical: HashMap<Handle<naga::LocalVariable>, Logical>,
    ops: Vec<Op>,
    guards: Vec<Option<u32>>,
    spans: Vec<Option<SourceSpan>>,
    /// Jumps waiting for the end of the innermost loop or switch.
    breaks: Vec<Vec<usize>>,
    /// Jumps waiting for the continuing block of the innermost loop.
    continues: Vec<Vec<usize>>,
}

impl Builder<'_> {
    fn push(&mut self, op: Op, guard: Option<u32>, span: Option<SourceSpan>) -> usize {
        self.ops.push(op);
        self.guards.push(guard);
        self.spans.push(span);
        self.ops.len() - 1
    }

    fn here(&self) -> u32 {
        self.ops.len() as u32
    }

    fn patch_jump(&mut self, at: usize, target: u32) {
        match &mut self.ops[at] {
            Op::Jump(t) => *t = target,
            Op::Branch { otherwise, .. } => *otherwise = target,
            _ => unreachable!("only jumps and branches are patched"),
        }
    }

    /// Recognizes naga's lowering of `a || b` and `a && b`, which stores the result in an
    /// unnamed local: `if (!a or a) { tmp = b } else { tmp = true or false }`.
    fn short_circuit(
        &mut self,
        condition: Handle<Expression>,
        accept: &Block,
        reject: &Block,
    ) -> bool {
        let Some(f) = self.function else {
            return false;
        };
        let (
            Some(Statement::Store {
                pointer,
                value: other,
            }),
            1,
        ) = (reject.first(), reject.len())
        else {
            return false;
        };
        let Some(Statement::Store {
            pointer: p,
            value: rhs,
        }) = accept.last()
        else {
            return false;
        };
        let Expression::LocalVariable(var) = f.expressions[*pointer] else {
            return false;
        };
        if p != pointer || f.local_variables[var].name.is_some() {
            return false;
        }
        let or = matches!(
            f.expressions[*other],
            Expression::Literal(naga::Literal::Bool(true))
        );
        let lhs = match f.expressions[condition] {
            Expression::Unary {
                op: naga::UnaryOperator::LogicalNot,
                expr,
            } if or => expr,
            _ => condition,
        };
        self.logical.insert(var, Logical { or, lhs, rhs: *rhs });
        true
    }

    fn block(&mut self, block: &Block, guard: Option<u32>) {
        for (statement, span) in block.span_iter() {
            self.statement(statement, source_span(*span), guard);
        }
    }

    fn statement(&mut self, statement: &Statement, span: Option<SourceSpan>, guard: Option<u32>) {
        match statement {
            Statement::Emit(range) => {
                let range = range.index_range();
                if !range.is_empty() {
                    self.push(
                        Op::Emit {
                            start: range.start,
                            end: range.end,
                        },
                        guard,
                        span,
                    );
                }
            }
            Statement::Block(block) => self.block(block, guard),
            Statement::If {
                condition,
                accept,
                reject,
            } => {
                let internal = self.short_circuit(*condition, accept, reject);
                let at = self.push(
                    Op::Branch {
                        condition: *condition,
                        otherwise: 0,
                        internal,
                    },
                    guard,
                    span,
                );
                let inner = Some(at as u32);
                self.block(accept, inner);
                if reject.is_empty() {
                    let end = self.here();
                    self.patch_jump(at, end);
                } else {
                    let skip = self.push(Op::Jump(0), inner, None);
                    let otherwise = self.here();
                    self.patch_jump(at, otherwise);
                    self.block(reject, inner);
                    let end = self.here();
                    self.patch_jump(skip, end);
                }
            }
            Statement::Switch { selector, cases } => {
                let at = self.push(
                    Op::Switch {
                        selector: *selector,
                        cases: Vec::new(),
                    },
                    guard,
                    span,
                );
                let inner = Some(at as u32);
                self.breaks.push(Vec::new());
                let mut targets = Vec::with_capacity(cases.len());
                for case in cases {
                    targets.push((case.value, self.here()));
                    self.block(&case.body, inner);
                    if !case.fall_through {
                        let jump = self.push(Op::Jump(0), inner, None);
                        self.breaks.last_mut().expect("pushed above").push(jump);
                    }
                }
                let end = self.here();
                for jump in self.breaks.pop().expect("pushed above") {
                    self.patch_jump(jump, end);
                }
                if let Op::Switch { cases, .. } = &mut self.ops[at] {
                    *cases = targets;
                }
            }
            Statement::Loop {
                body,
                continuing,
                break_if,
            } => {
                let start = self.here();
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                self.block(body, guard);
                let continuing_at = self.here();
                for jump in self.continues.pop().expect("pushed above") {
                    self.patch_jump(jump, continuing_at);
                }
                self.block(continuing, guard);
                match break_if {
                    Some(condition) => self.push(
                        Op::BreakIf {
                            condition: *condition,
                            start,
                        },
                        guard,
                        None,
                    ),
                    None => self.push(Op::Jump(start), guard, None),
                };
                let end = self.here();
                for jump in self.breaks.pop().expect("pushed above") {
                    self.patch_jump(jump, end);
                }
            }
            Statement::Break => {
                let jump = self.push(Op::Jump(0), guard, span);
                if let Some(breaks) = self.breaks.last_mut() {
                    breaks.push(jump);
                }
            }
            Statement::Continue => {
                let jump = self.push(Op::Jump(0), guard, span);
                if let Some(continues) = self.continues.last_mut() {
                    continues.push(jump);
                }
            }
            Statement::Return { value } => {
                self.push(Op::Return(*value), guard, span);
            }
            Statement::Kill => {
                self.push(Op::Kill, guard, span);
            }
            Statement::Store { pointer, value } => {
                self.push(
                    Op::Store {
                        pointer: *pointer,
                        value: *value,
                    },
                    guard,
                    span,
                );
            }
            Statement::Call {
                function,
                arguments,
                result,
            } => {
                self.push(
                    Op::Call {
                        function: *function,
                        arguments: arguments.clone(),
                        result: *result,
                    },
                    guard,
                    span,
                );
            }
            // One invocation runs alone, so barriers have nothing to wait for.
            Statement::ControlBarrier(_) | Statement::MemoryBarrier(_) => {}
            Statement::ImageStore { .. } => {
                self.push(Op::Unsupported("textureStore"), guard, span);
            }
            Statement::Atomic { .. } | Statement::ImageAtomic { .. } => {
                self.push(Op::Unsupported("atomics"), guard, span);
            }
            Statement::WorkGroupUniformLoad { .. } => {
                self.push(Op::Unsupported("workgroupUniformLoad"), guard, span);
            }
            Statement::SubgroupBallot { .. }
            | Statement::SubgroupGather { .. }
            | Statement::SubgroupCollectiveOperation { .. } => {
                self.push(Op::Unsupported("subgroup operations"), guard, span);
            }
            _ => {
                self.push(Op::Unsupported("this statement"), guard, span);
            }
        }
    }
}
