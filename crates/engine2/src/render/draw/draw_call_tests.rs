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
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Z).normalize();
    let up = right.cross(forward);
    let rotation = Mat4::from_cols(right.extend(0.0), up.extend(0.0), (-forward).extend(0.0), Vec4::W).transpose();
    let x_scale = 1.0 / (fov.to_radians() * 0.5).tan();
    let range = far - near;
    let projection = Mat4::from_cols(Vec4::new(x_scale, 0.0, 0.0, 0.0), Vec4::new(0.0, x_scale, 0.0, 0.0), Vec4::new(0.0, 0.0, near / range, -1.0), Vec4::new(0.0, 0.0, far * near / range, 0.0));
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
    b
}

/// One `TransformBuffer.Entry`: identity rows, opaque, `tint` as RGB888.
fn transform_entry(tint: u32) -> Vec<u8> {
    let rows = Mat4::IDENTITY.transpose().to_cols_array();
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
    let direct = middle(&|| draw_model(c, draw, mat, model, 0, 0, ids, &[Sub { first: 1, count: 1, range: None }]));
    assert!(red(direct), "the red box in the middle, got {direct:?}");
    let culled = middle(&|| draw_indirect(c, draw, mat, model, (0, 0), (ids, 1), (args, 0)));
    assert_eq!(culled, CLEAR, "no survivors, no box");
    let kept = middle(&|| draw_indirect(c, draw, mat, model, (0, 0), (ids, 1), (args, 20)));
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
