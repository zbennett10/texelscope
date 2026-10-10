//! Capture files hold everything Texelscope needs to replay one fragment workload.
//! docs/capture-format.md describes the JSON.

mod model;
mod validate;

use std::path::{Path, PathBuf};

pub use model::*;
pub use validate::validate;

/// Why a capture could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("could not read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("capture JSON is malformed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("capture is invalid:\n  {}", .0.join("\n  "))]
    Invalid(Vec<String>),
}

/// A capture read from disk, with its shader source resolved.
#[derive(Debug, Clone)]
pub struct LoadedCapture {
    pub capture: Capture,
    pub shader_source: String,
    /// The directory that relative paths in the capture resolve against.
    pub base_dir: PathBuf,
}

/// Parses and validates capture JSON without touching the filesystem.
pub fn parse(json: &str) -> Result<Capture, CaptureError> {
    let capture: Capture = serde_json::from_str(json)?;
    let problems = validate(&capture);
    if problems.is_empty() {
        Ok(capture)
    } else {
        Err(CaptureError::Invalid(problems))
    }
}

/// Reads a capture file and the shader source it points to.
pub fn load(path: &Path) -> Result<LoadedCapture, CaptureError> {
    let capture = parse(&read(path)?)?;
    let base_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let shader_source = match &capture.shader.source {
        SourceRef::Inline(source) => source.clone(),
        SourceRef::Path(relative) => read(&base_dir.join(relative))?,
    };
    Ok(LoadedCapture {
        capture,
        shader_source,
        base_dir,
    })
}

fn read(path: &Path) -> Result<String, CaptureError> {
    std::fs::read_to_string(path).map_err(|source| CaptureError::Io {
        path: path.to_path_buf(),
        source,
    })
}
