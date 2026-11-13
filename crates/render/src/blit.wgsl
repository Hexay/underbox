// Blit: copy a source rect into the bound viewport with a full-screen triangle, nearest texel,
// converting format (e.g. Rgba16Float HDR into an Rgba8Unorm swap chain, clamped) and optionally
// applying the sRGB transfer function (blit.rs `Transfer`), one fragment entry point per transfer.

@group(0) @binding(0) var src: texture_2d<f32>;
struct Params {
    // x, y, w, h of the source rect in texels.
    rect: vec4<f32>,
}
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var nearest: sampler;

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

// The texel under p, through a nearest clamping sampler at its centre: on lavapipe a sample costs
// less than textureLoad's integer path.
fn texel(v: Varyings) -> vec4<f32> {
    let p = params.rect.xy + v.uv * params.rect.zw;
    return textureSampleLevel(src, nearest, (floor(p) + 0.5) / vec2<f32>(textureDimensions(src)), 0.0);
}

@fragment
fn fs_copy(v: Varyings) -> @location(0) vec4<f32> {
    return texel(v);
}

@fragment
fn fs_encode(v: Varyings) -> @location(0) vec4<f32> {
    let c = texel(v);
    let x = max(c.rgb, vec3<f32>(0.0));
    return vec4<f32>(select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308)), c.a);
}

@fragment
fn fs_decode(v: Varyings) -> @location(0) vec4<f32> {
    let c = texel(v);
    let x = max(c.rgb, vec3<f32>(0.0));
    return vec4<f32>(select(pow((x + 0.055) / 1.055, vec3<f32>(2.4)), x / 12.92, x <= vec3<f32>(0.04045)), c.a);
}
