//! `DrawModelDrawCall` the way `Sandbox.SceneRenderer` (`r_managed_scene`) issues it, without a
//! client boot: managed's attribute chain (frame → segment → draw, the context's parented to the
//! segment), its 1088-byte `ViewConstants` (row-vector, camera-relative, reverse-Z), a
//! `TransformBuffer.Entry` GPU buffer as `g_TransformBuffer` and an instance-id vertex buffer,
//! then the colour target read back. Skips without an sbox-public checkout or a GPU.

use super::{Sub, draw_indirect, draw_model};
use crate::render::{attributes, buffers, context, device, material, texture};
use abi::StringToken;
use abi::structs::BufferDesc;
use glam::{Mat4, Vec3, Vec4};

const SIZE: u32 = 64;
const CLEAR: [u8; 4] = [0, 0, 255, 255];

/// `RenderView.Update` + `FillConstants` for a camera at `eye` looking at `target` (horizontal
/// FOV `fov` degrees): `ViewConstants` bytes. A row-vector matrix's row-major bytes are the
/// column-vector matrix's columns, so glam's `to_cols_array` writes them.
fn managed_view_constants(eye: Vec3, target: Vec3, fov: f32, (near, far): (f32, f32)) -> Vec<u8> {
    let x_scale = 1.0 / (fov.to_radians() * 0.5).tan();
    let range = far - near;
    let projection = Mat4::from_cols(Vec4::new(x_scale, 0.0, 0.0, 0.0), Vec4::new(0.0, x_scale, 0.0, 0.0), Vec4::new(0.0, 0.0, near / range, -1.0), Vec4::new(0.0, 0.0, far * near / range, 0.0));
    view_constants(eye, target, projection, fov, (near, far)).0
}

/// [`managed_view_constants`] for any projection, with the world-to-projection matrix.
fn view_constants(eye: Vec3, target: Vec3, projection: Mat4, fov: f32, (near, far): (f32, f32)) -> (Vec<u8>, Mat4) {
    let forward = (target - eye).normalize();
    let right = forward.cross(if forward.z.abs() > 0.99 { Vec3::X } else { Vec3::Z }).normalize();
    let up = right.cross(forward);
    let rotation = Mat4::from_cols(right.extend(0.0), up.extend(0.0), (-forward).extend(0.0), Vec4::W).transpose();
    let world_to_projection = projection * rotation;
    let world_to_view = rotation * Mat4::from_translation(-eye);

    let mut b = vec![0u8; 1088];
    let mut put = |offset: usize, values: &[f32]| {
        for (i, v) in values.iter().enumerate() {
            b[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
    };
    let s = SIZE as f32;
    put(0, &world_to_projection.to_cols_array());
    put(64, &world_to_projection.inverse().to_cols_array());
    put(128, &world_to_view.to_cols_array());
    put(192, &projection.to_cols_array());
    put(288, &[1.0, 1.0, 1.0, 1.0]);
    put(304, &[eye.x, eye.y, eye.z, 0.0]);
    put(320, &[forward.x, forward.y, forward.z, 1.0]);
    put(336, &[up.x, up.y, up.z]);
    put(364, &[near, far, fov.to_radians(), 1.0 / s, 1.0 / s, 1.0, 1.0]);
    put(400, &[1.0 / s, 1.0 / s, 1.0, 1.0, 0.0, 0.0, s, s, s, s]);
    put(704, &[-eye.x, -eye.y, -eye.z, 0.0]);
    put(944, &projection.inverse().to_cols_array());
    (b, world_to_projection)
}

/// One `TransformBuffer.Entry`: identity rows, opaque, `tint` as RGB888.
fn transform_entry(tint: u32) -> Vec<u8> {
    placed_entry(Mat4::IDENTITY, tint)
}

/// One `TransformBuffer.Entry` for `world`.
fn placed_entry(world: Mat4, tint: u32) -> Vec<u8> {
    let rows = world.transpose().to_cols_array();
    let mut b: Vec<u8> = rows[..12].iter().flat_map(|f| f.to_le_bytes()).collect();
    b.extend(1f32.to_le_bytes());
    b.extend(tint.to_le_bytes());
    b.extend([0u8; 8]);
    b
}

fn gpu_buffer(bytes: &[u8], element: i32) -> buffers::Handle {
    let desc = BufferDesc { m_n_element_count: bytes.len() as i32 / element, m_n_element_size_in_bytes: element, ..Default::default() };
    let h = buffers::create(0, desc, 0, None);
    buffers::set_data(context::Ref::NULL, h, abi::RawPtr::from_raw(bytes.as_ptr().cast_mut().cast()), bytes.len() as u32, 0);
    h
}

#[test]
fn managed_scene_renderer_draws_a_box() {
    let Some(core) = crate::render::shading::tests::core_dir() else {
        eprintln!("skipped: no sbox-public checkout");
        return;
    };
    // The stand-in's includes (common.fxc) resolve through the search paths, as in a client.
    crate::fs::add_search_path("GAME", &core.to_string_lossy(), false);
    let Some(gpu) = device::create() else {
        eprintln!("skipped: no GPU");
        return;
    };
    let (frame, segment, draw) = (attributes::create(), attributes::create(), attributes::create());
    attributes::set_parent(segment, frame);
    attributes::set_parent(draw, segment);
    let c = context::create(0);
    attributes::set_parent(context::get(c).unwrap().attributes, segment);

    let constants = managed_view_constants(Vec3::new(-30.0, 0.0, 30.0), Vec3::ZERO, 90.0, (1.0, 1000.0));
    attributes::set_constant_buffer(segment, StringToken::of(crate::render::shading::view::BLOCK), &constants);
    let transforms = gpu_buffer(&transform_entry(0xFF0000), 64);
    attributes::set_buffer(frame, StringToken::of("g_TransformBuffer"), transforms.value);
    // Slot 0 is another object's; the box reads entry 0 through instance slot 1.
    let ids = gpu_buffer(&[7u32, 0].map(u32::to_le_bytes).concat(), 4);

    // Culled draws' args: the second has the occlusion cull's one survivor.
    let args = gpu_buffer(&[36u32, 0, 0, 0, 0, 36, 1, 0, 0, 0].map(u32::to_le_bytes).concat(), 20);

    let (color, depth) = (texture::create_target("managed", SIZE, SIZE, 0), texture::create_target("managed depth", SIZE, SIZE, 62));
    context::bind_targets(c, color, depth);
    let model = crate::model::create_runtime("models/test/cube.vmdl", Default::default());
    let mat = material::create_raw(Some(c"materials/test/managed.vmat"), Some(c"shaders/complex.shader"), true);
    let middle = |draw: &dyn Fn() -> bool| {
        context::clear(c, crate::math::vec4(0.0, 0.0, 1.0, 1.0), true, true, true);
        assert!(draw(), "the draw call is accepted");
        let px = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(gpu);
        assert_eq!(px.texel(0, 0), CLEAR, "clear colour in the corner");
        px.texel(32, 32)[..4].try_into().unwrap()
    };
    let red = |[r, g, b, _]: [u8; 4]| r > 60 && g < 10 && b < 10;
    let direct = middle(&|| draw_model(c, draw, mat, model, (0, 0), (0, 0.0), ids,&[Sub { first: 1, count: 1, range: None }]));
    assert!(red(direct), "the red box in the middle, got {direct:?}");
    let culled = middle(&|| draw_indirect(c, draw, mat, model, (0, 0), (0, 0.0), (ids, 1), (args, 0)));
    assert_eq!(culled, CLEAR, "no survivors, no box");
    let kept = middle(&|| draw_indirect(c, draw, mat, model, (0, 0), (0, 0.0), (ids, 1), (args, 20)));
    assert_eq!(kept, direct, "a survivor draws as the direct call does");
    buffers::destroy(args);
    for t in [color, depth] {
        crate::resource::destroy_handle(t);
    }
    crate::resource::destroy_handle(model);
    crate::resource::destroy_handle(mat);
    buffers::destroy(transforms);
    buffers::destroy(ids);
    context::release(c);
}

/// `DirectionalLightCB` as `ShadowSystem` fills it: a dim grey sun (so lit floor doesn't clip) along `direction`, one cascade
/// `world_to_shadow` (world → uv, reverse-Z depth) in the bindless map `index`, 512² texels.
fn sun_constants(direction: Vec3, world_to_shadow: Mat4, index: i32) -> Vec<u8> {
    let mut w = [0u32; 104];
    w[..8].copy_from_slice(&[0.4, 0.4, 0.4, 0.0, direction.x, direction.y, direction.z, 0.0].map(f32::to_bits));
    w[8..24].copy_from_slice(&world_to_shadow.to_cols_array().map(f32::to_bits));
    (w[72], w[76], w[77], w[79]) = (index as u32, 1, (1.0f32 / 512.0).to_bits(), 1);
    w.map(u32::to_le_bytes).concat()
}

/// Managed's sun shadows end to end: a reverse-Z orthographic depth pass of a box into a D32 map
/// (with `ShadowMapper`'s rasterizer bias), then the box on a floor shaded with that map named by
/// bindless index in `DirectionalLightCB`. The floor beside the box, down the sun, is in shadow.
#[test]
fn managed_sun_shadow_darkens_the_floor() {
    let Some(core) = crate::render::shading::tests::core_dir() else { return };
    crate::fs::add_search_path("GAME", &core.to_string_lossy(), false);
    let Some(gpu) = device::create() else { return };
    let (frame, light_view, main_view, draw) = (attributes::create(), attributes::create(), attributes::create(), attributes::create());
    for a in [light_view, main_view] {
        attributes::set_parent(a, frame);
    }
    let c = context::create(0);
    let ctx_attrs = context::get(c).unwrap().attributes;
    let sun = Vec3::new(1.0, 0.0, -1.0).normalize();
    // Showroom scale: 48-unit boxes under a cascade like managed's for that view.
    let k = 3.0;
    let floor = Mat4::from_translation(Vec3::new(0.0, 0.0, -8.5 * k)) * Mat4::from_scale(Vec3::new(10.0 * k, 10.0 * k, 0.05 * k));
    let transforms = gpu_buffer(&[placed_entry(Mat4::from_scale(Vec3::splat(k)), 0xFF0000), placed_entry(floor, 0xFFFFFF)].concat(), 64);
    attributes::set_buffer(frame, StringToken::of("g_TransformBuffer"), transforms.value);
    let ids = gpu_buffer(&[0u32, 1].map(u32::to_le_bytes).concat(), 4);
    let model = crate::model::create_runtime("models/test/cube.vmdl", Default::default());
    let mat = material::create_raw(Some(c"materials/test/shadowed.vmat"), Some(c"shaders/complex.shader"), true);
    let both = [Sub { first: 0, count: 2, range: None }];

    // The shadow pass: ShadowMapper.GetCascades' shape, a sphere of radius r with 4096 units of
    // caster pull-back, as ReverseZOrthographic(2r, 2r, 0, 2r + 4096).
    let r = 2700.0;
    let (range, size) = (2.0 * r + 4096.0, 2.0 * r);
    let ortho = Mat4::from_cols(Vec4::new(2.0 / size, 0.0, 0.0, 0.0), Vec4::new(0.0, 2.0 / size, 0.0, 0.0), Vec4::new(0.0, 0.0, 1.0 / range, 0.0), Vec4::new(0.0, 0.0, 1.0, 1.0));
    let light = -sun * (r + 4096.0);
    let (constants, light_relative) = view_constants(light, Vec3::ZERO, ortho, 0.0, (0.0, range));
    // ViewConstants are camera-relative; WorldToTexture has the full translation (RenderView.cs).
    let light_to_projection = light_relative * Mat4::from_translation(-light);
    attributes::set_constant_buffer(light_view, StringToken::of(crate::render::shading::view::BLOCK), &constants);
    attributes::set_parent(ctx_attrs, light_view);
    let map = texture::create_target("shadow map", 512, 512, 62);
    context::bind_targets(c, texture::Ref::NULL, map);
    context::clear(c, crate::math::vec4(0.0, 0.0, 0.0, 0.0), false, true, false);
    assert!(draw_model(c, draw, mat, model, (0, 0), (-1, -1.5), ids, &both));

    // The main pass, lit by the sun through that map.
    let to_uv = Mat4::from_cols(Vec4::new(0.5, 0.0, 0.0, 0.0), Vec4::new(0.0, -0.5, 0.0, 0.0), Vec4::Z, Vec4::new(0.5, 0.5, 0.0, 1.0));
    let index = crate::render::bindless::index(map, false);
    attributes::set_constant_buffer(main_view, StringToken::of("DirectionalLightCB"), &sun_constants(sun, to_uv * light_to_projection, index));
    let x_scale = 1.0;
    let persp = Mat4::from_cols(Vec4::new(x_scale, 0.0, 0.0, 0.0), Vec4::new(0.0, x_scale, 0.0, 0.0), Vec4::new(0.0, 0.0, 1.0 / 999.0, -1.0), Vec4::new(0.0, 0.0, 1000.0 / 999.0, 0.0));
    let eye = Vec3::new(0.0, -60.0, 60.0) * k;
    let (constants, camera) = view_constants(eye, Vec3::new(0.0, 0.0, -8.0) * k, persp, 90.0, (1.0, 1000.0));
    let camera = camera * Mat4::from_translation(-eye);
    attributes::set_constant_buffer(main_view, StringToken::of(crate::render::shading::view::BLOCK), &constants);
    attributes::set_parent(ctx_attrs, main_view);
    let (color, depth) = (texture::create_target("shadowed", SIZE, SIZE, 0), texture::create_target("shadowed depth", SIZE, SIZE, 62));
    context::bind_targets(c, color, depth);
    context::clear(c, crate::math::vec4(0.0, 0.0, 1.0, 1.0), true, true, true);
    assert!(draw_model(c, draw, mat, model, (0, 0), (0, 0.0), ids, &both));
    let px = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(gpu);

    let at = |p: Vec3| {
        let ndc = camera.project_point3(p);
        let (x, y) = ((ndc.x * 0.5 + 0.5) * SIZE as f32, (0.5 - ndc.y * 0.5) * SIZE as f32);
        px.texel(x as u32, y as u32)[0] as u32
    };
    let (shadowed, lit) = (at(Vec3::new(14.0, 0.0, -8.0) * k), at(Vec3::new(-14.0, 0.0, -8.0) * k));
    assert!(shadowed + 30 < lit, "floor down-sun of the box in shadow: shadowed {shadowed}, lit {lit}");
    let top = at(Vec3::new(0.0, 0.0, 8.0) * k);
    assert_eq!(top, lit, "the box top doesn't shadow itself (acne)");
    for t in [color, depth, map] {
        crate::resource::destroy_handle(t);
    }
    crate::resource::destroy_handle(model);
    crate::resource::destroy_handle(mat);
    buffers::destroy(transforms);
    buffers::destroy(ids);
    context::release(c);
}
