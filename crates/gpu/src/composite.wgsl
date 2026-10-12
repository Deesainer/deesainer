
struct Composite {
    rect: vec4<f32>,
    size: vec2<f32>,
    opacity: f32,
    mode: u32,
    src: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> params: Composite;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var backdrop: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[index];
    let px = mix(params.rect.xy, params.rect.zw, c);
    return vec4<f32>(px.x / params.size.x * 2.0 - 1.0, 1.0 - px.y / params.size.y * 2.0, 0.0, 1.0);
}

fn texel(position: vec4<f32>) -> vec2<i32> {
    return vec2<i32>(floor(position.xy - params.rect.xy + params.src));
}

@fragment
fn fs_over(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, texel(position), 0) * params.opacity;
}

fn lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.3, 0.59, 0.11));
}

fn clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = lum(c);
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    var out = c;
    if n < 0.0 {
        out = l + (out - l) * l / max(l - n, 1e-6);
    }
    if x > 1.0 {
        out = l + (out - l) * (1.0 - l) / max(x - l, 1e-6);
    }
    return out;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return clip_color(c + (l - lum(c)));
}

fn sat(c: vec3<f32>) -> f32 {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    if mx <= mn {
        return vec3<f32>(0.0);
    }
    return (c - mn) * s / (mx - mn);
}

fn dodge(b: f32, s: f32) -> f32 {
    if b <= 0.0 {
        return 0.0;
    }
    if s >= 1.0 {
        return 1.0;
    }
    return min(1.0, b / (1.0 - s));
}

fn burn(b: f32, s: f32) -> f32 {
    if b >= 1.0 {
        return 1.0;
    }
    if s <= 0.0 {
        return 0.0;
    }
    return 1.0 - min(1.0, (1.0 - b) / s);
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        return b * 2.0 * s;
    }
    let t = 2.0 * s - 1.0;
    return b + t - b * t;
}

fn soft_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        return b - (1.0 - 2.0 * s) * b * (1.0 - b);
    }
    var d = sqrt(b);
    if b <= 0.25 {
        d = ((16.0 * b - 12.0) * b + 4.0) * b;
    }
    return b + (2.0 * s - 1.0) * (d - b);
}

fn blend(b: vec3<f32>, s: vec3<f32>, mode: u32) -> vec3<f32> {
    switch mode {
        case 1u: { return min(b, s); }
        case 2u: { return b * s; }
        case 3u: { return vec3<f32>(burn(b.r, s.r), burn(b.g, s.g), burn(b.b, s.b)); }
        case 4u: { return max(b, s); }
        case 5u: { return b + s - b * s; }
        case 6u: { return vec3<f32>(dodge(b.r, s.r), dodge(b.g, s.g), dodge(b.b, s.b)); }
        case 7u: { return vec3<f32>(hard_light(s.r, b.r), hard_light(s.g, b.g), hard_light(s.b, b.b)); }
        case 8u: { return vec3<f32>(soft_light(b.r, s.r), soft_light(b.g, s.g), soft_light(b.b, s.b)); }
        case 9u: { return vec3<f32>(hard_light(b.r, s.r), hard_light(b.g, s.g), hard_light(b.b, s.b)); }
        case 10u: { return abs(b - s); }
        case 11u: { return b + s - 2.0 * b * s; }
        case 12u: { return set_lum(set_sat(s, sat(b)), lum(b)); }
        case 13u: { return set_lum(set_sat(b, sat(s)), lum(b)); }
        case 14u: { return set_lum(s, lum(b)); }
        case 15u: { return set_lum(b, lum(s)); }
        default: { return s; }
    }
}

@fragment
fn fs_blend(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let src = textureLoad(source, texel(position), 0) * params.opacity;
    let dst = textureLoad(backdrop, texel(position), 0);
    let sa = src.a;
    let da = dst.a;
    let cs = select(vec3<f32>(0.0), src.rgb / sa, sa > 0.0);
    let cb = select(vec3<f32>(0.0), dst.rgb / da, da > 0.0);
    let mixed = clamp(blend(cb, cs, params.mode), vec3<f32>(0.0), vec3<f32>(1.0));
    let color = sa * (1.0 - da) * cs + sa * da * mixed + (1.0 - sa) * dst.rgb;
    return vec4<f32>(color, sa + da * (1.0 - sa));
}
