// A synthetic scalar field drawn through a stepped colormap.
// Values outside [visible_min, visible_max] are transparent.

struct Params {
    visible_min: f32,
    visible_max: f32,
    bin_width: f32,
    opacity: f32,
    palette: array<vec4<f32>, 4>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var field: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One triangle that covers the target, with uv at (0, 0) in the top-left corner.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

// Bilinear read done by hand, because r32float is not filterable everywhere.
fn sample_field(uv: vec2<f32>) -> f32 {
    let size = vec2<f32>(textureDimensions(field));
    let p = clamp(uv * size - 0.5, vec2<f32>(0.0), size - 1.0);
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let last = vec2<i32>(size) - 1;
    let v00 = textureLoad(field, i, 0).r;
    let v10 = textureLoad(field, min(i + vec2<i32>(1, 0), last), 0).r;
    let v01 = textureLoad(field, min(i + vec2<i32>(0, 1), last), 0).r;
    let v11 = textureLoad(field, min(i + vec2<i32>(1, 1), last), 0).r;
    return mix(mix(v00, v10, f.x), mix(v01, v11, f.x), f.y);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let value = sample_field(in.uv);
    if (value < params.visible_min || value > params.visible_max) {
        return vec4<f32>(0.0);
    }
    let bin = min(u32((value - params.visible_min) / params.bin_width), 3u);
    let color = params.palette[bin];
    return vec4<f32>(color.rgb, color.a * params.opacity);
}
