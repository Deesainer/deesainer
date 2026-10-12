
struct Filter {
    size: vec2<f32>,
    size_used: vec2<f32>,
    tint: vec4<f32>,
    sigma: f32,
    axis: u32,
    invert: u32,
    tinted: u32,
    origin: vec2<f32>,
};

@group(0) @binding(0) var<uniform> params: Filter;
@group(0) @binding(1) var source: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let px = params.origin + corners[index] * params.size_used;
    return vec4<f32>(px.x / params.size.x * 2.0 - 1.0, 1.0 - px.y / params.size.y * 2.0, 0.0, 1.0);
}

fn shade(c: vec4<f32>) -> vec4<f32> {
    var a = c.a;
    if params.invert != 0u {
        a = 1.0 - a;
    }
    if params.tinted == 0u {
        if params.invert != 0u {
            return vec4<f32>(a);
        }
        return c;
    }
    let t = params.tint;
    return vec4<f32>(t.rgb * t.a * a, t.a * a);
}

@fragment
fn fs_tint(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return shade(textureLoad(source, vec2<i32>(floor(position.xy)), 0));
}

@fragment
fn fs_blur(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(floor(position.xy));
    let lo = vec2<i32>(params.origin);
    let hi = lo + vec2<i32>(params.size_used);
    let sigma = max(params.sigma, 0.01);
    let reach = ceil(sigma * 3.0);
    let step = max(1.0, ceil(reach / 64.0));
    let taps = i32(ceil(reach / step));
    var dir = vec2<i32>(1, 0);
    if params.axis == 1u {
        dir = vec2<i32>(0, 1);
    }
    var sum = vec4<f32>(0.0);
    var weights = 0.0;
    for (var k = -taps; k <= taps; k = k + 1) {
        let d = f32(k) * step;
        let w = exp(-(d * d) / (2.0 * sigma * sigma));
        weights = weights + w;
        let p = at + dir * i32(d);
        if p.x >= lo.x && p.y >= lo.y && p.x < hi.x && p.y < hi.y {
            sum = sum + shade(textureLoad(source, p, 0)) * w;
        }
    }
    return sum / weights;
}
