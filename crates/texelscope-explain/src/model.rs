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
    /// Why the pixel has its color, in a few sentences, most telling first.
    #[serde(default)]
    pub why: Vec<Reason>,
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
    /// Whether it comes only from constants and settings, so every pixel has the same value.
    #[serde(default)]
    pub setting: bool,
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
    /// Whether the condition reads only constants and settings, so every pixel goes one way.
    #[serde(default)]
    pub setting: bool,
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

/// One sentence of the account of why a pixel has its color. Code is set off in backticks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason {
    pub text: String,
    /// The code the sentence is about.
    pub span: Option<SourceSpan>,
}

impl Reason {
    pub(crate) fn at(span: Option<SourceSpan>, text: String) -> Self {
        Self { text, span }
    }
}

/// What sets two pixels apart, A being the one selected first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub a: [u32; 2],
    pub b: [u32; 2],
    pub color_a: Value,
    pub color_b: Value,
    /// What sets them apart, most telling first.
    pub summary: Vec<Reason>,
    /// Decisions that went differently or ran in one pixel only, in the order A made them.
    pub decisions: Vec<DecisionPair>,
    /// Named values on the way to each color, from the data to the color.
    pub values: Vec<ValuePair>,
    /// Why each pixel has its color.
    pub why_a: Vec<Reason>,
    pub why_b: Vec<Reason>,
}

/// One decision as each of two pixels made it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionPair {
    pub condition: String,
    pub span: Option<SourceSpan>,
    /// The condition, for flipping it.
    pub expr: Option<ExprRef>,
    /// How it went for each pixel, or `None` where that pixel never made it.
    pub a: Option<bool>,
    pub b: Option<bool>,
    /// How many times each pixel made it.
    pub times_a: u32,
    pub times_b: u32,
}

/// One named value as each of two pixels computed it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValuePair {
    pub name: String,
    /// The function it was computed in, when not the entry point.
    pub scope: Option<String>,
    pub span: Option<SourceSpan>,
    pub a: Option<Value>,
    pub b: Option<Value>,
}
