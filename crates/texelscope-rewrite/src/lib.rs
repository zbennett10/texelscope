//! Rewrites of naga IR. It emits code back out as WGSL and GLSL ES, applies what-if
//! overrides, and adds GPU probes that output a chosen value instead of the color. GLSL from
//! WebGL2 is rewritten as text, for the app's own context to run.

mod emit;
mod glsl;
mod patch;
mod probe;

pub use emit::{emit_glsl_es, emit_wgsl};
pub use glsl::{LOOP_LIMIT, replay_glsl};
pub use patch::{Patch, apply, gpu_source, patch_source};
pub use probe::{NOT_COMPUTED, Probe, probe};
pub use texelscope_ir::Override;

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
