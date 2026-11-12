//! Core `tonemapping/tonemapping.shader` as managed's `Tonemapping` effect blits it: a fullscreen
//! triangle at z = 1 over a depth buffer the scene already filled. Its `RenderState( DepthEnable,
//! false )` (render_state.rs) must let it through, and `g_flToneMapScalarLinear` (the view's
//! exposure) must scale it. Skips without a GPU or an sbox-public checkout.

use super::{program, tests::core_dir, vertex};
use crate::mesh::layout::{Field, Layout, format};
use crate::render::draw::{self, Draw, Target, shaded::Shaded};
use crate::render::lighting::Lighting;
use crate::render::{device, material, texture};
use abi::structs::TextureCreationConfig_t;
use glam::Mat4;
use render::pass::{DrawUniforms, Load};
use render::{BufferKind, RasterState};
use std::collections::HashMap;
use std::sync::Arc;

const PATH: &str = "shaders/tonemapping/tonemapping.shader";

/// The centre texel after tonemapping a flat (64, 64, 64) source at `exposure`.
fn tonemapped(exposure: f32) -> Option<[u8; 4]> {
    tonemapped_with(exposure, &shaders::Variant::default())
}

/// [`tonemapped`] in a dynamic combo variant (`D_TONEMAPPING` is the first combo).
fn tonemapped_with(exposure: f32, variant: &shaders::Variant) -> Option<[u8; 4]> {
    let (core, gpu) = (core_dir()?, device::create()?);
    let shader = shaders::VfxShader::parse_file(&core.join(PATH), &shaders::IncludePaths::new([core])).unwrap();
    let s = Arc::new(program::build(gpu, &shaders::Compiler::new().unwrap(), &shader, PATH, variant).unwrap());
    let fields = vec![
        Field { semantic: "position".into(), index: 0, format: format::RGB32F, offset: 0 },
        Field { semantic: "texcoord".into(), index: 0, format: format::RG32F, offset: 12 },
    ];
    let inputs = vertex::inputs(&s.vertex_inputs, &Layout { name: "blit".into(), stride: 20, fields }).expect("the triangle feeds the blit");
    let tri: [f32; 15] = [-1.0, -1.0, 1.0, 0.0, 1.0, 3.0, -1.0, 1.0, 2.0, 1.0, -1.0, 3.0, 1.0, 0.0, -1.0];
    let bytes: Vec<u8> = tri.iter().flat_map(|f| f.to_le_bytes()).collect();

    let mut config = TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (1, 1, 1, 1);
    let src = texture::create(Some(c"tonemap source"), true, config, &mut [64, 64, 64, 255]);
    let mat = material::create_raw(Some(c"tonemap test"), Some(c"shaders/tonemapping/tonemapping.shader"), true);
    material::set_texture(mat, "ColorBuffer", src);
    let instance = super::bind::Instance { world: Mat4::IDENTITY, tint: [1.0; 4] };
    let shaded = Shaded { shading: s, vertex: inputs.keys, material: material::resolve(mat).unwrap(), values: HashMap::new(), instance, view_proj: Mat4::IDENTITY, context: Default::default() };
    let d = Draw {
        vertices: Arc::new(render::Buffer::with_data(gpu, "blit", BufferKind::Vertex, &bytes)),
        vertex_offset: 0,
        vertex: Vec::new(),
        indices: None,
        range: 0..3,
        base_vertex: 0,
        instances: 1,
        uniforms: DrawUniforms { mvp: Mat4::IDENTITY, model: Mat4::IDENTITY, tint: [1.0; 4] },
        // What RenderTools.Draw gives every immediate draw: depth tested and written.
        state: RasterState::default(),
        shaded: Some(shaded),
        transforms: None,
    };
    let (color, depth) = (texture::create_target("tonemap", 8, 8, 0), texture::create_target("tonemap depth", 8, 8, 62));
    let lighting = Lighting { tone_map: Some(exposure), ..Default::default() };
    draw::execute(&Target { color, depth, ..Default::default() }, &lighting, Load { color: Some(render::wgpu::Color::BLACK), depth: Some(0.5) }, &[d]);
    let texel = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(gpu).texel(4, 4).try_into().unwrap();
    [color, depth, src].into_iter().for_each(crate::resource::destroy_handle);
    crate::resource::destroy_handle(mat);
    Some(texel)
}

#[test]
fn tonemap_blit_passes_scene_depth_and_applies_exposure() {
    let Some(once) = tonemapped(1.0) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let twice = tonemapped(2.0).unwrap();
    assert!(once[0].abs_diff(64) <= 2, "the blit drew over scene depth: {once:?}");
    assert!(twice[0].abs_diff(128) <= 2, "exposure 2 doubles it: {twice:?}");
}

/// Hable filmic (`D_TONEMAPPING` 1, walker's camera): Uncharted 2's curve over its value at the
/// white point 11.2.
#[test]
fn tonemap_hable_divides_by_the_white_point() {
    let Some(texel) = tonemapped_with(4.0, &shaders::Variant::new(0, 1)) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let partial = |x: f32| (x * (0.15 * x + 0.1 * 0.5) + 0.2 * 0.02) / (x * (0.15 * x + 0.5) + 0.2 * 0.3) - 0.02 / 0.3;
    let x = 4.0 * render::srgb::decode(64.0 / 255.0);
    // Encoded as the flat test above reads its 64 back.
    let want = (render::srgb::encode(partial(x) / partial(11.2)) * 255.0).round() as u8;
    assert!(texel[0].abs_diff(want) <= 2, "Hable of {x}: want {want}, got {texel:?}");
}
