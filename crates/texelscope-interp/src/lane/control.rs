//! Branches and switches, with how close each decision came to going the other way.

use texelscope_ir::ExprRef;
use texelscope_ir::naga::{self, BinaryOperator, Expression, Handle, UnaryOperator};

use super::{Lane, NO_STEP};
use crate::InterpError;
use crate::record::BranchFacts;
use crate::trace::BranchKind;
use crate::val::Scalar;

impl<'a> Lane<'a> {
    /// Decides a branch. Branches without a kind are naga's own and go unrecorded.
    pub(super) fn decide(
        &mut self,
        condition: Handle<Expression>,
        pc: usize,
        kind: Option<BranchKind>,
    ) -> Result<bool, InterpError> {
        self.ensure(condition)?;
        let taken = self.value(condition)?.as_bool()?;
        let taken = match kind {
            Some(kind) => self.note_decision(condition, taken, kind),
            None => taken,
        };
        let step = self.step_of(condition).unwrap_or(NO_STEP);
        self.top_mut().branch_steps[pc] = step;
        Ok(taken)
    }

    /// Applies a forced branch and records the decision. Returns the side taken.
    pub(super) fn note_decision(
        &mut self,
        condition: Handle<Expression>,
        taken: bool,
        kind: BranchKind,
    ) -> bool {
        let function = self.top().compiled.function;
        let forced = self
            .overrides
            .branches
            .get(&(function, condition.index() as u32))
            .copied();
        let decided = forced.unwrap_or(taken);
        if self.record.is_on() {
            let margin = self.margin(condition, 0);
            let frame = self.top();
            let step = self.step_of(condition);
            let span = frame.compiled.spans[condition.index()];
            let depth = self.frames.len() as u32 - 1;
            self.record.branch(BranchFacts {
                condition: ExprRef {
                    function,
                    expr: condition.index() as u32,
                },
                kind,
                step,
                taken: decided,
                margin,
                forced: forced.is_some(),
                span,
                depth,
            });
        }
        decided
    }

    /// How far a condition's compared operands are from flipping it.
    pub(super) fn margin(&self, condition: Handle<Expression>, depth: u32) -> Option<f64> {
        if depth > 8 {
            return None;
        }
        let frame = self.top();
        let scalar = |h: Handle<Expression>| frame.values[h.index()].as_ref()?.scalar().ok();
        match frame.ir.expressions[condition] {
            Expression::Binary { op, left, right } => {
                use BinaryOperator as B;
                match op {
                    B::Less
                    | B::LessEqual
                    | B::Greater
                    | B::GreaterEqual
                    | B::Equal
                    | B::NotEqual => {
                        let (l, r) = (scalar(left)?, scalar(right)?);
                        if matches!(l, Scalar::Bool(_)) {
                            return None;
                        }
                        let m = (l.as_f64() - r.as_f64()).abs();
                        m.is_finite().then_some(m)
                    }
                    B::LogicalAnd | B::LogicalOr | B::And | B::InclusiveOr => {
                        let (Scalar::Bool(lv), Scalar::Bool(rv)) = (scalar(left)?, scalar(right)?)
                        else {
                            return None;
                        };
                        let (lm, rm) =
                            (self.margin(left, depth + 1), self.margin(right, depth + 1));
                        let and = matches!(op, B::LogicalAnd | B::And);
                        let result = if and { lv && rv } else { lv || rv };
                        let either = || match (lm, rm) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            (a, b) => a.or(b),
                        };
                        let both = || Some(lm?.max(rm?));
                        // Flipping needs one operand to change if any single one decides it.
                        match (and, result) {
                            (true, true) | (false, false) => either(),
                            (true, false) if !lv && !rv => both(),
                            (false, true) if lv && rv => both(),
                            (true, false) => {
                                if lv {
                                    rm
                                } else {
                                    lm
                                }
                            }
                            (false, true) => {
                                if lv {
                                    lm
                                } else {
                                    rm
                                }
                            }
                        }
                    }
                    _ => None,
                }
            }
            Expression::Unary {
                op: UnaryOperator::LogicalNot,
                expr,
            } => self.margin(expr, depth + 1),
            // The result of a short-circuited `&&` or `||` that naga keeps in a local.
            Expression::Load { pointer } => {
                let Expression::LocalVariable(var) = frame.ir.expressions[pointer] else {
                    return None;
                };
                let logical = frame.compiled.logical.get(&var)?;
                let value = |h: Handle<Expression>| match frame.values[h.index()]
                    .as_ref()?
                    .scalar()
                    .ok()?
                {
                    Scalar::Bool(b) => Some(b),
                    _ => None,
                };
                let lv = value(logical.lhs)?;
                let lm = self.margin(logical.lhs, depth + 1);
                // The right side only ran when the left side did not decide the result.
                let decided_by_left = lv == logical.or;
                if decided_by_left {
                    return lm;
                }
                let rv = value(logical.rhs)?;
                let rm = self.margin(logical.rhs, depth + 1);
                let result = if logical.or { lv || rv } else { lv && rv };
                match (logical.or, result) {
                    (true, true) | (false, false) => rm,
                    _ => match (lm, rm) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    },
                }
            }
            _ => None,
        }
    }

    pub(super) fn switch(
        &mut self,
        selector: Handle<Expression>,
        cases: &[(naga::SwitchValue, u32)],
        pc: usize,
    ) -> Result<usize, InterpError> {
        let value = self.owned(selector)?.scalar()?;
        let step = self.step_of(selector).unwrap_or(NO_STEP);
        self.top_mut().branch_steps[pc] = step;
        let matches = |case: &naga::SwitchValue| match (case, value) {
            (naga::SwitchValue::I32(c), Scalar::I32(v)) => *c == v,
            (naga::SwitchValue::U32(c), Scalar::U32(v)) => *c == v,
            (naga::SwitchValue::I32(c), other) => i64::from(*c) == other.as_i64(),
            (naga::SwitchValue::U32(c), other) => i64::from(*c) == other.as_i64(),
            (naga::SwitchValue::Default, _) => false,
        };
        let target = cases
            .iter()
            .find(|(case, _)| matches(case))
            .or_else(|| {
                cases
                    .iter()
                    .find(|(case, _)| *case == naga::SwitchValue::Default)
            })
            .map(|&(_, target)| target as usize);
        if self.record.is_on() {
            let function = self.top().compiled.function;
            let span = self.top().compiled.spans[selector.index()];
            let depth = self.frames.len() as u32 - 1;
            let step = self.step_of(selector);
            self.record.branch(BranchFacts {
                condition: ExprRef {
                    function,
                    expr: selector.index() as u32,
                },
                kind: BranchKind::Switch,
                step,
                taken: target.is_some(),
                margin: None,
                forced: false,
                span,
                depth,
            });
        }
        // A switch with no matching case and no default does nothing.
        Ok(target.unwrap_or_else(|| self.switch_end(pc)))
    }

    pub(super) fn switch_end(&self, pc: usize) -> usize {
        let ops = &self.top().compiled.ops;
        let guards = &self.top().compiled.guards;
        (pc + 1..ops.len())
            .find(|&i| guards[i] != Some(pc as u32))
            .unwrap_or(ops.len())
    }
}
