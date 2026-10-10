// A synthetic 3D scalar field, rendered by marching a ray through the unit cube.

struct Camera {
    inverse_view_projection: mat4x4<f32>,
    position: vec3<f32>,
    step_count: u32,
}

struct Transfer {
    density_scale: f32,
    threshold: f32,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> transfer: Transfer;
@group(0) @binding(2) var volume: texture_3d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    let ndc = corner * 2.0 - 1.0;
    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.ndc = ndc;
    return out;
}

// Nearest read of the volume at a point in the unit cube.
fn density(p: vec3<f32>) -> f32 {
    let size = vec3<i32>(textureDimensions(volume));
    let i = clamp(vec3<i32>(p * vec3<f32>(size)), vec3<i32>(0), size - 1);
    return textureLoad(volume, i, 0).r;
}

// Entry and exit distances of a ray through the unit cube.
fn hit_box(origin: vec3<f32>, dir: vec3<f32>) -> vec2<f32> {
    let inv = 1.0 / dir;
    let t0 = (vec3<f32>(0.0) - origin) * inv;
    let t1 = (vec3<f32>(1.0) - origin) * inv;
    let near = max(max(min(t0.x, t1.x), min(t0.y, t1.y)), min(t0.z, t1.z));
    let far = min(min(max(t0.x, t1.x), max(t0.y, t1.y)), max(t0.z, t1.z));
    return vec2<f32>(max(near, 0.0), far);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let clip = camera.inverse_view_projection * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(clip.xyz / clip.w - camera.position);
    let span = hit_box(camera.position, dir);
    if (span.x >= span.y) {
        return vec4<f32>(0.0);
    }
    let step_size = (span.y - span.x) / f32(camera.step_count);
    var color = vec3<f32>(0.0);
    var alpha = 0.0;
    for (var i = 0u; i < camera.step_count; i++) {
        let p = camera.position + dir * (span.x + (f32(i) + 0.5) * step_size);
        let d = density(p);
        if (d > transfer.threshold) {
            let a = 1.0 - exp(-d * transfer.density_scale * step_size);
            color += (1.0 - alpha) * a * vec3<f32>(d, d * d, 1.0 - d);
            alpha += (1.0 - alpha) * a;
        }
        if (alpha > 0.99) {
            break;
        }
    }
    return vec4<f32>(color, alpha);
}
