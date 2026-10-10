//! Small shaders that pin down the interpreter's semantics.

use std::sync::Arc;

use serde_json::json;
use texelscope_capture::Rect;
use texelscope_interp::{BranchKind, CaptureData, Interpreter, Origin, Perturbation};
use texelscope_ir::{ExprRef, FunctionRef, Override, ShaderModule, Value};

/// A 4x4 target with the given bindings and the shader's `fs` entry point.
fn interp(wgsl: &str, bindings: serde_json::Value) -> Interpreter {
    let capture = json!({
        "version": 0,
        "shader": { "language": "wgsl", "source": { "inline": wgsl }, "entryPoint": "fs" },
        "target": { "width": 4, "height": 4, "format": "rgba32float" },
        "inputs": {
            "kind": "fullscreen",
            "locations": [{ "location": 0, "corners": [[0, 0], [1, 0], [0, 1], [1, 1]] }]
        },
        "bindings": bindings,
    });
    let capture = texelscope_capture::parse(&capture.to_string()).unwrap();
    let module = ShaderModule::from_wgsl(wgsl).unwrap_or_else(|e| panic!("{e}"));
    let data = CaptureData::new(capture, None).unwrap();
    Interpreter::new(Arc::new(module), Arc::new(data)).unwrap()
}

fn color(interp: &Interpreter, pixel: [u32; 2]) -> [f32; 4] {
    interp
        .shade_pixel(pixel, &[], None)
        .unwrap()
        .color()
        .unwrap()
}

/// The last value a trace gave the `let` called `name`.
fn named(trace: &texelscope_interp::Trace, name: &str) -> Value {
    trace
        .steps
        .iter()
        .rev()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no step named {name}"))
        .value
        .clone()
}

fn approx(a: [f32; 4], b: [f32; 4]) {
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() < 1e-5, "{a:?} is not {b:?}");
    }
}

/// A uniform `n` read at run time, so naga cannot fold the arithmetic away.
fn numbers(ty: &str, values: serde_json::Value) -> serde_json::Value {
    let _ = ty;
    json!([{ "group": 0, "binding": 0, "resource": { "kind": "uniform", "data": { "values": { "n": values } } } }])
}

#[test]
fn follows_wgsl_integer_rules() {
    let shader = interp(
        "@group(0) @binding(0) var<uniform> n: vec4<i32>;
        @fragment fn fs() -> @location(0) vec4<f32> {
            let wrapped = n.x + 1i;
            let by_zero = n.y / n.z;
            let rem_zero = n.y % n.z;
            let negative_rem = -n.y % 3i;
            let shifted = 1u << u32(n.w);
            return vec4<f32>(f32(wrapped), f32(by_zero), f32(rem_zero), f32(negative_rem + i32(shifted)));
        }",
        numbers("vec4<i32>", json!([2147483647, 7, 0, 33])),
    );
    approx(color(&shader, [0, 0]), [-2147483648.0, 7.0, 0.0, 1.0]);
}

#[test]
fn computes_float_builtins_like_the_spec() {
    let shader = interp(
        "@group(0) @binding(0) var<uniform> n: vec4<f32>;
        @fragment fn fs() -> @location(0) vec4<f32> {
            let r = round(n.x) + round(n.x + 1.0);
            let m = -n.y % 2.0;
            let s = smoothstep(0.0, 4.0, n.z);
            let a = degrees(atan2(n.z, n.w));
            return vec4<f32>(r, m, s, a);
        }",
        numbers("vec4<f32>", json!([2.5, 7.5, 1.0, 0.0])),
    );
    approx(color(&shader, [0, 0]), [6.0, -1.5, 0.15625, 90.0]);
}

#[test]
fn interpolates_fullscreen_inputs_at_pixel_centers() {
    let shader = interp(
        "@fragment fn fs(@builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
            return vec4<f32>(p.xy, uv);
        }",
        json!([]),
    );
    approx(color(&shader, [1, 2]), [1.5, 2.5, 0.375, 0.625]);
}

#[test]
fn runs_loops_switches_and_calls_through_pointers() {
    let shader = interp(
        "fn bump(p: ptr<function, i32>, by: i32) { *p = *p + by; }
        @fragment fn fs() -> @location(0) vec4<f32> {
            var total = 0i;
            for (var i = 0i; i < 10i; i++) {
                if (i == 2i) { continue; }
                if (i == 6i) { break; }
                bump(&total, i);
            }
            var picked = 0.0;
            switch (total) {
                case 1i, 2i: { picked = 1.0; }
                case 13i: { picked = 2.0; }
                default: { picked = 3.0; }
            }
            var n = 0i;
            loop {
                n += 1i;
                continuing { break if n >= 4i; }
            }
            return vec4<f32>(f32(total), picked, f32(n), 0.0);
        }",
        json!([]),
    );
    approx(color(&shader, [0, 0]), [13.0, 2.0, 4.0, 0.0]);
}

#[test]
fn reads_uniforms_from_values_and_from_bytes() {
    let wgsl = "struct U { scale: f32, offset: vec3<f32>, tint: vec4<f32> }
        @group(0) @binding(0) var<uniform> u: U;
        @fragment fn fs() -> @location(0) vec4<f32> {
            return vec4<f32>(u.offset * u.scale, u.tint.w);
        }";
    let by_values = interp(
        wgsl,
        json!([{ "group": 0, "binding": 0, "resource": { "kind": "uniform", "data": { "values": {
            "scale": 2.0, "offset": [1.0, 2.0, 3.0], "tint": [0.0, 0.0, 0.0, 0.5]
        } } } }]),
    );
    approx(color(&by_values, [0, 0]), [2.0, 4.0, 6.0, 0.5]);

    // offset is a vec3 aligned to 16 bytes, so 12 bytes of padding follow scale.
    let mut bytes = Vec::new();
    for v in [
        2.0f32, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.5,
    ] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let base64 = texelscope_capture::encode_base64(&bytes);
    let by_bytes = interp(
        wgsl,
        json!([{ "group": 0, "binding": 0, "resource": { "kind": "uniform", "data": { "base64": base64 } } }]),
    );
    approx(color(&by_bytes, [0, 0]), [2.0, 4.0, 6.0, 0.5]);
    // Member values pack back into the same bytes, for uploading to a GPU.
    assert_eq!(by_values.buffer_bytes(0, 0).unwrap(), bytes);
}

fn ramp_texture() -> serde_json::Value {
    // A 4x1 r32float row of 0, 10, 20, 30.
    json!({ "group": 0, "binding": 0, "resource": {
        "kind": "texture", "dimension": "2d", "format": "r32float", "size": [4, 1, 1],
        "data": { "inline": [0.0, 10.0, 20.0, 30.0] }
    } })
}

#[test]
fn clamps_texture_loads_and_says_so() {
    let shader = interp(
        "@group(0) @binding(0) var t: texture_2d<f32>;
        @fragment fn fs() -> @location(0) vec4<f32> {
            let inside = textureLoad(t, vec2<i32>(2, 0), 0).r;
            let outside = textureLoad(t, vec2<i32>(9, 0), 0).r;
            return vec4<f32>(inside, outside, 0.0, 1.0);
        }",
        json!([ramp_texture()]),
    );
    let trace = shader.trace_pixel([0, 0], &[]).unwrap();
    assert_eq!(named(&trace, "inside"), Value::F32(20.0));
    assert_eq!(named(&trace, "outside"), Value::F32(30.0));
    let clamped = trace
        .steps
        .iter()
        .find(|s| s.note.is_some())
        .expect("a clamped load is noted");
    assert!(
        matches!(&clamped.origin, Some(Origin::Texels { texels, .. }) if texels == &[[3, 0, 0]])
    );
}

#[test]
fn filters_samples_between_texels() {
    let mut bindings = vec![ramp_texture()];
    bindings.push(json!({ "group": 0, "binding": 1, "resource": {
        "kind": "sampler", "magFilter": "linear", "minFilter": "linear"
    } }));
    let shader = interp(
        "@group(0) @binding(0) var t: texture_2d<f32>;
        @group(0) @binding(1) var s: sampler;
        @fragment fn fs() -> @location(0) vec4<f32> {
            // Halfway between the centers of texels 1 and 2.
            return vec4<f32>(textureSampleLevel(t, s, vec2<f32>(0.5, 0.5), 0.0).r, 0.0, 0.0, 1.0);
        }",
        json!(bindings),
    );
    approx(color(&shader, [0, 0]), [15.0, 0.0, 0.0, 1.0]);
}

#[test]
fn takes_derivatives_across_the_quad() {
    let shader = interp(
        "@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
            let f = p.x * p.x + 3.0 * p.y;
            return vec4<f32>(dpdx(f), dpdy(f), fwidth(f), 0.0);
        }",
        json!([]),
    );
    assert!(shader.uses_quads());
    // At pixel (2, 1) the quad's columns sit at x = 2.5 and 3.5.
    approx(color(&shader, [2, 1]), [6.0, 3.0, 9.0, 0.0]);
    approx(color(&shader, [3, 0]), [6.0, 3.0, 9.0, 0.0]);
}

#[test]
fn reports_how_close_a_short_circuit_came_to_flipping() {
    let shader = interp(
        "@fragment fn fs(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
            if (uv.x < 0.1 || uv.y > 0.5) { return vec4<f32>(1.0); }
            return vec4<f32>(0.0);
        }",
        json!([]),
    );
    // uv is (0.375, 0.375): 0.275 above the first bound and 0.125 below the second.
    let trace = shader.trace_pixel([1, 1], &[]).unwrap();
    let decision = trace
        .branches
        .iter()
        .find(|b| b.kind == BranchKind::If)
        .unwrap();
    assert!(!decision.taken);
    assert!(
        (decision.margin.unwrap() - 0.125).abs() < 1e-6,
        "{decision:?}"
    );
    assert_eq!(
        trace
            .branches
            .iter()
            .filter(|b| b.kind == BranchKind::If)
            .count(),
        1
    );
}

#[test]
fn applies_pins_and_forced_branches() {
    let wgsl = "@fragment fn fs(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
            let level = uv.x * 2.0;
            if (level > 1.5) { return vec4<f32>(1.0, 0.0, 0.0, 1.0); }
            return vec4<f32>(level, 0.0, 0.0, 1.0);
        }";
    let shader = interp(wgsl, json!([]));
    let trace = shader.trace_pixel([0, 0], &[]).unwrap();
    let level = trace
        .steps
        .iter()
        .find(|s| s.name.as_deref() == Some("level"))
        .unwrap()
        .expr;
    let condition = trace.branches[0].condition;

    let pinned = [Override::Pin {
        expr: level,
        value: Value::F32(0.75),
    }];
    approx(
        shader
            .shade_pixel([0, 0], &pinned, None)
            .unwrap()
            .color()
            .unwrap(),
        [0.75, 0.0, 0.0, 1.0],
    );

    let forced = [Override::ForceBranch {
        condition,
        taken: true,
    }];
    let trace = shader.trace_pixel([0, 0], &forced).unwrap();
    assert!(trace.branches[0].forced);
    approx(
        texelscope_interp::color_of(trace.color().unwrap()),
        [1.0, 0.0, 0.0, 1.0],
    );
}

#[test]
fn links_values_to_the_steps_they_came_from() {
    let shader = interp(
        "@fragment fn fs(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
            var acc = 0.0;
            acc = acc + uv.x;
            let doubled = acc * 2.0;
            return vec4<f32>(doubled);
        }",
        json!([]),
    );
    let trace = shader.trace_pixel([0, 0], &[]).unwrap();
    let doubled = trace
        .steps
        .iter()
        .position(|s| s.name.as_deref() == Some("doubled"))
        .unwrap();
    // doubled reads acc, whose load links back to the step that stored it.
    let load = trace.steps[doubled].inputs[0] as usize;
    let stored = trace.steps[load].inputs[0] as usize;
    assert_eq!(trace.steps[stored].value, Value::F32(0.125));
    assert_eq!(
        trace
            .stores
            .iter()
            .map(|s| s.target.as_str())
            .collect::<Vec<_>>(),
        ["acc"]
    );
}

#[test]
fn nudges_inputs_for_sensitivities() {
    let shader = interp(
        "@group(0) @binding(0) var<uniform> gain: f32;
        @fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(gain * 3.0); }",
        json!([{ "group": 0, "binding": 0, "resource": { "kind": "uniform", "data": { "values": { "gain": 2.0 } } } }]),
    );
    let nudge = Perturbation::Buffer {
        group: 0,
        binding: 0,
        indices: vec![],
        delta: 0.5,
    };
    let shaded = shader.shade_pixel([0, 0], &[], Some(&nudge)).unwrap();
    approx(shaded.color().unwrap(), [7.5, 7.5, 7.5, 7.5]);
}

#[test]
fn inspects_regions_as_layers() {
    let shader = interp(
        "@fragment fn fs(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
            let band = floor(uv.x * 4.0);
            if (band > 1.0) { return vec4<f32>(1.0); }
            return vec4<f32>(band / 4.0, 0.0, 0.0, 1.0);
        }",
        json!([]),
    );
    let region = shader
        .inspect_region(
            Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            &[],
            None,
        )
        .unwrap();
    let band = region.layers.iter().find(|l| l.name == "band").unwrap();
    let firsts: Vec<f32> = band.data.chunks(4).map(|px| px[0]).collect();
    assert_eq!(firsts, [0.0, 1.0, 2.0, 3.0, 0.0, 1.0, 2.0, 3.0]);
    assert_eq!((band.min[0], band.max[0]), (0.0, 3.0));
    let branch = &region.branches[0];
    assert_eq!(branch.data, [1, 1, 2, 2, 1, 1, 2, 2]);
    assert_eq!(region.colors[..4], [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn rejects_overrides_for_expressions_that_do_not_exist() {
    let shader = interp(
        "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }",
        json!([]),
    );
    let bogus = Override::Pin {
        expr: ExprRef {
            function: FunctionRef::EntryPoint(0),
            expr: 999,
        },
        value: Value::F32(1.0),
    };
    assert!(shader.trace_pixel([0, 0], &[bogus]).is_err());
}
