//! The reference interpreter. It runs a capture's fragment shader on the CPU one pixel at a
//! time and records every value it computes, so a pixel can be traced, explained and
//! compared with the GPU.

mod trace;

pub use trace::{Step, Trace};

use texelscope_capture::Capture;
use texelscope_ir::{ShaderModule, Stage};

/// Why a pixel could not be traced.
#[derive(Debug, thiserror::Error)]
pub enum InterpError {
    #[error("the shader has no entry point named {0:?}")]
    NoEntryPoint(String),
    #[error("entry point {0:?} is not a fragment shader")]
    NotFragment(String),
    #[error("pixel ({x}, {y}) is outside the {width}x{height} target")]
    PixelOutOfBounds {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    #[error("{0}")]
    Unsupported(String),
}

/// Runs one capture's fragment entry point.
pub struct Interpreter<'a> {
    module: &'a ShaderModule,
    capture: &'a Capture,
}

impl<'a> Interpreter<'a> {
    pub fn new(module: &'a ShaderModule, capture: &'a Capture) -> Self {
        Self { module, capture }
    }

    /// Runs the fragment shader for one pixel and records every value it computes.
    pub fn trace_pixel(&self, pixel: [u32; 2]) -> Result<Trace, InterpError> {
        let name = &self.capture.shader.entry_point;
        let entry = self
            .module
            .entry_point(name)
            .ok_or_else(|| InterpError::NoEntryPoint(name.clone()))?;
        if entry.stage != Stage::Fragment {
            return Err(InterpError::NotFragment(name.clone()));
        }
        let target = self.capture.target;
        let [x, y] = pixel;
        if x >= target.width || y >= target.height {
            return Err(InterpError::PixelOutOfBounds {
                x,
                y,
                width: target.width,
                height: target.height,
            });
        }
        Err(InterpError::Unsupported(
            "the interpreter does not execute shaders yet".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use texelscope_capture::LoadedCapture;
    use texelscope_ir::ShaderModule;

    use super::{InterpError, Interpreter};

    fn scalar_field() -> (LoadedCapture, ShaderModule) {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/scalar-field/capture.json");
        let loaded = texelscope_capture::load(&path).unwrap();
        let module = ShaderModule::from_wgsl(loaded.shader_source.clone()).unwrap();
        (loaded, module)
    }

    #[test]
    fn rejects_pixels_outside_the_target() {
        let (loaded, module) = scalar_field();
        let error = Interpreter::new(&module, &loaded.capture)
            .trace_pixel([256, 0])
            .unwrap_err();
        assert!(matches!(
            error,
            InterpError::PixelOutOfBounds { x: 256, .. }
        ));
    }

    #[test]
    fn rejects_entry_points_that_are_not_fragment_shaders() {
        let (mut loaded, module) = scalar_field();
        loaded.capture.shader.entry_point = "vs_main".to_string();
        let error = Interpreter::new(&module, &loaded.capture)
            .trace_pixel([0, 0])
            .unwrap_err();
        assert!(matches!(error, InterpError::NotFragment(_)));
    }
}
