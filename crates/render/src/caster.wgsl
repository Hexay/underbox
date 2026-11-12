// Depth-only shadow casters (caster.rs): one mesh, one world matrix per instance.

@group(0) @binding(0) var<uniform> view_proj: mat4x4<f32>;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) world0: vec4<f32>,
    @location(2) world1: vec4<f32>,
    @location(3) world2: vec4<f32>,
    @location(4) world3: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn) -> @builtin(position) vec4<f32> {
    let world = mat4x4<f32>(v.world0, v.world1, v.world2, v.world3);
    return view_proj * world * vec4<f32>(v.position, 1.0);
}
