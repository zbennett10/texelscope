use std::path::Path;

use texelscope_ir::ShaderModule;
use texelscope_rewrite::{RewriteError, emit_glsl_es, emit_wgsl};

fn example(path: &str) -> ShaderModule {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(path);
    ShaderModule::from_wgsl(std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn emitted_wgsl_validates_again() {
    for path in ["scalar-field/field.wgsl", "volume-raymarch/volume.wgsl"] {
        let wgsl = emit_wgsl(&example(path)).unwrap();
        ShaderModule::from_wgsl(wgsl).unwrap_or_else(|e| panic!("{path} round trip failed:\n{e}"));
    }
}

#[test]
fn emits_webgl2_glsl_for_both_stages() {
    let module = example("scalar-field/field.wgsl");
    for entry in ["vs_main", "fs_main"] {
        let glsl = emit_glsl_es(&module, entry).unwrap();
        assert!(glsl.starts_with("#version 300 es"), "{entry}:\n{glsl}");
    }
}

#[test]
fn names_a_missing_entry_point() {
    let error = emit_glsl_es(&example("scalar-field/field.wgsl"), "main").unwrap_err();
    assert!(matches!(error, RewriteError::NoEntryPoint(name) if name == "main"));
}
