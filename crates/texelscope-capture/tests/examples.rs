use std::path::Path;

use texelscope_capture::{FragmentInputs, Resource, TextureFormat, load};

#[test]
fn scalar_field_example_loads() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/scalar-field/capture.json");
    let loaded = load(&path).expect("example capture should load");
    let capture = &loaded.capture;

    assert_eq!(capture.shader.entry_point, "fs_main");
    assert!(loaded.shader_source.contains("fn fs_main"));
    assert_eq!(capture.target.format, TextureFormat::Rgba8Unorm);
    assert!(matches!(capture.inputs, FragmentInputs::Fullscreen { .. }));
    assert_eq!(capture.bindings.len(), 2);
    assert!(matches!(capture.bindings[1].resource, Resource::Texture(_)));
}
