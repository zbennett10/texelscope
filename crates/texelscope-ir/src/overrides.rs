use serde::{Deserialize, Serialize};

use crate::{ExprRef, Value};

/// One what-if change. Overrides address expressions, so they apply the same way to WGSL,
/// GLSL and the interpreter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Override {
    /// Replace an expression's value with a constant. Scrubbing a literal or pinning a
    /// uniform is a pin.
    Pin { expr: ExprRef, value: Value },
    /// Make the condition `condition` come out as `taken` wherever it is used.
    ForceBranch { condition: ExprRef, taken: bool },
    /// Replace an expression with WGSL evaluated in its scope.
    Replace { expr: ExprRef, wgsl: String },
}

impl Override {
    /// The expression the override changes.
    pub fn target(&self) -> ExprRef {
        match self {
            Override::Pin { expr, .. } | Override::Replace { expr, .. } => *expr,
            Override::ForceBranch { condition, .. } => *condition,
        }
    }
}
