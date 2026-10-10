use serde::{Deserialize, Serialize};
use texelscope_capture::Rect;
use texelscope_ir::{ExprRef, ShaderModule, Value};

use crate::RewriteError;

/// One what-if change. Overrides address expressions, so they apply the same way to WGSL,
/// GLSL and the interpreter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Override {
    /// Replace an expression's value with a constant. Scrubbing a literal or pinning a
    /// uniform is a pin.
    Pin { expr: ExprRef, value: Value },
    /// Send the `if` whose condition is `condition` down one side.
    ForceBranch { condition: ExprRef, taken: bool },
    /// Replace an expression with WGSL evaluated in its scope.
    Replace { expr: ExprRef, wgsl: String },
}

/// A set of overrides, optionally limited to one region of the target.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub overrides: Vec<Override>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Rect>,
}

/// Applies a patch and returns the rewritten module, which still validates.
pub fn apply(_module: &ShaderModule, _patch: &Patch) -> Result<ShaderModule, RewriteError> {
    Err(RewriteError::Unsupported(
        "overrides are not applied yet".to_string(),
    ))
}
