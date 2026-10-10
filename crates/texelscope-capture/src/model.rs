use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The capture format version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 0;

/// A recorded fragment workload that Texelscope can replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capture {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Frame index in the app the capture came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u64>,
    pub shader: Shader,
    pub target: Target,
    #[serde(default)]
    pub inputs: FragmentInputs,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Shader {
    pub language: Language,
    pub source: SourceRef,
    /// The fragment entry point to replay.
    pub entry_point: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Wgsl,
    Glsl,
}

/// Shader source given inline or as a path relative to the capture file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SourceRef {
    Inline(String),
    Path(String),
}

/// The color target the draw wrote to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
}

/// A rectangle of pixels in the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Whether the pixel at (x, y) lies inside.
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.width && y - self.y < self.height
    }
}

/// How the fragment shader's inputs are produced.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FragmentInputs {
    /// Only builtins such as position.
    #[default]
    Builtins,
    /// A full-screen pass, with each location interpolated from corner values.
    Fullscreen { locations: Vec<CornerInput> },
}

/// A user input interpolated across the target from its four corner values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CornerInput {
    pub location: u32,
    /// Values at the top-left, top-right, bottom-left and bottom-right corners.
    pub corners: [Vec<f32>; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub group: u32,
    pub binding: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub resource: Resource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Resource {
    Uniform { data: BufferData },
    Storage { data: BufferData },
    Texture(Texture),
    Sampler(Sampler),
}

/// Member values by name, or the raw bytes the shader sees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BufferData {
    /// Packed with the shader's layout when replayed.
    Values(BTreeMap<String, serde_json::Value>),
    Base64(String),
    /// A file relative to the capture.
    Path(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Texture {
    pub dimension: TextureDimension,
    pub format: TextureFormat,
    /// Width, height and depth or array layer count.
    pub size: [u32; 3],
    #[serde(default = "one")]
    pub mip_level_count: u32,
    pub data: TextureData,
}

/// Inline texel components, all mip levels in order with rows from the top, or raw bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TextureData {
    Inline(Vec<f32>),
    Base64(String),
    Path(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TextureDimension {
    #[serde(rename = "1d")]
    D1,
    #[serde(rename = "2d")]
    D2,
    #[serde(rename = "3d")]
    D3,
}

/// Texture formats by their WebGPU names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TextureFormat {
    #[serde(rename = "r8unorm")]
    R8Unorm,
    #[serde(rename = "r32float")]
    R32Float,
    #[serde(rename = "rg32float")]
    Rg32Float,
    #[serde(rename = "rgba8unorm")]
    Rgba8Unorm,
    #[serde(rename = "rgba8unorm-srgb")]
    Rgba8UnormSrgb,
    #[serde(rename = "bgra8unorm")]
    Bgra8Unorm,
    #[serde(rename = "rgba16float")]
    Rgba16Float,
    #[serde(rename = "rgba32float")]
    Rgba32Float,
}

impl TextureFormat {
    /// Components per texel.
    pub fn components(self) -> u32 {
        match self {
            Self::R8Unorm | Self::R32Float => 1,
            Self::Rg32Float => 2,
            Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Bgra8Unorm
            | Self::Rgba16Float
            | Self::Rgba32Float => 4,
        }
    }

    /// Bytes per texel in raw data.
    pub fn bytes_per_texel(self) -> u32 {
        match self {
            Self::R8Unorm => 1,
            Self::R32Float | Self::Rgba8Unorm | Self::Rgba8UnormSrgb | Self::Bgra8Unorm => 4,
            Self::Rg32Float | Self::Rgba16Float => 8,
            Self::Rgba32Float => 16,
        }
    }
}

/// Fields and defaults follow GPUSamplerDescriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Sampler {
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub address_mode_w: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_filter: FilterMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AddressMode {
    #[default]
    ClampToEdge,
    Repeat,
    MirrorRepeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FilterMode {
    #[default]
    Nearest,
    Linear,
}

fn one() -> u32 {
    1
}
