use serde::{Deserialize, Serialize};
use texelscope_ir::{SourceSpan, Value};

/// Why a pixel has its color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Explanation {
    pub pixel: [u32; 2],
    pub color: Value,
    /// From the inputs to the color, in order.
    pub links: Vec<Link>,
    /// Branches that shaped the result.
    pub decisions: Vec<Decision>,
    /// Inputs ordered by how much they move the color, most first.
    pub sensitivities: Vec<Sensitivity>,
}

/// One named value on the way from inputs to color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub name: String,
    pub value: Value,
    /// Where the value came from, in words.
    pub detail: String,
    pub span: Option<SourceSpan>,
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
}

/// How strongly one input moves the output color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sensitivity {
    pub input: String,
    /// Change in each color channel per unit change of the input.
    pub d_color: [f64; 4],
}
