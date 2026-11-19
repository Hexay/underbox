// One mip from the mip above: each texel the average of the 2x2 texels it covers (clamped at an
// odd edge). Loads go through the source view's format, so an sRGB view averages decoded values.

@group(0) @binding(0) var src: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let last = vec2<i32>(textureDimensions(src)) - 1;
    let at = vec2<i32>(p.xy) * 2;
    let a = textureLoad(src, min(at, last), 0);
    let b = textureLoad(src, min(at + vec2<i32>(1, 0), last), 0);
    let c = textureLoad(src, min(at + vec2<i32>(0, 1), last), 0);
    let d = textureLoad(src, min(at + vec2<i32>(1, 1), last), 0);
    return (a + b + c + d) * 0.25;
}
