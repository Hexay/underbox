// Blit: copy a source rect into the bound viewport with a full-screen triangle, nearest texel,
// converting format (e.g. Rgba16Float HDR into an Rgba8Unorm swap chain, clamped).

@group(0) @binding(0) var src: texture_2d<f32>;
// x, y, w, h of the source rect in texels.
@group(0) @binding(1) var<uniform> rect: vec4<f32>;

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Varyings {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Varyings;
    out.position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(v: Varyings) -> @location(0) vec4<f32> {
    let p = rect.xy + v.uv * rect.zw;
    let last = vec2<i32>(textureDimensions(src)) - vec2<i32>(1, 1);
    return textureLoad(src, clamp(vec2<i32>(floor(p)), vec2<i32>(0, 0), last), 0);
}
