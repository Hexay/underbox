// A clear limited to the pass's scissor: a full-screen triangle at the clear depth, in the clear colour.

struct Params {
    color: vec4<f32>,
    // x: depth.
    depth: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), params.depth.x, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return params.color;
}
