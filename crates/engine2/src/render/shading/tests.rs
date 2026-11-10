//! The material path end to end on a real device: translate sbox-public's scene_renderer_mesh and
//! the stand-in, build their programs, match a mesh layout, bind every reflected resource without
//! a validation error, and draw a cube through the stand-in. Skips without a GPU or an
//! sbox-public checkout.

use super::{bind, program, standin, vertex, view::View};
use crate::mesh::layout::Layout;
use crate::model::content::Content;
use crate::render::draw::{self, Target, model, shaded::Shaded};
use crate::render::lighting::Lighting;
use crate::render::{device, material, texture};
use glam::{Mat4, Vec3, Vec4};
use render::RasterState;
use render::pass::{DrawUniforms, Load};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) fn core_dir() -> Option<PathBuf> {
    let env = std::env::var_os("SBOX_PUBLIC").map(PathBuf::from);
    let beside = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().map(|a| a.join("sbox-public")).find(|p| p.join("game/core/shaders").is_dir());
    env.or(beside).map(|p| p.join("game/core")).filter(|p| p.is_dir())
}

#[test]
fn scene_renderer_mesh_builds_and_binds() {
    let (Some(core), Ok(gpu)) = (core_dir(), render::Gpu::headless()) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let includes = shaders::IncludePaths::new([core.clone()]);
    let shader = shaders::VfxShader::parse_file(&core.join("shaders/scene_renderer_mesh.shader"), &includes).unwrap();
    let compiler = shaders::Compiler::new().unwrap();
    let s = program::build(&gpu, &compiler, &shader, "scene_renderer_mesh", &shaders::Variant::default()).unwrap();

    let inputs = vertex::inputs(&s.vertex_inputs, &Layout::sbox_vertex()).expect("mesh layout feeds the shader");
    assert!(!inputs.keys.is_empty());

    let view = View { view_proj: Mat4::IDENTITY, viewport: [0.0, 0.0, 64.0, 64.0], target: [64.0, 64.0] };
    let instances = [bind::Instance { world: Mat4::IDENTITY, tint: [1.0; 4] }];
    let (textures, values) = (HashMap::new(), HashMap::new());
    let lighting = Lighting::fallback();
    let draw = bind::Inputs { view: &view, lighting: &lighting, instances: &instances, textures: &textures, values: &[&values], attributes: &[], slots: &[] };
    let groups = gpu.checked(|| bind::bind_groups(&gpu, &s, &draw)).unwrap().unwrap();
    assert_eq!(groups.len(), s.program.bind_group_layouts.len());
}

/// The stand-in's 16-unit cube seen from above and -X: red-tinted in the middle (the view and
/// instance matrices reach the translated program the right way round), lit on top.
#[test]
fn standin_draws_a_tinted_cube() {
    let Some(core) = core_dir() else {
        eprintln!("skipped: no sbox-public checkout");
        return;
    };
    let Some(gpu) = device::create() else {
        eprintln!("skipped: no GPU");
        return;
    };
    let shader = standin::shader(&shaders::IncludePaths::new([core])).unwrap();
    let compiler = shaders::Compiler::new().unwrap();
    let s = Arc::new(program::build(gpu, &compiler, &shader, standin::PATH, &shaders::Variant::default()).unwrap());
    let inputs = vertex::inputs(&s.vertex_inputs, &Layout::sbox_vertex()).expect("mesh layout feeds the stand-in");
    let view_proj = render::Camera::perspective(90f32.to_radians(), 1.0, 1.0, Vec3::new(-30.0, 0.0, 30.0), Vec3::ZERO).view_proj;
    let red = [1.0, 0.0, 0.0, 1.0];
    let content = Content::from_meshes(&[assets::model::stand_in()]);
    let mesh = &content.meshes[0].mesh;
    // The placeholder would draw it green: a red result proves the stand-in drew.
    let uniforms = DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, tint: [0.0, 1.0, 0.0, 1.0] };
    let mut d = model::draw_call(mesh, &mesh.draw_calls()[0], uniforms, view_proj, RasterState::default()).unwrap();
    let mat = material::load_path("materials/test/standin.vmat");
    let values = HashMap::from([("UncompressedTangentFrame".to_owned(), Vec4::ONE)]);
    let instance = bind::Instance { world: Mat4::IDENTITY, tint: red };
    d.shaded = Some(Shaded { shading: s, vertex: inputs.keys, material: material::resolve(mat).unwrap(), values, instance, view_proj, context: Default::default() });

    let (color, depth) = (texture::create_target("standin", 64, 64, 0), texture::create_target("standin depth", 64, 64, 62));
    let target = Target { color, depth, ..Default::default() };
    draw::execute(&target, &Lighting::fallback(), Load { color: Some(render::wgpu::Color::BLACK), depth: Some(1.0) }, &[d]);
    let frame = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(gpu);
    assert_eq!(frame.texel(0, 0), [0, 0, 0, 255], "background");
    let [r, g, b, _] = frame.texel(32, 32)[..4].try_into().unwrap();
    assert!(r > 60 && g < 10 && b < 10, "red cube face in the middle, got {:?}", [r, g, b]);
    // The +Z face is lit (0.3 + 0.7 · 0.85), the -X face only ambient: normals reach the light.
    let reds: Vec<u8> = (0..64).flat_map(|y| (0..64).map(move |x| (x, y))).map(|(x, y)| frame.texel(x, y)[0]).collect();
    assert!(reds.iter().any(|r| *r > 200) && reds.iter().any(|r| (60..100).contains(r)), "a lit and an unlit face");
    crate::resource::destroy_handle(color);
    crate::resource::destroy_handle(depth);
    crate::resource::destroy_handle(mat);
}
