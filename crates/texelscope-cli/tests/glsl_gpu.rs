//! The GLSL a WebGL2 app's own context runs for a what-if or a probe must compute what the
//! interpreter computes with the override applied, and keep every name the app binds.

use std::sync::Arc;

use serde_json::json;
use texelscope_explain::explain_pixel;
use texelscope_interp::{CaptureData, Interpreter};
use texelscope_ir::{ExprRef, Override, ShaderModule, Value};
use texelscope_rewrite::{LOOP_LIMIT, NOT_COMPUTED, gpu_source, probe};

const SHADER: &str = "#version 300 es
precision highp float;
uniform float u_gain;
uniform int u_steps;
uniform bool u_flag;
uniform vec3 u_tints[2];
in vec2 v_uv;
out vec4 color;

float ramp(float t, float k) {
    return clamp(t * k, 0.0, 1.0);
}

void main() {
    float level = ramp(v_uv.x, u_gain);
    vec3 tint = u_tints[1] * (v_uv.y > 0.5 ? 2.0 : 0.5);
    float acc = 0.0;
    for (int i = 0; i < u_steps; i++) {
        acc += 0.25 * level;
        if (acc > 0.6) break;
    }
    int turns = 0;
    while (turns < 2) { turns++; }
    if (u_flag && level > 0.25) {
        tint.x += 0.125;
    }
    color = vec4(tint * level, acc + float(turns));
}
";

/// Little-endian f32s as base64, as a WebGL capture packs them.
fn packed(values: &[f32]) -> String {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    texelscope_capture::encode_base64(&bytes)
}

fn data() -> Arc<CaptureData> {
    let (mut frag, mut uv) = (Vec::new(), Vec::new());
    for y in 0..4 {
        for x in 0..4 {
            frag.extend([x as f32 + 0.5, 3.5 - y as f32, 0.5, 1.0]);
            uv.extend([x as f32 / 3.0, y as f32 / 3.0]);
        }
    }
    let uniform = |name: &str, value: serde_json::Value| {
        json!({ "group": 0, "binding": 0, "name": name, "resource": {
            "kind": "uniform", "data": { "values": { name: value } } } })
    };
    let capture = json!({
        "version": 0,
        "shader": { "language": "glsl", "source": { "inline": SHADER }, "entryPoint": "main" },
        "target": { "width": 4, "height": 4, "format": "rgba8unorm", "yUp": true },
        "inputs": {
            "kind": "window", "rect": { "x": 0, "y": 0, "width": 4, "height": 4 },
            "fragCoord": packed(&frag),
            "varyings": [{ "name": "v_uv", "components": 2, "data": packed(&uv) }]
        },
        "bindings": [
            uniform("u_gain", json!(1.5)),
            uniform("u_steps", json!(4)),
            uniform("u_flag", json!(true)),
            uniform("u_tints", json!([[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]])),
        ],
    });
    let capture = texelscope_capture::parse(&capture.to_string()).unwrap_or_else(|e| panic!("{e}"));
    Arc::new(CaptureData::new(capture, None).unwrap_or_else(|e| panic!("{e}")))
}

fn interpreter(source: &str) -> Interpreter {
    let module = ShaderModule::from_glsl(source).unwrap_or_else(|e| panic!("{e}\n{source}"));
    Interpreter::new(Arc::new(module), data()).unwrap_or_else(|e| panic!("{e}"))
}

/// A nearby value of the same type, as someone trying another value would enter.
fn nudged(value: &Value) -> Option<Value> {
    Some(match value {
        Value::Bool(b) => Value::Bool(!b),
        Value::I32(i) => Value::I32(i + 1),
        Value::U32(u) => Value::U32(u + 1),
        Value::F32(f) => Value::F32(f * 1.5 + 0.25),
        Value::Vector(lanes) => Value::Vector(lanes.iter().map(nudged).collect::<Option<_>>()?),
        _ => return None,
    })
}

#[test]
fn the_glsl_for_the_app_agrees_with_the_interpreter() {
    let interp = interpreter(SHADER);
    let module = interp.module();
    let (mut checked, mut skipped) = (0, Vec::new());
    for pixel in (0..16).map(|i| [i % 4, i / 4]) {
        let explanation = explain_pixel(&interp, pixel, &[]).unwrap();
        let pins = explanation.links.iter().filter_map(|link| {
            let o = Override::Pin {
                expr: link.expr?,
                value: nudged(&link.value)?,
            };
            Some((link.name.clone(), o))
        });
        let flips = explanation.decisions.iter().filter_map(|d| {
            let flip = Override::ForceBranch {
                condition: d.expr?,
                taken: !d.taken,
            };
            Some((d.condition.clone(), flip))
        });
        for (label, o) in pins.chain(flips).collect::<Vec<_>>() {
            let what = format!("{label} at {pixel:?}");
            // A flip that keeps a loop going ends at the loop limit on both sides.
            let expected = interp
                .shade_pixel(pixel, std::slice::from_ref(&o), None)
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            let source = match gpu_source(module, std::slice::from_ref(&o)) {
                Ok(source) => source,
                Err(e) => {
                    skipped.push(format!("{what}: {e}"));
                    continue;
                }
            };
            let shown = interpreter(&source).shade_pixel(pixel, &[], None).unwrap();
            assert_eq!(shown.color(), expected.color(), "{what}\n{source}");
            checked += 1;
        }
    }
    assert!(checked > 150, "only {checked} overrides were checked");
    // Only increments, whose text names no value, can't be pinned yet.
    assert!(
        skipped.iter().all(|s| s.contains("increment")),
        "{skipped:#?}"
    );
}

#[test]
fn keeps_every_name_the_app_binds() {
    let module = ShaderModule::from_glsl(SHADER).unwrap();
    let source = gpu_source(&module, &[]).unwrap();
    for line in SHADER
        .lines()
        .filter(|l| l.starts_with("uniform") || l.starts_with("in ") || l.starts_with("out "))
    {
        assert!(source.contains(line), "{line} is gone from\n{source}");
    }
}

#[test]
fn keeps_every_line_where_it_was() {
    // The app's compiler reports errors by line, and the editor shows the original lines.
    let module = ShaderModule::from_glsl(SHADER).unwrap();
    let source = gpu_source(&module, &[]).unwrap();
    assert!(source.contains("texelscope_more("), "{source}");
    let line = |text: &str, needle: &str| text.lines().position(|l| l.contains(needle));
    for needle in [
        "float ramp(float t, float k) {",
        "void main() {",
        "color = vec4(",
    ] {
        assert_eq!(
            line(&source, needle),
            line(SHADER, needle),
            "{needle}\n{source}"
        );
    }
}

#[test]
fn stops_a_loop_a_flip_keeps_going() {
    let interp = interpreter(SHADER);
    let trace = interp.trace_pixel([0, 0], &[]).unwrap();
    let test = trace
        .branches
        .iter()
        .rev()
        .find(|b| interp.module().text(b.span.unwrap()) == "turns < 2")
        .unwrap();
    // The test that ended the loop came out true, so forcing it false keeps it going.
    assert!(test.taken);
    let flip = Override::ForceBranch {
        condition: test.condition,
        taken: false,
    };
    let source = gpu_source(interp.module(), std::slice::from_ref(&flip)).unwrap();
    assert!(source.contains("while (texelscope_more(true))"), "{source}");
    // The interpreter ends the loop at the same turn, so both sides agree.
    let interpreted = interp.trace_pixel([0, 0], &[flip]).unwrap();
    assert!(
        interpreted
            .notes
            .iter()
            .any(|n| n.contains("stopped after")),
        "{:?}",
        interpreted.notes
    );
    let trace = interpreter(&source).trace_pixel([0, 0], &[]).unwrap();
    assert_eq!(trace.outputs, interpreted.outputs);
    let turns = trace
        .stores
        .iter()
        .rfind(|s| s.target == "turns")
        .map(|s| s.value.clone());
    // `acc` takes up to four turns of the first loop, which share the limit.
    let Some(Value::I32(n)) = turns else {
        panic!("turns was never stored")
    };
    assert!(n > LOOP_LIMIT as i32 - 8 && n <= LOOP_LIMIT as i32, "{n}");
}

/// The value a step had at a pixel, and what the probed shader outputs there.
fn probed(
    pick: impl Fn(&ShaderModule, &texelscope_interp::Step) -> bool,
    pixel: [u32; 2],
    overrides: &[Override],
) -> (Option<Value>, [f32; 4], String) {
    let interp = interpreter(SHADER);
    let module = interp.module();
    let expr: ExprRef = (0..16)
        .flat_map(|i| interp.trace_pixel([i % 4, i / 4], &[]).unwrap().steps)
        .find(|s| pick(module, s))
        .expect("no such step")
        .expr;
    let value = interp
        .trace_pixel(pixel, overrides)
        .unwrap()
        .steps
        .iter()
        .rev()
        .find(|s| s.expr == expr)
        .map(|s| s.value.clone());
    let probe = probe(module, "main", expr, overrides).unwrap_or_else(|e| panic!("{e}"));
    let out = interpreter(&probe.source)
        .shade_pixel(pixel, &[], None)
        .unwrap()
        .color()
        .unwrap();
    (value, out, probe.source)
}

fn spells(text: &'static str) -> impl Fn(&ShaderModule, &texelscope_interp::Step) -> bool {
    move |m, s| s.span.is_some_and(|span| m.text(span) == text)
}

fn f32s(value: Option<Value>) -> Vec<f32> {
    match value {
        Some(Value::F32(f)) => vec![f],
        Some(Value::I32(i)) => vec![i as f32],
        Some(Value::Bool(b)) => vec![f32::from(u8::from(b))],
        Some(Value::Vector(lanes)) => lanes.into_iter().flat_map(|l| f32s(Some(l))).collect(),
        other => panic!("{other:?} is not a number"),
    }
}

#[test]
fn probes_output_values_as_the_interpreter_traces_them() {
    let cases: [(&str, [u32; 2]); 6] = [
        ("ramp(v_uv.x, u_gain)", [2, 1]),
        ("u_tints[1] * (v_uv.y > 0.5 ? 2.0 : 0.5)", [1, 3]),
        ("acc += 0.25 * level", [3, 0]),
        ("level > 0.25", [3, 2]),
        ("i < u_steps", [1, 1]),
        ("t * k", [2, 2]),
    ];
    for (text, pixel) in cases {
        let (value, out, source) = probed(spells(text), pixel, &[]);
        let lanes = f32s(value);
        let mut expected = [0.0; 4];
        expected[..lanes.len()].copy_from_slice(&lanes);
        assert_eq!(out, expected, "{text} at {pixel:?}\n{source}");
        assert!(source.contains("uniform float u_gain;"), "{source}");
    }
}

#[test]
fn probes_mark_pixels_that_never_compute_the_value() {
    // The flag's test only runs where the first half of `&&` holds, and the sum only inside.
    let (value, out, _) = probed(spells("tint.x += 0.125"), [0, 0], &[]);
    assert_eq!(value, None);
    assert_eq!(out, [NOT_COMPUTED; 4]);
}

#[test]
fn probes_follow_the_value_through_what_ifs() {
    let interp = interpreter(SHADER);
    let level = interp
        .trace_pixel([1, 0], &[])
        .unwrap()
        .steps
        .into_iter()
        .find(|s| {
            s.span
                .is_some_and(|span| interp.module().text(span) == "ramp(v_uv.x, u_gain)")
        })
        .unwrap()
        .expr;
    let pin = [Override::Pin {
        expr: level,
        value: Value::F32(0.5),
    }];
    let (value, out, _) = probed(spells("tint * level"), [1, 0], &pin);
    let lanes = f32s(value);
    assert_eq!(out[..3], lanes[..]);
}

#[test]
fn probes_an_input_from_the_top_of_main() {
    let interp = interpreter(SHADER);
    let trace = interp.trace_pixel([1, 2], &[]).unwrap();
    let input = trace
        .steps
        .iter()
        .find(|s| matches!(&s.origin, Some(texelscope_interp::Origin::Input { name, .. }) if name == "v_uv"))
        .unwrap();
    let probe = probe(interp.module(), "main", input.expr, &[]).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        probe
            .source
            .contains("void texelscope_main() { texelscope_store(v_uv);"),
        "{}",
        probe.source
    );
    let out = interpreter(&probe.source)
        .shade_pixel([1, 2], &[], None)
        .unwrap()
        .color()
        .unwrap();
    assert_eq!(out, [1.0 / 3.0, 2.0 / 3.0, 0.0, 0.0]);
}

#[test]
fn probes_a_parameter_from_the_top_of_its_function() {
    let module = ShaderModule::from_glsl(SHADER).unwrap();
    let (f, _) = module
        .naga()
        .functions
        .iter()
        .find(|(_, f)| f.name.as_deref() == Some("ramp"))
        .unwrap();
    let k = ExprRef {
        function: texelscope_ir::FunctionRef::Function(f.index() as u32),
        expr: module.naga().functions[f]
            .expressions
            .iter()
            .position(|(_, e)| matches!(e, texelscope_ir::naga::Expression::FunctionArgument(1)))
            .unwrap() as u32,
    };
    let probe = probe(&module, "main", k, &[]).unwrap();
    assert!(
        probe
            .source
            .contains("float ramp(float t, float k) { texelscope_store(k);"),
        "{}",
        probe.source
    );
    let out = interpreter(&probe.source)
        .shade_pixel([0, 0], &[], None)
        .unwrap()
        .color()
        .unwrap();
    assert_eq!(out, [1.5, 0.0, 0.0, 0.0]);
}
