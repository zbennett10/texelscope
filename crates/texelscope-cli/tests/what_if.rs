//! Every pin and flip the inspector offers must look the same on the GPU, which runs the
//! patched source, as in the interpreter, which applies the override itself.

use std::sync::Arc;

use serde_json::json;
use texelscope_explain::{LinkKind, explain_pixel};
use texelscope_interp::{CaptureData, Interpreter};
use texelscope_ir::{Override, ShaderModule, Value};
use texelscope_rewrite::patch_source;

const SHADER: &str = "@group(0) @binding(0) var<uniform> n: vec4<f32>;

fn ramp(t: f32, k: f32) -> f32 {
    return clamp(t * k, 0.0, 1.0);
}

fn band_of(x: f32) -> i32 {
    return i32(floor(clamp(x, 0.0, 3.0)));
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let level = ramp(uv.x, n.x);
    let up = level >= 0.5;
    let band = band_of(uv.y * 4.0);
    var tint = select(0.25, 0.75, up);
    if (up && band > 1) { tint += 0.1; }
    if (!up && pos.x > 1.0) { tint -= 0.1; }
    return vec4<f32>(level, tint, f32(band) / 4.0, 1.0);
}";

fn data() -> Arc<CaptureData> {
    let capture = json!({
        "version": 0,
        "shader": { "language": "wgsl", "source": { "inline": SHADER }, "entryPoint": "fs" },
        "target": { "width": 4, "height": 4, "format": "rgba32float" },
        "inputs": {
            "kind": "fullscreen",
            "locations": [{ "location": 0, "corners": [[0, 0], [1, 0], [0, 1], [1, 1]] }]
        },
        "bindings": [{ "group": 0, "binding": 0, "resource": {
            "kind": "uniform", "data": { "values": { "n": [2.0, 0.0, 0.0, 0.0] } } } }],
    });
    let capture = texelscope_capture::parse(&capture.to_string()).unwrap();
    Arc::new(CaptureData::new(capture, None).unwrap())
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
fn the_patched_source_agrees_with_the_interpreter() {
    let module = Arc::new(ShaderModule::from_wgsl(SHADER).unwrap());
    let data = data();
    let interp = Interpreter::new(module.clone(), data.clone()).unwrap();
    let mut checked = 0;
    for pixel in (0..16).map(|i| [i % 4, i / 4]) {
        let explanation = explain_pixel(&interp, pixel, &[]).unwrap();
        let pins = explanation.links.iter().filter_map(|link| {
            let expr = link.expr.filter(|_| link.kind != LinkKind::Var)?;
            let value = nudged(&link.value)?;
            Some((link.name.clone(), Override::Pin { expr, value }))
        });
        let flips = explanation.decisions.iter().filter_map(|d| {
            let condition = d.expr?;
            let flip = Override::ForceBranch {
                condition,
                taken: !d.taken,
            };
            Some((d.condition.clone(), flip))
        });
        for (label, o) in pins.chain(flips).collect::<Vec<_>>() {
            let what = format!("{label} at {pixel:?}");
            let patched = patch_source(&module, std::slice::from_ref(&o))
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            let patched = ShaderModule::from_wgsl(patched.as_str())
                .unwrap_or_else(|e| panic!("{what} patches to invalid WGSL\n{e}\n{patched}"));
            let gpu = Interpreter::new(Arc::new(patched), data.clone()).unwrap();
            let expected = interp.shade_pixel(pixel, &[o], None).unwrap().color();
            let shown = gpu.shade_pixel(pixel, &[], None).unwrap().color();
            assert_eq!(shown, expected, "{what}");
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} overrides were checked");
}
