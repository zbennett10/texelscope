//! The front end for Texelscope, built on naga. It parses and validates WGSL and the GLSL ES
//! that WebGL2 runs, gives every expression a stable id, and defines the value model the other
//! crates share.

mod glsl;
mod ids;
mod module;
mod overrides;
mod value;

pub use glsl::{GlslInput, GlslLayout, GlslUniform, GlslUniformKind};
pub use ids::{ExprRef, FunctionRef, LineCol, SourceSpan};
pub use module::{
    EntryPointInfo, IrError, Language, NamedValue, NamedValueKind, ResourceInfo, ResourceKind,
    ShaderModule, Stage, StageInput,
};
pub use overrides::Override;
pub use value::Value;

/// The naga every Texelscope crate builds against.
pub use naga;
