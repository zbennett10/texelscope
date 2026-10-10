//! Explanations of traced pixels. Each one gives the chain from inputs to color, the
//! branches that decided it and the inputs that move it most.

mod model;
mod text;

pub use model::{Decision, Explanation, Link, Sensitivity};
pub use text::render_text;

use texelscope_interp::Trace;
use texelscope_ir::ShaderModule;

#[derive(Debug, thiserror::Error)]
pub enum ExplainError {
    #[error("{0}")]
    Unsupported(String),
}

/// Builds an explanation by walking back from a trace's output to its inputs.
pub fn explain(_module: &ShaderModule, _trace: &Trace) -> Result<Explanation, ExplainError> {
    Err(ExplainError::Unsupported(
        "explanations are not built yet".to_string(),
    ))
}
