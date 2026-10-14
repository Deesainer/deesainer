
struct Blit {
    srgb_target: u32,
    display_p3: u32,
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

fn encode(c: f32) -> f32 {
    if c <= 0.0031308 {
        return 12.92 * c;
    }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

fn to_p3(c: vec3<f32>) -> vec3<f32> {
    let m = mat3x3<f32>(
        vec3<f32>(0.8224619687, 0.1775380480, 0.0000000000),
        vec3<f32>(0.0331941989, 0.9668058063, 0.0000000000),
        vec3<f32>(0.0170826297, 0.0723974419, 0.9105194739),
    );
    return m * c;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(canvas, canvas_sampler, in.uv);
    if (params.srgb_target == 0u && params.display_p3 == 0u) || c.a <= 0.0 {
        return c;
    }
    let rgb = c.rgb / c.a;
    var lin = vec3<f32>(decode(rgb.r), decode(rgb.g), decode(rgb.b));
    if params.display_p3 != 0u {
        lin = to_p3(lin);
    }
    var out = lin;
    if params.srgb_target == 0u {
        out = vec3<f32>(encode(out.r), encode(out.g), encode(out.b));
    }
    return vec4<f32>(out * c.a, c.a);
}
