//! What an explanation tells the inspector about each value.

use std::sync::Arc;

use serde_json::json;
use texelscope_explain::explain_pixel;
use texelscope_interp::{CaptureData, Interpreter};
use texelscope_ir::ShaderModule;

#[test]
fn marks_values_that_come_only_from_settings() {
    let wgsl = "@group(0) @binding(0) var<uniform> n: vec4<f32>;
        @fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
            let scale = n.x * 2.0;
            let x = pos.x * scale;
            return vec4<f32>(x, 0.0, 0.0, 1.0);
        }";
    let explanation = explain_pixel(&interpreter(wgsl), [1, 0], &[]).unwrap();
    let setting = |name: &str| {
        explanation
            .links
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("no link named {name}"))
            .setting
    };
    assert!(setting("n.x"));
    assert!(setting("scale"));
    assert!(!setting("pos"));
    assert!(!setting("x"));
}

#[test]
fn marks_decisions_that_read_only_settings() {
    let wgsl = "@group(0) @binding(0) var<uniform> n: vec4<f32>;
        @fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
            var c = vec4<f32>(0.0, 0.0, 0.0, 1.0);
            if (n.x > 0.5) { c.x = 1.0; }
            if (pos.x > 2.0) { c.y = 1.0; }
            return c;
        }";
    let explanation = explain_pixel(&interpreter(wgsl), [3, 0], &[]).unwrap();
    let setting = |condition: &str| {
        explanation
            .decisions
            .iter()
            .find(|d| d.condition == condition)
            .unwrap_or_else(|| panic!("no decision {condition}"))
            .setting
    };
    assert!(setting("n.x > 0.5"));
    assert!(!setting("pos.x > 2.0"));
}

/// A 4 by 4 target for a shader whose uniform `n` is (1, 0, 0, 0).
fn interpreter(wgsl: &str) -> Interpreter {
    let capture = json!({
        "version": 0,
        "shader": { "language": "wgsl", "source": { "inline": wgsl }, "entryPoint": "fs" },
        "target": { "width": 4, "height": 4, "format": "rgba32float" },
        "bindings": [{ "group": 0, "binding": 0, "resource": {
            "kind": "uniform", "data": { "values": { "n": [1.0, 0.0, 0.0, 0.0] } } } }],
    });
    let capture = texelscope_capture::parse(&capture.to_string()).unwrap();
    let module = ShaderModule::from_wgsl(wgsl).unwrap();
    let data = CaptureData::new(capture, None).unwrap();
    Interpreter::new(Arc::new(module), Arc::new(data)).unwrap()
}

/// The sentences of why one pixel has its color.
fn why(wgsl: &str, pixel: [u32; 2]) -> Vec<String> {
    let explanation = explain_pixel(&interpreter(wgsl), pixel, &[]).unwrap();
    explanation.why.into_iter().map(|r| r.text).collect()
}

fn says(why: &[String], words: &str) {
    assert!(
        why.iter().any(|w| w.contains(words)),
        "{words:?} is not in {why:#?}"
    );
}

#[test]
fn says_when_a_clamp_holds_the_color() {
    let why = why(
        "@group(0) @binding(0) var<uniform> n: vec4<f32>;
        @fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
            let level = pos.x * n.x - 2.0;
            let t = clamp(level, 0.0, 1.0);
            return vec4<f32>(t, t, t, 1.0);
        }",
        [0, 0],
    );
    says(&why, "The color comes from `t`, on line 4.");
    says(
        &why,
        "`t` is held at 0 by `clamp` on line 4, because the value going in is -1.5.",
    );
    says(&why, "So `level` (-1.5) does not change the color here.");
}

#[test]
fn says_when_the_pixel_returned_early() {
    let why = why(
        "@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
            if (pos.x < 2.0) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
            return vec4<f32>(pos.x, 0.0, 0.0, 1.0);
        }",
        [0, 0],
    );
    says(
        &why,
        "It returned early, on line 2, because `pos.x < 2.0` was true (1.5 from flipping).",
    );
}

#[test]
fn says_which_skipped_branch_left_a_variable_alone() {
    let why = why(
        "@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
            var c = vec4<f32>(0.2, 0.2, 0.2, 1.0);
            if (pos.x > 2.0) { c = vec4<f32>(pos.x, 0.0, 0.0, 1.0); }
            return c;
        }",
        [0, 0],
    );
    says(
        &why,
        "`pos.x > 2.0` was false, so the code inside it that changes `c` didn't run, and `c` kept its initial value.",
    );
}

#[test]
fn says_how_far_a_blend_moved_the_color() {
    let why = why(
        "@group(0) @binding(0) var<uniform> n: vec4<f32>;
        @fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
            let base = vec4<f32>(pos.x / 4.0, 0.0, 0.0, 1.0);
            return mix(base, vec4<f32>(0.0, 0.0, 1.0, 1.0), n.x * 0.5);
        }",
        [1, 0],
    );
    says(
        &why,
        "On line 4 the color is blended toward `vec4<f32>(0.0, 0.0, 1.0, 1.0)` by 0.5.",
    );
    says(&why, "The color comes from the fragment input `pos`.");
}

/// The sentences of why one pixel of a GLSL ES shader has its color, on a 4 by 1 target whose
/// `v_uv` runs (0.5, 0.5), (0.6, 0.5), ... and whose `u_inner` is 0.25.
fn why_glsl(glsl: &str, pixel: [u32; 2]) -> Vec<String> {
    let (mut frag, mut uv) = (Vec::new(), Vec::new());
    for x in 0..4 {
        frag.extend([x as f32 + 0.5, 0.5, 0.5, 1.0]);
        uv.extend([0.5 + x as f32 * 0.1, 0.5]);
    }
    let packed = |values: &[f32]| {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        texelscope_capture::encode_base64(&bytes)
    };
    let capture = json!({
        "version": 0,
        "shader": { "language": "glsl", "source": { "inline": glsl }, "entryPoint": "main" },
        "target": { "width": 4, "height": 1, "format": "rgba8unorm", "yUp": true },
        "inputs": {
            "kind": "window", "rect": { "x": 0, "y": 0, "width": 4, "height": 1 },
            "fragCoord": packed(&frag),
            "varyings": [{ "name": "v_uv", "components": 2, "data": packed(&uv) }]
        },
        "bindings": [
            { "group": 0, "binding": 0, "name": "u_inner", "resource": {
                "kind": "uniform", "data": { "values": { "u_inner": 0.25 } } } },
            { "group": 0, "binding": 0, "name": "u_ring", "resource": {
                "kind": "uniform", "data": { "values": { "u_ring": [1.0, 0.6, 0.1] } } } }
        ],
    });
    let capture = texelscope_capture::parse(&capture.to_string()).unwrap();
    let module = ShaderModule::from_glsl(glsl).unwrap();
    let data = CaptureData::new(capture, None).unwrap();
    let shader = Interpreter::new(Arc::new(module), Arc::new(data)).unwrap();
    let explanation = explain_pixel(&shader, pixel, &[]).unwrap();
    explanation.why.into_iter().map(|r| r.text).collect()
}

#[test]
fn says_which_side_a_mix_by_a_step_kept() {
    let glsl = "#version 300 es
precision highp float;
in vec2 v_uv;
uniform float u_inner;
uniform vec3 u_ring;
out vec4 color;
void main() {
    float d = length(v_uv - 0.5);
    float ring = step(u_inner, d) * step(d, u_inner + 0.1);
    color = vec4(mix(vec3(0.1, 0.2, 0.6), u_ring, ring), 1.0);
}
";
    let why = why_glsl(glsl, [0, 0]);
    says(
        &why,
        "The color is the constant `vec3(0.1, 0.2, 0.6)` on line 10.",
    );
    says(
        &why,
        "`mix` on line 10 keeps only `vec3(0.1, 0.2, 0.6)`, because `ring` is 0.",
    );
    says(
        &why,
        "`step(u_inner, d)` on line 9 is 0, because `d` (0) is below `u_inner` (0.25).",
    );
    // Past the inner edge, the ring's color wins instead.
    let why = why_glsl(glsl, [3, 0]);
    says(
        &why,
        "The color comes from the setting `u_ring`, read on line 10.",
    );
    says(
        &why,
        "`mix` on line 10 keeps only `u_ring`, because `ring` is 1.",
    );
}

/// The sentences that say what sets two pixels apart.
fn compared(wgsl: &str, a: [u32; 2], b: [u32; 2]) -> Vec<String> {
    let comparison = texelscope_explain::compare_pixels(&interpreter(wgsl), a, b, &[]).unwrap();
    comparison.summary.into_iter().map(|r| r.text).collect()
}

#[test]
fn finds_where_two_pixels_part_ways() {
    let summary = compared(
        "@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
            var c = vec4<f32>(0.0, 0.0, 0.0, 1.0);
            for (var i = 0; i < i32(pos.y) + 1; i++) { c.y += 0.1; }
            if (pos.x > 2.0) { c.x = 1.0; }
            return c;
        }",
        [0, 0],
        [3, 0],
    );
    says(
        &summary,
        "They part ways on line 4, where `pos.x > 2.0` was false for A and true for B.",
    );
}

#[test]
fn says_when_a_clamp_holds_one_pixel_but_not_the_other() {
    let summary = compared(
        "@group(0) @binding(0) var<uniform> n: vec4<f32>;
        @fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
            let level = pos.x * n.x - 2.0;
            let t = clamp(level, 0.0, 1.0);
            return vec4<f32>(t, t, t, 1.0);
        }",
        [0, 0],
        [2, 0],
    );
    says(&summary, "Both took the same path to the color.");
    says(
        &summary,
        "In A, `t` is held at 0 by `clamp`, so the values before it don't change A's color. In B, `t` is 0.5.",
    );
}
