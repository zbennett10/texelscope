use serde::{Deserialize, Serialize};
use texelscope_ir::{ExprRef, SourceSpan, Value};

/// One evaluated expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub expr: ExprRef,
    pub span: Option<SourceSpan>,
    pub value: Value,
    /// Indices of the earlier steps this value was computed from.
    pub inputs: Vec<u32>,
    /// Index of the branch condition step this one ran under, if any.
    pub guard: Option<u32>,
    /// Call depth, 0 in the entry point.
    pub depth: u32,
}

/// Everything one fragment invocation computed, in execution order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    pub entry_point: String,
    pub pixel: [u32; 2],
    pub steps: Vec<Step>,
    /// Fragment outputs by location. Empty when the invocation discarded.
    pub outputs: Vec<Value>,
    pub discarded: bool,
}
