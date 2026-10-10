//! The WGSL front end for Texelscope, built on naga. It parses and validates shaders, gives
//! every expression a stable id, and defines the value model the other crates share.

mod ids;
mod module;
mod value;

pub use ids::{ExprRef, FunctionRef, LineCol, SourceSpan};
pub use module::{EntryPointInfo, IrError, NamedValue, NamedValueKind, ShaderModule, Stage};
pub use value::Value;

/// The naga every Texelscope crate builds against.
pub use naga;
