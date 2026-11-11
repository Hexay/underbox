// The volumetric fog froxels: each column marched front to back, every slice storing the
// in-scattering and transmittance accumulated up to it (render/fog_volume.rs, docs/fog.md).

struct Params {
    inverse_view_proj: mat4x4f,
    camera_near: vec4f,
    forward_range: vec4f,
    // Fade-in start and end, phase asymmetry, scattering.
    fade_phase: vec4f,
    ambient: vec4f,
    sun_direction: vec4f,
    sun_color: vec4f,
    counts: vec4u,
};

struct Volume {
    world_to_volume: mat4x4f,
    center_density: vec4f,
    half_exponent: vec4f,
    color_spherical: vec4f,
};

struct Light {
    position_radius: vec4f,
    color: vec4f,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> volumes: array<Volume>;
@group(0) @binding(2) var<storage, read> lights: array<Light>;
@group(0) @binding(3) var froxels: texture_storage_3d<rgba16float, write>;

// Henyey-Greenstein, scaled so an isotropic medium scatters 1.
fn phase(cosine: f32) -> f32 {
    let g = p.fade_phase.z;
    return (1.0 - g * g) / pow(max(1.0 + g * g - 2.0 * g * cosine, 1e-4), 1.5);
}

// rgb the density-weighted colour sum, a the density.
fn medium(world: vec3f) -> vec4f {
    var sum = vec4f(0.0);
    for (var i = 0u; i < p.counts.x; i++) {
        let v = volumes[i];
        let local = ((v.world_to_volume * vec4f(world, 1.0)).xyz - v.center_density.xyz) / v.half_exponent.xyz;
        let a = abs(local);
        let edge = select(max(a.x, max(a.y, a.z)), length(local), v.color_spherical.w > 0.5);
        if edge < 1.0 {
            let density = v.center_density.w * pow(1.0 - edge, v.half_exponent.w);
            sum += vec4f(v.color_spherical.rgb * density, density);
        }
    }
    return sum;
}

fn incoming(world: vec3f, direction: vec3f) -> vec3f {
    var light = p.ambient.rgb + p.sun_color.rgb * phase(dot(direction, p.sun_direction.xyz));
    for (var i = 0u; i < p.counts.y; i++) {
        let l = lights[i];
        let to_light = l.position_radius.xyz - world;
        let falloff = saturate(1.0 - dot(to_light, to_light) / (l.position_radius.w * l.position_radius.w));
        if falloff > 0.0 {
            light += l.color.rgb * falloff * falloff * phase(dot(direction, normalize(to_light)));
        }
    }
    return light;
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(froxels);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    // Slices are view-depth planes; v runs down the screen, as volumetric_fog.fxc reads it.
    let ndc = vec2f((f32(id.x) + 0.5) / f32(size.x) * 2.0 - 1.0, 1.0 - (f32(id.y) + 0.5) / f32(size.y) * 2.0);
    let point = p.inverse_view_proj * vec4f(ndc, 0.5, 1.0);
    let direction = normalize(point.xyz / point.w - p.camera_near.xyz);
    let per_depth = 1.0 / max(dot(direction, p.forward_range.xyz), 1e-3);
    let slice = p.forward_range.w / f32(size.z);
    var scattered = vec3f(0.0);
    var transmittance = 1.0;
    for (var z = 0u; z < size.z; z++) {
        let depth = p.camera_near.w + (f32(z) + 0.5) * slice;
        let world = p.camera_near.xyz + direction * depth * per_depth;
        let m = medium(world) * smoothstep(p.fade_phase.x, p.fade_phase.y, depth);
        if m.a > 0.0 {
            let through = exp(-m.a * slice * per_depth);
            scattered += transmittance * (1.0 - through) * (m.rgb / m.a) * incoming(world, direction) * p.fade_phase.w;
            transmittance *= through;
        }
        textureStore(froxels, vec3u(id.xy, z), vec4f(scattered, transmittance));
    }
}
