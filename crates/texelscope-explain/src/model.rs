use serde::{Deserialize, Serialize};
use texelscope_interp::Perturbation;
use texelscope_ir::{ExprRef, SourceSpan, Value};

/// Why a pixel has its color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Explanation {
    pub pixel: [u32; 2],
    pub color: Value,
    pub discarded: bool,
    /// From the inputs to the color, in the order the shader computed them.
    pub links: Vec<Link>,
    /// Decisions on the way, in the order the shader made them.
    pub decisions: Vec<Decision>,
    /// Inputs ordered by how much they move the color, most first.
    pub sensitivities: Vec<Sensitivity>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkKind {
    /// A fragment input such as position.
    Input,
    /// A value read from a uniform or storage buffer.
    Buffer,
    /// A value read from a texture.
    Texture,
    /// A named `let`.
    Let,
    /// The last value assigned to a `var`.
    Var,
}

/// One value on the way from inputs to color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub kind: LinkKind,
    pub name: String,
    /// The helper function the value lives in, or `None` in the entry point.
    pub scope: Option<String>,
    pub value: Value,
    /// Where the value came from, in words.
    pub detail: String,
    pub span: Option<SourceSpan>,
    /// The expression to pin when trying a different value.
    pub expr: Option<ExprRef>,
    /// The trace step that computed it.
    pub step: Option<u32>,
}

/// A branch condition and how it went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub condition: String,
    pub taken: bool,
    /// How far the operands were from flipping the condition, in their own units.
    pub margin: Option<f64>,
    pub span: Option<SourceSpan>,
    /// The condition to force when trying the other side.
    pub expr: Option<ExprRef>,
    /// How many times the pixel decided it, such as once per loop iteration.
    pub count: u32,
    pub forced: bool,
}

/// How strongly one input moves the output color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sensitivity {
    pub input: String,
    /// The input's value in this run.
    pub value: f64,
    /// Change in each color channel per unit change of the input.
    pub d_color: [f64; 4],
    pub perturbation: Option<Perturbation>,
}
