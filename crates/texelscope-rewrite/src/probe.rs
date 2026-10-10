use serde::{Deserialize, Serialize};
use texelscope_capture::Rect;
use texelscope_ir::{ExprRef, ShaderModule};

use crate::RewriteError;

/// Expressions whose values the GPU should capture.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSet {
    pub exprs: Vec<ExprRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Rect>,
}

/// Where the instrumented shader runs. WebGPU probes write to a storage buffer. WebGL2 has
/// no storage buffers in fragment shaders, so its probes write to extra float color targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeTarget {
    WebGpu,
    WebGl2,
}

/// A rewritten shader and where it writes each probe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instrumented {
    pub source: String,
    pub slots: Vec<ProbeSlot>,
}

/// Where one probed value lands. For WebGPU `offset` is a byte offset within each pixel's
/// record, and for WebGL2 it is the index of the color target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSlot {
    pub expr: ExprRef,
    pub offset: u32,
    pub components: u32,
}

/// Rewrites the module so it writes every probed value alongside its normal output.
pub fn instrument(
    _module: &ShaderModule,
    _probes: &ProbeSet,
    _target: ProbeTarget,
) -> Result<Instrumented, RewriteError> {
    Err(RewriteError::Unsupported(
        "GPU probes are not written yet".to_string(),
    ))
}
