//! Rewrites of naga IR. It emits code back out as WGSL and GLSL ES, applies what-if
//! overrides, and adds GPU probes that capture chosen values.

mod emit;
mod patch;
mod probe;

pub use emit::{emit_glsl_es, emit_wgsl};
pub use patch::{Override, Patch, apply};
pub use probe::{Instrumented, ProbeSet, ProbeSlot, ProbeTarget, instrument};

#[derive(Debug, thiserror::Error)]
pub enum RewriteError {
    #[error("the shader has no entry point named {0:?}")]
    NoEntryPoint(String),
    #[error("could not emit {target}: {message}")]
    Emit {
        target: &'static str,
        message: String,
    },
    #[error("{0}")]
    Unsupported(String),
}
