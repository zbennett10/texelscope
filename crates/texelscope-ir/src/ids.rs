use serde::{Deserialize, Serialize};

/// A function in a module. Helpers count by their index in `Module::functions` and entry
/// points by their index in `Module::entry_points`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "index", rename_all = "camelCase")]
pub enum FunctionRef {
    Function(u32),
    EntryPoint(u32),
}

/// One expression in one function. Traces, overrides and GPU probes all address code this
/// way, so a value means the same thing in WGSL, in naga's GLSL output and in a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExprRef {
    pub function: FunctionRef,
    pub expr: u32,
}

/// A byte range in the shader source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: u32,
    pub end: u32,
}

impl SourceSpan {
    pub(crate) fn from_naga(span: naga::Span) -> Option<Self> {
        span.to_range().map(|range| Self {
            start: range.start as u32,
            end: range.end as u32,
        })
    }
}

/// A 1-based line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}
