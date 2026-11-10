// Placeholder forward shader until translated vfx combos exist (docs/renderer.md, "Shaders").

struct Draw {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    tint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> draw: Draw;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) normal: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

const LIGHT: vec3<f32> = vec3<f32>(0.32, 0.48, 0.82);

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = draw.mvp * vec4<f32>(v.position, 1.0);
    // Meshes without normals (the defaults buffer's zero normal) are drawn unlit.
    var shade = 1.0;
    if (dot(v.normal, v.normal) > 0.0) {
        let n = normalize((draw.model * vec4<f32>(v.normal, 0.0)).xyz);
        shade = 0.35 + 0.65 * max(dot(n, LIGHT), 0.0);
    }
    out.color = vec4<f32>(v.color.rgb * shade, v.color.a) * draw.tint;
    return out;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    return i.color;
}
