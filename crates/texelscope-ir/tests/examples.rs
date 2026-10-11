use std::path::Path;

use texelscope_ir::{FunctionRef, Language, NamedValueKind, ShaderModule, Stage};

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

#[test]
fn lists_the_lines_that_compute_values() {
    let module = ShaderModule::from_wgsl(
        "fn unused(x: f32) -> f32 {
    return x * 2.0;
}

@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let a = p.x * 0.5;
    // Signatures, comments and constants are not code that runs as a step.
    return vec4<f32>(a, 0.0, 0.0, 1.0);
}
",
    )
    .unwrap();
    assert_eq!(module.code_lines(), [2, 6, 8]);
}

#[test]
fn reads_glsl_es_as_webgl2_runs_it() {
    let source = "#version 300 es
precision highp float;
uniform float u_scale;
uniform sampler2D u_tex;
in vec2 v_uv;
out vec4 color;
void main() {
    float level = v_uv.x * u_scale;
    color = texture(u_tex, v_uv) * level;
}
";
    let module = ShaderModule::from_glsl(source).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(module.source(), source);
    assert_eq!(module.language(), Language::Glsl);
    let names: Vec<String> = module.resources().into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["u_scale", "u_tex", "u_tex_sampler"]);
    // Spans land on the original text and cover whole operands.
    let (_, f) = module.function(FunctionRef::Function(0)).unwrap();
    let texts: Vec<&str> = f
        .expressions
        .iter()
        .filter_map(|(h, _)| f.expressions.get_span(h).to_range())
        .map(|r| &source[r])
        .collect();
    assert!(texts.contains(&"v_uv.x * u_scale"), "{texts:?}");
    assert!(texts.contains(&"texture(u_tex, v_uv) * level"), "{texts:?}");
    let level = module
        .named_values()
        .into_iter()
        .find(|v| v.name == "level")
        .unwrap();
    assert!(module.text(level.span.unwrap()).starts_with("level"));
    assert_eq!(module.code_lines(), [8, 9]);
}

#[test]
fn spans_glsl_builtin_calls() {
    let source = "#version 300 es
precision highp float;
uniform float u_edge;
in vec2 v_uv;
out vec4 color;
void main() {
    float ring = step(u_edge, length(v_uv - 0.5));
    color = vec4(mix(vec3(0.1, 0.2, 0.6), vec3(1.0), ring), 1.0);
}
";
    let module = ShaderModule::from_glsl(source).unwrap_or_else(|e| panic!("{e}"));
    let (_, f) = module.function(FunctionRef::Function(0)).unwrap();
    let texts: Vec<&str> = f
        .expressions
        .iter()
        .filter_map(|(h, _)| f.expressions.get_span(h).to_range())
        .map(|r| &source[r])
        .collect();
    for call in [
        "step(u_edge, length(v_uv - 0.5))",
        "length(v_uv - 0.5)",
        "mix(vec3(0.1, 0.2, 0.6), vec3(1.0), ring)",
    ] {
        assert!(texts.contains(&call), "{call:?} is not in {texts:?}");
    }
}
