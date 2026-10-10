use std::path::Path;

use texelscope_ir::{NamedValueKind, ShaderModule, Stage};

fn example(path: &str) -> ShaderModule {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(path);
    let source = std::fs::read_to_string(&path).expect("example should exist");
    ShaderModule::from_wgsl(source)
        .unwrap_or_else(|e| panic!("{} should validate:\n{e}", path.display()))
}

#[test]
fn scalar_field_lists_its_entry_points() {
    let module = example("scalar-field/field.wgsl");
    let stages: Vec<_> = module
        .entry_points()
        .into_iter()
        .map(|ep| (ep.name, ep.stage))
        .collect();
    assert_eq!(
        stages,
        [
            ("vs_main".to_string(), Stage::Vertex),
            ("fs_main".to_string(), Stage::Fragment)
        ]
    );
}

#[test]
fn named_values_point_at_their_lines() {
    let module = example("scalar-field/field.wgsl");
    let value = module
        .named_values()
        .into_iter()
        .find(|v| v.function_name == "fs_main" && v.name == "value")
        .expect("fs_main binds value");

    assert_eq!(value.kind, NamedValueKind::Let);
    assert_eq!(value.at.map(|at| at.line), Some(46));
    assert_eq!(module.span_of(value.expr.unwrap()), value.span);
}

#[test]
fn volume_example_validates() {
    let module = example("volume-raymarch/volume.wgsl");
    let vars: Vec<_> = module
        .named_values()
        .into_iter()
        .filter(|v| v.kind == NamedValueKind::Var && v.function_name == "fs_main")
        .map(|v| v.name)
        .collect();
    assert_eq!(vars, ["color", "alpha", "i"]);
}

#[test]
fn reports_parse_errors_with_the_source_line() {
    let error = ShaderModule::from_wgsl("fn broken( {}").unwrap_err();
    assert!(error.to_string().contains("fn broken( {}"), "{error}");
}

#[test]
fn arguments_are_not_lets() {
    let module = example("scalar-field/field.wgsl");
    let kinds: Vec<_> = module
        .named_values()
        .into_iter()
        .filter(|v| v.function_name == "fs_main")
        .map(|v| (v.name, v.kind))
        .collect();
    assert_eq!(kinds[0], ("in".to_string(), NamedValueKind::Argument));
    assert!(
        kinds[1..]
            .iter()
            .all(|(_, kind)| *kind == NamedValueKind::Let)
    );
}

#[test]
fn resources_serialize_as_the_browser_reads_them() {
    let module = example("scalar-field/field.wgsl");
    let json = serde_json::to_value(module.resources()).unwrap();
    assert_eq!(
        json[1]["kind"],
        serde_json::json!({ "kind": "texture", "viewDimension": "2d", "sampleType": "float", "multisampled": false })
    );
}
