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
    // Volumes, lights, whether there is history, whether the sun has a shadow map.
    counts: vec4u,
    previous_view_proj: mat4x4f,
    // This frame's weight, the history's near and range.
    history_blend: vec4f,
    // World → sun shadow-map uv (y down) and depth (lighting/shadow.rs).
    sun_shadow: mat4x4f,
};

struct Volume {
    world_to_volume: mat4x4f,
    center_density: vec4f,
    half_exponent: vec4f,
    color_spherical: vec4f,
};

struct Light {
    position_radius: vec4f,
    // w: the outer cone cosine.
    color_outer: vec4f,
    // w: the inner cone cosine.
    direction_inner: vec4f,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> volumes: array<Volume>;
@group(0) @binding(2) var<storage, read> lights: array<Light>;
@group(0) @binding(3) var froxels: texture_storage_3d<rgba16float, write>;
// Per-froxel lighting (rgb in-scattered radiance, a density): last frame's, and this frame's.
@group(0) @binding(4) var history: texture_3d<f32>;
@group(0) @binding(5) var lighting: texture_storage_3d<rgba16float, write>;
@group(0) @binding(6) var sun_shadow_map: texture_depth_2d;

fn shadow_tap(texel: vec2i, depth: f32) -> f32 {
    let size = vec2i(textureDimensions(sun_shadow_map));
    let stored = textureLoad(sun_shadow_map, clamp(texel, vec2i(0), size - 1), 0);
    // counts.w: 1 a map where nearer is smaller, 2 a reverse-Z one (managed's cascades).
    if p.counts.w == 2u {
        return select(0.0, 1.0, depth >= stored - 1e-3);
    }
    return select(0.0, 1.0, depth <= stored + 1e-3);
}

// The fraction of the sun reaching `world`; points outside the map count as lit. A 4x4-texel tent
// (bilinear weights over a 3x3 PCF), since a single tap bands at the shadow's edge.
fn sun_visibility(world: vec3f) -> f32 {
    if p.counts.w == 0u {
        return 1.0;
    }
    let s = p.sun_shadow * vec4f(world, 1.0);
    if any(s.xy < vec2f(0.0)) || any(s.xy >= vec2f(1.0)) || s.z < 0.0 || s.z > 1.0 {
        return 1.0;
    }
    let at = s.xy * vec2f(textureDimensions(sun_shadow_map)) - 0.5;
    let base = vec2i(floor(at));
    let f = fract(at);
    var wx = array<f32, 4>(1.0 - f.x, 1.0, 1.0, f.x);
    var wy = array<f32, 4>(1.0 - f.y, 1.0, 1.0, f.y);
    var lit = 0.0;
    for (var j = 0; j < 4; j++) {
        for (var i = 0; i < 4; i++) {
            lit += wx[i] * wy[j] * shadow_tap(base + vec2i(i - 1, j - 1), s.z);
        }
    }
    return lit / 9.0;
}

// A history within this fraction of the current value is replaced by it. History is half
// precision, and a rounded blend stalls anywhere within ~1.7 ulps (2^-10) of its target, so
// without the snap a still view settles on a value that depends on the frames before it.
const SNAP: f32 = 1.0 / 256.0;

// `current` blended with the froxel last frame had at `world`, reprojected through its
// world-to-clip (w is view depth). The nearest froxel, not a filtered one: a still camera
// reads back exactly what it wrote.
fn temporal(world: vec3f, current: vec4f) -> vec4f {
    if p.counts.z == 0u {
        return current;
    }
    let clip = p.previous_view_proj * vec4f(world, 1.0);
    if clip.w <= 0.0 {
        return current;
    }
    let size = vec3f(textureDimensions(history));
    let uvw = vec3f(clip.x / clip.w * 0.5 + 0.5, 0.5 - clip.y / clip.w * 0.5, (clip.w - p.history_blend.y) / p.history_blend.z);
    let at = uvw * size;
    if any(at < vec3f(0.0)) || any(at >= size) {
        return current;
    }
    let previous = textureLoad(history, vec3u(at), 0);
    let settled = abs(previous - current) <= max(abs(current) * SNAP, vec4f(1e-6));
    return select(mix(previous, current, p.history_blend.x), current, settled);
}

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
    var light = p.ambient.rgb + p.sun_color.rgb * phase(dot(direction, p.sun_direction.xyz)) * sun_visibility(world);
    for (var i = 0u; i < p.counts.y; i++) {
        let l = lights[i];
        let to_light = l.position_radius.xyz - world;
        let falloff = saturate(1.0 - dot(to_light, to_light) / (l.position_radius.w * l.position_radius.w));
        let toward = normalize(to_light);
        let cone = smoothstep(l.color_outer.w, l.direction_inner.w, dot(-toward, l.direction_inner.xyz));
        if falloff > 0.0 && cone > 0.0 {
            light += l.color_outer.rgb * falloff * falloff * cone * phase(dot(direction, toward));
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
        var lit = vec4f(0.0);
        if m.a > 0.0 {
            lit = vec4f((m.rgb / m.a) * incoming(world, direction) * p.fade_phase.w, m.a);
        }
        lit = temporal(world, lit);
        textureStore(lighting, vec3u(id.xy, z), lit);
        if lit.a > 0.0 {
            let through = exp(-lit.a * slice * per_depth);
            scattered += transmittance * (1.0 - through) * lit.rgb;
            transmittance *= through;
        }
        textureStore(froxels, vec3u(id.xy, z), vec4f(scattered, transmittance));
    }
}
