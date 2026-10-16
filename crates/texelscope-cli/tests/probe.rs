//! A probed shader outputs, at every pixel, the value the original computes there.

use std::sync::Arc;

use serde_json::json;
use texelscope_interp::{CaptureData, Interpreter};
use texelscope_ir::{ExprRef, Override, ShaderModule, Value};
use texelscope_rewrite::{NOT_COMPUTED, probe};

const SHADER: &str = "@group(0) @binding(0) var<uniform> n: vec4<f32>;

fn spread(x: f32) -> vec2<f32> {
    let pair = vec2<f32>(x, x * n.x);
    return pair;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let level = pos.x * 0.25;
    let band = i32(pos.y);
    let both = spread(level);
    var c = vec4<f32>(both, 0.0, 1.0);
    if (pos.x > 2.0) {
        let edge = level * 2.0;
        c.z = edge;
    }
    return c;
}";

fn data(wgsl: &str) -> Arc<CaptureData> {
    let capture = json!({
        "version": 0,
        "shader": { "language": "wgsl", "source": { "inline": wgsl }, "entryPoint": "fs" },
        "target": { "width": 4, "height": 4, "format": "rgba32float" },
        "bindings": [{ "group": 0, "binding": 0, "resource": {
            "kind": "uniform", "data": { "values": { "n": [3.0, 0.0, 0.0, 0.0] } } } }],
    });
    Arc::new(
        CaptureData::new(
            texelscope_capture::parse(&capture.to_string()).unwrap(),
            None,
        )
        .unwrap(),
    )
}

/// The value a named step had at a pixel, and what the probed shader outputs there.
fn probed(name: &str, pixel: [u32; 2], overrides: &[Override]) -> (Option<Value>, [f32; 4]) {
    let module = Arc::new(ShaderModule::from_wgsl(SHADER).unwrap());
    let interp = Interpreter::new(module.clone(), data(SHADER)).unwrap();
    let all = interp.trace_pixel([0, 0], &[]).unwrap();
    let expr: ExprRef = all
        .steps
        .iter()
        .chain(interp.trace_pixel([3, 3], &[]).unwrap().steps.iter())
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no step named {name}"))
        .expr;
    let here = interp.trace_pixel(pixel, overrides).unwrap();
    let value = here
        .steps
        .iter()
        .rev()
        .find(|s| s.expr == expr)
        .map(|s| s.value.clone());
    let probe = probe(&module, "fs", expr, overrides).unwrap_or_else(|e| panic!("{e}"));
    let rewritten = ShaderModule::from_wgsl(probe.source.as_str())
        .unwrap_or_else(|e| panic!("{e}\n{}", probe.source));
    let gpu = Interpreter::new(Arc::new(rewritten), data(SHADER)).unwrap();
    let out = gpu.shade_pixel(pixel, &[], None).unwrap().color().unwrap();
    (value, out)
}

#[test]
fn outputs_a_let_of_the_entry_point() {
    let (value, out) = probed("level", [1, 2], &[]);
    assert_eq!(value, Some(Value::F32(0.375)));
    assert_eq!(out, [0.375, 0.0, 0.0, 0.0]);
}

#[test]
fn outputs_a_value_from_a_helper_and_converts_integers() {
    assert_eq!(probed("pair", [1, 0], &[]).1, [0.375, 1.125, 0.0, 0.0]);
    assert_eq!(probed("band", [0, 2], &[]).1, [2.0, 0.0, 0.0, 0.0]);
    assert_eq!(probed("pos", [1, 2], &[]).1, [1.5, 2.5, 0.0, 1.0]);
}

#[test]
fn marks_pixels_that_never_compute_the_value() {
    let (_, outside) = probed("edge", [0, 0], &[]);
    assert_eq!(outside, [NOT_COMPUTED; 4]);
    let (value, inside) = probed("edge", [3, 0], &[]);
    assert_eq!(value, Some(Value::F32(1.75)));
    assert_eq!(inside, [1.75, 0.0, 0.0, 0.0]);
}

#[test]
fn follows_the_value_through_what_ifs() {
    let module = ShaderModule::from_wgsl(SHADER).unwrap();
    let interp = Interpreter::new(Arc::new(module), data(SHADER)).unwrap();
    let level = interp
        .trace_pixel([1, 0], &[])
        .unwrap()
        .steps
        .into_iter()
        .find(|s| s.name.as_deref() == Some("level"))
        .unwrap()
        .expr;
    let pin = [Override::Pin {
        expr: level,
        value: Value::F32(5.0),
    }];
    // `edge` is 2 * level, and the pin moves every source offset after `level`.
    let (value, out) = probed("edge", [3, 0], &pin);
    assert_eq!(value, Some(Value::F32(10.0)));
    assert_eq!(out, [10.0, 0.0, 0.0, 0.0]);
}
