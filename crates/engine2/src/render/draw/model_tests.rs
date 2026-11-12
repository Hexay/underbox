use super::*;
use crate::mesh::material_ref::MaterialRef;
use crate::render::draw::{self, Load, Target};
use crate::render::lighting::Lighting;
use crate::render::{device, material, texture};
use glam::Vec3;

/// Whether the stand-in cube (an error model) covers the centre seen from `eye`, drawn with
/// `material` or, without one, the placeholder.
fn covers_centre(eye: Vec3, material: Option<&Arc<MaterialRef>>) -> bool {
    let content = crate::model::content::Content::from_meshes(&[assets::model::stand_in()]);
    let mesh = &content.meshes[0].mesh;
    let view_proj = render::Camera::perspective(90f32.to_radians(), 1.0, 1.0, eye, eye + Vec3::X).view_proj;
    let uniforms = DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, tint: [1.0; 4] };
    let call = DrawCall { material: material.cloned(), ..mesh.draw_calls()[0].clone() };
    let d = cull_placeholder(draw_call(mesh, &call, uniforms, view_proj, RasterState::default()).unwrap());
    assert_eq!(d.shaded.is_some(), material.is_some(), "shaded by its material, else the placeholder");
    let (color, depth) = (texture::create_target("cull", 16, 16, 0), texture::create_target("cull depth", 16, 16, 62));
    let target = Target { color, depth, ..Default::default() };
    draw::execute(&target, &Lighting::default(), Load { color: Some(render::wgpu::Color::BLACK), depth: Some(0.0) }, &[d]);
    let frame = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(device::gpu().unwrap());
    [color, depth].into_iter().for_each(crate::resource::destroy_handle);
    frame.texel(8, 8)[..3] != [0, 0, 0]
}

#[test]
fn placeholder_culls_back_faces() {
    if device::create().is_none() {
        eprintln!("skipped: no GPU");
        return;
    }
    assert!(covers_centre(Vec3::new(-60.0, 0.0, 0.0), None), "seen from outside");
    assert!(!covers_centre(Vec3::ZERO, None), "from inside, only back faces: nothing drawn");
}

/// The material stand-in culls back faces, as real's closed shaders do, unless
/// `F_RENDER_BACKFACES`. Unlit, so the unlit test view still shows it.
#[test]
fn standin_material_culls_back_faces_unless_two_sided() {
    let Some(core) = crate::render::shading::tests::core_dir() else {
        eprintln!("skipped: no sbox-public checkout");
        return;
    };
    crate::fs::add_search_path("GAME", &core.to_string_lossy(), false);
    if device::create().is_none() {
        eprintln!("skipped: no GPU");
        return;
    }
    let mat = material::create_raw(Some(c"materials/test/cull.vmat"), Some(c"shaders/unlit.shader"), true);
    let kept = MaterialRef::keep(mat).unwrap();
    assert!(covers_centre(Vec3::new(-60.0, 0.0, 0.0), Some(&kept)), "seen from outside");
    assert!(!covers_centre(Vec3::ZERO, Some(&kept)), "from inside, only back faces: nothing drawn");
    material::set_vector(mat, "F_RENDER_BACKFACES", crate::math::vec4(1.0, 0.0, 0.0, 0.0));
    assert!(covers_centre(Vec3::ZERO, Some(&kept)), "two-sided: the back faces draw");
    drop(kept);
    crate::resource::destroy_handle(mat);
}
