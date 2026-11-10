// Blit: copy a source rect into the bound viewport with a full-screen triangle, nearest texel,
// converting format (e.g. Rgba16Float HDR into an Rgba8Unorm swap chain, clamped) and optionally
// applying the sRGB transfer function (blit.rs `Transfer`).

@group(0) @binding(0) var src: texture_2d<f32>;
struct Params {
    // x, y, w, h of the source rect in texels.
    rect: vec4<f32>,
    // x: 0 as is, 1 linear -> sRGB encode, 2 sRGB -> linear decode.
    transfer: vec4<f32>,
}
@group(0) @binding(1) var<uniform> params: Params;

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

fn encode(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}

fn decode(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(pow((x + 0.055) / 1.055, vec3<f32>(2.4)), x / 12.92, x <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(v: Varyings) -> @location(0) vec4<f32> {
    let p = params.rect.xy + v.uv * params.rect.zw;
    let last = vec2<i32>(textureDimensions(src)) - vec2<i32>(1, 1);
    let c = textureLoad(src, clamp(vec2<i32>(floor(p)), vec2<i32>(0, 0), last), 0);
    if params.transfer.x == 1.0 {
        return vec4<f32>(encode(c.rgb), c.a);
    }
    if params.transfer.x == 2.0 {
        return vec4<f32>(decode(c.rgb), c.a);
    }
    return c;
}
