use super::*;
use crate::render::draw::{self, Load, Target};
use crate::render::lighting::Lighting;
use crate::render::{device, texture};
use glam::Vec3;

/// Whether the placeholder-drawn stand-in cube (an error model) covers the centre seen from `eye`.
fn covers_centre(eye: Vec3) -> bool {
    let content = crate::model::content::Content::from_meshes(&[assets::model::stand_in()]);
    let mesh = &content.meshes[0].mesh;
    let view_proj = render::Camera::perspective(90f32.to_radians(), 1.0, 1.0, eye, eye + Vec3::X).view_proj;
    let uniforms = DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, tint: [1.0; 4] };
    let d = cull_placeholder(draw_call(mesh, &mesh.draw_calls()[0], uniforms, view_proj, RasterState::default()).unwrap());
    assert!(d.shaded.is_none(), "no material: the placeholder draws it");
    let (color, depth) = (texture::create_target("cull", 16, 16, 0), texture::create_target("cull depth", 16, 16, 62));
    let target = Target { color, depth, ..Default::default() };
    draw::execute(&target, &Lighting::default(), Load { color: Some(render::wgpu::Color::BLACK), depth: Some(1.0) }, &[d]);
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
    assert!(covers_centre(Vec3::new(-60.0, 0.0, 0.0)), "seen from outside");
    assert!(!covers_centre(Vec3::ZERO), "from inside, only back faces: nothing drawn");
}
