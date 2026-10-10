use std::path::Path;
use std::sync::Arc;

use texelscope_capture::Capture;
use texelscope_ir::ShaderModule;

use super::{CaptureData, InterpError, Interpreter};

fn scalar_field() -> Interpreter {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/scalar-field/capture.json");
    let loaded = texelscope_capture::load(&path).unwrap();
    let module = ShaderModule::from_wgsl(loaded.shader_source.clone()).unwrap();
    let data = CaptureData::new(loaded.capture, Some(&loaded.base_dir)).unwrap();
    Interpreter::new(Arc::new(module), Arc::new(data)).unwrap()
}

#[test]
fn rejects_pixels_outside_the_target() {
    let error = scalar_field().trace_pixel([256, 0], &[]).unwrap_err();
    assert!(matches!(
        error,
        InterpError::PixelOutOfBounds { x: 256, .. }
    ));
}

#[test]
fn rejects_entry_points_that_are_not_fragment_shaders() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/scalar-field/capture.json");
    let mut loaded = texelscope_capture::load(&path).unwrap();
    loaded.capture.shader.entry_point = "vs_main".to_string();
    let module = ShaderModule::from_wgsl(loaded.shader_source.clone()).unwrap();
    let capture: Capture = loaded.capture;
    let data = CaptureData::new(capture, None).unwrap();
    let error = Interpreter::new(Arc::new(module), Arc::new(data))
        .err()
        .unwrap();
    assert!(matches!(error, InterpError::NotFragment(_)));
}
