
struct Transform {
    scale: f32,
    _pad: f32,
    offset: vec2<f32>,
    size: vec2<f32>,
    _pad2: vec2<f32>,
};

@group(0) @binding(0) var<uniform> transform: Transform;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOut {
    let px = pos * transform.scale + transform.offset;
    var out: VertexOut;
    out.position = vec4<f32>(px.x / transform.size.x * 2.0 - 1.0, 1.0 - px.y / transform.size.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(image, image_sampler, in.uv) * in.color;
}
