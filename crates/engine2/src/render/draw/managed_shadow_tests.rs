//! Managed's sun shadows (`ShadowSystem` + `ShadowLayer`) end to end without a boot: reverse-Z
//! orthographic depth passes of boxes into D32 cascade maps with `ShadowMapper`'s rasterizer bias,
//! then the boxes on a floor shaded through those maps by bindless index in `DirectionalLightCB`.
//! Skips without an sbox-public checkout or a GPU.

use super::tests::{SIZE, gpu_buffer, placed_entry, view_constants};
use super::{Sub, draw_model};
use crate::render::{attributes, buffers, context, device, material, texture};
use abi::StringToken;
use glam::{Mat4, Vec3, Vec4};

const SUN: Vec3 = Vec3::new(std::f32::consts::FRAC_1_SQRT_2, 0.0, -std::f32::consts::FRAC_1_SQRT_2);
/// Showroom scale: 48-unit boxes.
const K: f32 = 3.0;

/// One cascade as `DirectionalLightCB` holds it: world → (uv, reverse-Z depth), bindless index, sphere.
struct Cascade {
    world_to_uv: Mat4,
    index: i32,
    sphere: Vec4,
}

/// `DirectionalLightCB` as `ShadowSystem` fills it: a dim grey sun (so lit floor doesn't clip) and
/// `cascades` of 512² texels.
fn sun_constants(cascades: &[Cascade]) -> Vec<u8> {
    let mut w = [0u32; 104];
    w[..8].copy_from_slice(&[0.4, 0.4, 0.4, 0.0, SUN.x, SUN.y, SUN.z, 0.0].map(f32::to_bits));
    for (n, c) in cascades.iter().enumerate() {
        w[8 + 16 * n..24 + 16 * n].copy_from_slice(&c.world_to_uv.to_cols_array().map(f32::to_bits));
        w[72 + n] = c.index as u32;
        w[84 + 4 * n..88 + 4 * n].copy_from_slice(&c.sphere.to_array().map(f32::to_bits));
    }
    (w[76], w[77], w[79]) = (cascades.len() as u32, (1.0f32 / 512.0).to_bits(), 1);
    w.map(u32::to_le_bytes).concat()
}

/// `ShadowMapper.GetCascades`' shape around `center`: a sphere of radius `r` with 4096 units of
/// caster pull-back, as ReverseZOrthographic(2r, 2r, 0, 2r + 4096). Its view constants and world → uv.
fn cascade_view(center: Vec3, r: f32) -> (Vec<u8>, Mat4) {
    let (range, size) = (2.0 * r + 4096.0, 2.0 * r);
    let ortho = Mat4::from_cols(Vec4::new(2.0 / size, 0.0, 0.0, 0.0), Vec4::new(0.0, 2.0 / size, 0.0, 0.0), Vec4::new(0.0, 0.0, 1.0 / range, 0.0), Vec4::new(0.0, 0.0, 1.0, 1.0));
    let light = center - SUN * (r + 4096.0);
    let (constants, relative) = view_constants(light, center, ortho, 0.0, (0.0, range));
    let to_uv = Mat4::from_cols(Vec4::new(0.5, 0.0, 0.0, 0.0), Vec4::new(0.0, -0.5, 0.0, 0.0), Vec4::Z, Vec4::new(0.5, 0.5, 0.0, 1.0));
    // ViewConstants are camera-relative; WorldToTexture has the full translation (RenderView.cs).
    (constants, to_uv * relative * Mat4::from_translation(-light))
}

/// The main camera's view constants and world → clip.
fn main_view() -> (Vec<u8>, Mat4) {
    let persp = Mat4::from_cols(Vec4::X, Vec4::Y, Vec4::new(0.0, 0.0, 1.0 / 999.0, -1.0), Vec4::new(0.0, 0.0, 1000.0 / 999.0, 0.0));
    let eye = Vec3::new(0.0, -60.0, 60.0) * K;
    let (constants, camera) = view_constants(eye, Vec3::new(0.0, 0.0, -8.0) * K, persp, 90.0, (1.0, 1000.0));
    (constants, camera * Mat4::from_translation(-eye))
}

/// Boxes at `boxes` (×K, scaled K) on a floor, one context and managed's attribute chain.
struct Scene {
    gpu: &'static render::Gpu,
    c: context::Ref,
    frame: attributes::Ref,
    draw: attributes::Ref,
    model: crate::model::Ref,
    mat: material::Ref,
    transforms: buffers::Handle,
    ids: buffers::Handle,
    count: u32,
    textures: Vec<texture::Ref>,
}

impl Scene {
    fn new(boxes: &[Vec3]) -> Option<Scene> {
        let core = crate::render::shading::tests::core_dir()?;
        crate::fs::add_search_path("GAME", &core.to_string_lossy(), false);
        let gpu = device::create()?;
        let floor = Mat4::from_translation(Vec3::new(0.0, 0.0, -8.5 * K)) * Mat4::from_scale(Vec3::new(10.0 * K, 10.0 * K, 0.05 * K));
        let mut entries = vec![placed_entry(floor, 0xFFFFFF)];
        entries.extend(boxes.iter().map(|p| placed_entry(Mat4::from_translation(*p * K) * Mat4::from_scale(Vec3::splat(K)), 0xFF0000)));
        let count = entries.len() as u32;
        let (frame, draw) = (attributes::create(), attributes::create());
        let transforms = gpu_buffer(&entries.concat(), 64);
        attributes::set_buffer(frame, StringToken::of("g_TransformBuffer"), transforms.value);
        let ids = gpu_buffer(&(0..count).flat_map(u32::to_le_bytes).collect::<Vec<_>>(), 4);
        let model = crate::model::create_runtime("models/test/cube.vmdl", Default::default());
        let mat = material::create_raw(Some(c"materials/test/shadowed.vmat"), Some(c"shaders/complex.shader"), true);
        let c = context::create(0);
        attributes::set_parent(context::get(c).unwrap().attributes, frame);
        Some(Scene { gpu, c, frame, draw, model, mat, transforms, ids, count, textures: Vec::new() })
    }

    fn attrs(&self) -> attributes::Ref {
        context::get(self.c).unwrap().attributes
    }

    fn target(&mut self, name: &str, size: u32, format: i32) -> texture::Ref {
        let t = texture::create_target(name, size, size, format);
        self.textures.push(t);
        t
    }

    /// Every box and the floor into the bound targets; `bias` as the shadow layer passes it.
    fn draw(&self, bias: (i32, f32)) {
        assert!(draw_model(self.c, self.draw, self.mat, self.model, (0, 0), bias, self.ids, &[Sub { first: 0, count: self.count, range: None }]));
    }

    /// `ShadowSystem.BeginPass` then the casters: bind `map`, clear it (with whatever view
    /// constants the context holds), then the view's own constants.
    fn shadow_pass(&self, map: texture::Ref, constants: &[u8]) {
        context::bind_targets(self.c, texture::Ref::NULL, map);
        context::clear(self.c, crate::math::vec4(0.0, 0.0, 0.0, 0.0), false, true, false);
        attributes::set_constant_buffer(self.attrs(), StringToken::of(crate::render::shading::view::BLOCK), constants);
        self.draw((-1, -1.5));
    }

    /// The red channel at world point `p` of a read-back main view through `camera`.
    fn red_at(px: &render::Frame, camera: Mat4, p: Vec3) -> u32 {
        let ndc = camera.project_point3(p * K);
        px.texel(((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32, ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32)[0] as u32
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        for t in self.textures.drain(..) {
            crate::resource::destroy_handle(t);
        }
        crate::resource::destroy_handle(self.model);
        crate::resource::destroy_handle(self.mat);
        buffers::destroy(self.transforms);
        buffers::destroy(self.ids);
        context::release(self.c);
    }
}

/// The floor beside `at` (×K), down-sun of a box, is darker than up-sun.
fn assert_shadowed(px: &render::Frame, camera: Mat4, at: Vec3, what: &str) {
    let (shadowed, lit) = (Scene::red_at(px, camera, at + Vec3::new(14.0, 0.0, -8.0)), Scene::red_at(px, camera, at + Vec3::new(-14.0, 0.0, -8.0)));
    assert!(shadowed + 30 < lit, "{what}: floor down-sun in shadow: shadowed {shadowed}, lit {lit}");
}

/// One cascade like the Low preset's (one 512² map over a 2700-unit sphere), `DirectionalLightCB`
/// inherited from the frame's attributes.
#[test]
fn managed_sun_shadow_darkens_the_floor() {
    let Some(mut s) = Scene::new(&[Vec3::ZERO]) else { return };
    let map = s.target("shadow map", 512, 62);
    let (light_constants, world_to_uv) = cascade_view(Vec3::ZERO, 2700.0);
    let (constants, camera) = main_view();
    // The frame's constants come first: the map's clear needs them to know it's reverse-Z.
    attributes::set_constant_buffer(s.attrs(), StringToken::of(crate::render::shading::view::BLOCK), &constants);
    s.shadow_pass(map, &light_constants);

    let index = crate::render::tools::texture_view_index(map);
    attributes::set_constant_buffer(s.frame, StringToken::of("DirectionalLightCB"), &sun_constants(&[Cascade { world_to_uv, index, sphere: Vec4::new(0.0, 0.0, 0.0, 1e9) }]));
    attributes::set_constant_buffer(s.attrs(), StringToken::of(crate::render::shading::view::BLOCK), &constants);
    let (color, depth) = (s.target("shadowed", SIZE, 0), s.target("shadowed depth", SIZE, 62));
    context::bind_targets(s.c, color, depth);
    context::clear(s.c, crate::math::vec4(0.0, 0.0, 1.0, 1.0), true, true, true);
    s.draw((0, 0.0));
    let px = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(s.gpu);
    assert_shadowed(&px, camera, Vec3::ZERO, "one cascade");
    let lit = Scene::red_at(&px, camera, Vec3::new(-14.0, 0.0, -8.0));
    assert_eq!(Scene::red_at(&px, camera, Vec3::new(0.0, 0.0, 8.0)), lit, "the box top doesn't shadow itself (acne)");
}

/// The boot's order (`UNDERBOX_SUBMITS=log`): one context whose own attributes hold the main view's
/// constants and `DirectionalLightCB` throughout; the depth prepass clear, then each cascade's clear
/// (under the previous view's constants) and casters, a local light's D16 map, and the main view
/// last. Two cascades: a tight one around the near box, a wide one for the far box.
#[test]
fn managed_cascades_in_boot_order() {
    // Cascade 0's map doesn't reach the far box, so its shadow needs cascade 1.
    let far = Vec3::new(0.0, 40.0, 0.0);
    let Some(mut s) = Scene::new(&[Vec3::ZERO, far]) else { return };
    let (main_constants, camera) = main_view();
    let block = StringToken::of(crate::render::shading::view::BLOCK);
    let maps = [s.target("cascade 0", 512, 62), s.target("cascade 1", 512, 62)];
    let views = [cascade_view(Vec3::ZERO, 20.0 * K), cascade_view(Vec3::ZERO, 2700.0)];
    let near_sphere = Vec4::new(0.0, 0.0, 0.0, (18.0 * K) * (18.0 * K));
    let cascades: Vec<Cascade> = (0..2).map(|n| Cascade { world_to_uv: views[n].1, index: crate::render::tools::texture_view_index(maps[n]), sphere: if n == 0 { near_sphere } else { Vec4::new(0.0, 0.0, 0.0, 1e9) } }).collect();
    attributes::set_constant_buffer(s.attrs(), StringToken::of("DirectionalLightCB"), &sun_constants(&cascades));
    attributes::set_constant_buffer(s.attrs(), block, &main_constants);

    let (color, depth) = (s.target("boot order", SIZE, 0), s.target("boot order depth", SIZE, 62));
    context::bind_targets(s.c, texture::Ref::NULL, depth);
    context::clear(s.c, crate::math::vec4(0.0, 0.0, 0.0, 0.0), false, true, false);
    for (map, (constants, _)) in maps.iter().zip(&views) {
        s.shadow_pass(*map, constants);
    }
    let local = s.target("local light", 512, 60);
    s.shadow_pass(local, &views[0].0);

    attributes::set_constant_buffer(s.attrs(), block, &main_constants);
    context::bind_targets(s.c, color, depth);
    context::clear(s.c, crate::math::vec4(0.0, 0.0, 1.0, 1.0), true, false, false);
    s.draw((0, 0.0));
    let px = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(s.gpu);
    assert_shadowed(&px, camera, Vec3::ZERO, "cascade 0");
    assert_shadowed(&px, camera, far, "cascade 1");
}
