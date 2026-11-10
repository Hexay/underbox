// Placeholder forward shader until translated vfx combos exist (docs/renderer.md, "Shaders").

struct Draw {
    mvp: mat4x4<f32>,
    tint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> draw: Draw;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = draw.mvp * vec4<f32>(v.position, 1.0);
    out.color = v.color * draw.tint;
    return out;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    return i.color;
}
