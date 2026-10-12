
struct Blit {
    srgb_target: u32,
    _pad: u32,
    uv_scale: vec2<f32>,
};

@group(0) @binding(0) var<uniform> params: Blit;
@group(0) @binding(1) var canvas: texture_2d<f32>;
@group(0) @binding(2) var canvas_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[index];
    var out: VertexOut;
    out.position = vec4<f32>(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0, 0.0, 1.0);
    out.uv = c * params.uv_scale;
    return out;
}

fn decode(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(canvas, canvas_sampler, in.uv);
    if params.srgb_target == 0u || c.a <= 0.0 {
        return c;
    }
    let rgb = c.rgb / c.a;
    return vec4<f32>(vec3<f32>(decode(rgb.r), decode(rgb.g), decode(rgb.b)) * c.a, c.a);
}
