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
    /// Which call of its function computed it. Each call gets a new number.
    pub call: u32,
    /// The `let` name the source gives this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Where the value came from when it was read from outside the shader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Something worth knowing, such as an index that was clamped or a value that was pinned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Data a value was read from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Origin {
    /// A load from a uniform or storage buffer. `indices` walks members and elements.
    Buffer {
        group: u32,
        binding: u32,
        path: String,
        indices: Vec<u32>,
    },
    /// The texels a texture read or filtered.
    Texels {
        group: u32,
        binding: u32,
        name: String,
        level: u32,
        texels: Vec<[i32; 3]>,
    },
    /// A fragment input.
    Input {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        location: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BranchKind {
    If,
    Select,
    /// A loop's `break if`, taken when the loop ends.
    BreakIf,
    Switch,
}

/// A decision the invocation made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    /// How many steps ran before it.
    pub at: u32,
    pub kind: BranchKind,
    pub condition: ExprRef,
    /// The step that computed the condition, when it was not a constant.
    pub step: Option<u32>,
    pub taken: bool,
    /// How far the compared operands were from flipping the condition.
    pub margin: Option<f64>,
    /// Whether an override decided it.
    pub forced: bool,
    pub span: Option<SourceSpan>,
    pub depth: u32,
}

/// An assignment to a `var`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub at: u32,
    /// The variable and any member or element, such as `h.ri`.
    pub target: String,
    pub value: Value,
    /// The step that computed the stored value, when it was not a constant.
    pub step: Option<u32>,
    pub span: Option<SourceSpan>,
    pub depth: u32,
    /// Which call of its function made the assignment.
    pub call: u32,
    /// The branch condition step the assignment ran under, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<u32>,
}

/// One call of a function, numbered as steps' `call` numbers it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub function: texelscope_ir::FunctionRef,
    /// The step that computed each argument, or `None` for a constant.
    pub args: Vec<Option<u32>>,
}

/// A value the entry point returned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    pub value: Value,
}

/// Everything one fragment invocation computed, in execution order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    pub entry_point: String,
    pub pixel: [u32; 2],
    pub steps: Vec<Step>,
    pub branches: Vec<Branch>,
    pub stores: Vec<Store>,
    /// Every call the invocation made, the entry point first, indexed by call number.
    #[serde(default)]
    pub calls: Vec<Call>,
    /// Empty when the invocation discarded.
    pub outputs: Vec<Output>,
    /// The step that produced the entry point's return value.
    pub result: Option<u32>,
    /// The condition step the entry point returned or discarded under, if it did so inside a
    /// branch.
    pub result_guard: Option<u32>,
    pub discarded: bool,
    /// Whether recording stopped at the step limit before the invocation ended.
    pub truncated: bool,
    /// Problems with the capture that the run worked around.
    pub notes: Vec<String>,
}

impl Trace {
    /// The color written to location 0, if there is one.
    pub fn color(&self) -> Option<&Value> {
        self.outputs
            .iter()
            .find(|o| o.location == Some(0))
            .map(|o| &o.value)
    }
}
