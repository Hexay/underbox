//! The sky stand-in samples its material's `SkyTexture` cube along the view ray: a camera looking
//! down an axis sees that axis's face. Skips without a GPU or an sbox-public checkout.

use super::{program, standin, tests::core_dir, vertex};
use crate::mesh::layout::{Field, Layout, format};
use crate::render::draw::{self, Target, shaded::Shaded};
use crate::render::lighting::Lighting;
use crate::render::{device, material, scene::execute, texture};
use abi::structs::TextureCreationConfig_t;
use glam::{Mat4, Vec3, Vec4};
use render::pass::Load;
use std::collections::HashMap;
use std::sync::Arc;

/// Face `i` (+X, -X, +Y, -Y, +Z, -Z) is a distinct opaque colour.
const FACES: [[u8; 4]; 6] = [[200, 0, 0, 255], [0, 200, 0, 255], [0, 0, 200, 255], [200, 200, 0, 255], [0, 200, 200, 255], [200, 0, 200, 255]];

fn cube() -> texture::Ref {
    let mut config = TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (1, 1, 1, 1);
    config.m_n_flags = texture::TSPEC_CUBE_TEXTURE;
    let mut data: Vec<u8> = FACES.concat();
    texture::create(Some(c"sky cube"), true, config, &mut data)
}

#[test]
fn every_standin_builds() {
    let (Some(core), Some(gpu)) = (core_dir(), device::create()) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let compiler = shaders::Compiler::new().unwrap();
    for path in standin::ALL_PATHS {
        let shader = standin::shader(path, &shaders::IncludePaths::new([core.clone()])).unwrap().unwrap();
        program::build(gpu, &compiler, &shader, path, &shaders::Variant::default()).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
}

#[test]
fn sky_standin_samples_the_face_it_looks_at() {
    let (Some(core), Some(gpu)) = (core_dir(), device::create()) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let shader = standin::shader(standin::SKY_PATH, &shaders::IncludePaths::new([core])).unwrap().unwrap();
    let s = Arc::new(program::build(gpu, &shaders::Compiler::new().unwrap(), &shader, standin::SKY_PATH, &shaders::Variant::default()).unwrap());
    let layout = Layout { name: "sky".into(), stride: 12, fields: vec![Field { semantic: "position".into(), index: 0, format: format::RGB32F, offset: 0 }] };
    let inputs = vertex::inputs(&s.vertex_inputs, &layout).expect("the triangle feeds the sky");
    let sky = cube();
    let mat = material::create_raw(Some(c"sky test"), Some(c"shaders/sky.shader"), true);
    material::set_texture(mat, "SkyTexture", sky);
    for (toward, face) in [(Vec3::X, 0), (Vec3::NEG_X, 1), (Vec3::Z, 4)] {
        let view_proj = render::Camera::perspective(60f32.to_radians(), 1.0, 1.0, Vec3::ZERO, toward).view_proj;
        let mut d = execute::sky([1.0; 4], None, view_proj, Default::default()).unwrap();
        let instance = super::bind::Instance { world: Mat4::IDENTITY, tint: [1.0; 4] };
        let values = HashMap::from([("g_vSkyTint".to_owned(), Vec4::ONE)]);
        d.shaded = Some(Shaded { shading: s.clone(), vertex: inputs.keys.clone(), material: material::resolve(mat).unwrap(), values, instance, view_proj, context: Default::default(), cached_mesh: None });
        let color = texture::create_target("sky", 16, 16, 0);
        draw::execute(&Target { color, ..Default::default() }, &Lighting::fallback(), Load { color: Some(render::wgpu::Color::BLACK), depth: None }, &[d]);
        let frame = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(gpu);
        // Bilinear on 1×1 faces blends a little of the neighbours in, even at a face's centre.
        let got = frame.texel(8, 8);
        assert!(got.iter().zip(FACES[face]).all(|(a, b)| a.abs_diff(b) <= 12), "looking {toward}: {got:?} vs face {face}");
        crate::resource::destroy_handle(color);
    }
    crate::resource::destroy_handle(mat);
    crate::resource::destroy_handle(sky);
}
