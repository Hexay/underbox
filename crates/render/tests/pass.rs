//! The generic draw path: meshes in arbitrary layouts (missing attributes from the defaults
//! buffer), 16-bit indices, viewports, depth-only targets and region read-back.

use glam::Mat4;
use render::pass::{self, DrawUniforms, Load, MeshDraw, Targets};
use render::{Buffer, BufferKind, Frame, Gpu, Placeholder, RasterState, Semantic, Texture, TextureDesc, wgpu};
use std::sync::OnceLock;

const SIZE: u32 = 32;

fn gpu() -> &'static Gpu {
    static GPU: OnceLock<Gpu> = OnceLock::new();
    GPU.get_or_init(|| Gpu::headless().expect("a wgpu adapter (install mesa-vulkan-drivers for lavapipe)"))
}

/// A full-screen quad (clip space, z = 0.5) as bare positions with u16 indices.
fn quad(gpu: &Gpu) -> (Buffer, Buffer) {
    let p: [f32; 12] = [-1.0, -1.0, 0.5, 1.0, -1.0, 0.5, 1.0, 1.0, 0.5, -1.0, 1.0, 0.5];
    let i: [u16; 6] = [0, 1, 2, 0, 2, 3];
    (Buffer::with_data(gpu, "quad", BufferKind::Vertex, bytemuck::cast_slice(&p)), Buffer::with_data(gpu, "quad", BufferKind::Index, bytemuck::cast_slice(&i)))
}

fn draw<'a>(v: &'a Buffer, i: &'a Buffer, tint: [f32; 4]) -> MeshDraw<'a> {
    MeshDraw {
        vertices: (&v.buffer, 0),
        vertex: Placeholder::vertex_keys(12, &[(Semantic::Position, wgpu::VertexFormat::Float32x3, 0)]),
        indices: Some((&i.buffer, wgpu::IndexFormat::Uint16)),
        range: 0..6,
        base_vertex: 0,
        instances: 1,
        uniforms: DrawUniforms { mvp: Mat4::IDENTITY, model: Mat4::IDENTITY, tint },
        state: RasterState::default(),
        shaded: None,
        indirect: None,
    }
}

#[test]
fn position_only_mesh_in_a_viewport() {
    let gpu = gpu();
    let placeholder = Placeholder::new(gpu);
    let color = Texture::new(gpu, TextureDesc::render_target("c", SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm));
    let (v, i) = quad(gpu);
    let half = SIZE as f32 / 2.0;
    let targets = Targets { color: Some(&color), viewport: Some([0.0, 0.0, half, SIZE as f32, 0.0, 1.0]), ..Default::default() };
    pass::execute(gpu, &placeholder, &targets, Load { color: Some(wgpu::Color::BLACK), depth: None }, &[draw(&v, &i, [1.0, 0.0, 0.0, 1.0])]);
    let mut want = Frame::filled(SIZE, SIZE, &[0, 0, 0, 255]);
    want.fill_rect(0.0, 0.0, half, SIZE as f32, &[255, 0, 0, 255]);
    assert_eq!(color.read_back(gpu).compare(&want, 1).differing, 0);

    let region = color.read_region(gpu, 0, [half as u32 - 2, 3, 4, 2]).unwrap();
    assert_eq!((region.width, region.height), (4, 2));
    assert_eq!((region.texel(1, 0), region.texel(2, 1)), (&[255, 0, 0, 255][..], &[0, 0, 0, 255][..]));
    assert!(color.read_region(gpu, 0, [30, 0, 4, 1]).is_none(), "out of bounds");
}

#[test]
fn srgb_write_encodes_into_an_8_bit_target() {
    let gpu = gpu();
    let placeholder = Placeholder::new(gpu);
    let color = Texture::new(gpu, TextureDesc::render_target("c", SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm));
    let (v, i) = quad(gpu);
    let grey = |srgb_write: bool| {
        let targets = Targets { color: Some(&color), srgb_write, ..Default::default() };
        pass::execute(gpu, &placeholder, &targets, Load::default(), &[draw(&v, &i, [0.5, 0.5, 0.5, 1.0])]);
        color.read_back(gpu).texel(4, 4)[0]
    };
    assert_eq!(grey(false), 128);
    assert!((187..=188).contains(&grey(true)), "0.5 linear is sRGB 187.5");
}

#[test]
fn depth_only_pass_writes_depth() {
    let gpu = gpu();
    let placeholder = Placeholder::new(gpu);
    let depth = Texture::new(gpu, TextureDesc::depth("d", SIZE, SIZE));
    let (v, i) = quad(gpu);
    let targets = Targets { depth: Some(&depth), ..Default::default() };
    pass::execute(gpu, &placeholder, &targets, Load { color: None, depth: Some(0.0) }, &[draw(&v, &i, [1.0; 4])]);
    let frame = depth.read_back(gpu);
    assert_eq!(f32::from_le_bytes(frame.texel(5, 5).try_into().unwrap()), 0.5);
}

#[test]
fn a_pass_that_neither_clears_nor_draws_is_skipped() {
    let gpu = gpu();
    let placeholder = Placeholder::new(gpu);
    let color = Texture::new(gpu, TextureDesc::render_target("c", SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm));
    let targets = Targets { color: Some(&color), ..Default::default() };
    pass::execute(gpu, &placeholder, &targets, Load { color: Some(wgpu::Color::RED), depth: None }, &[]);
    pass::execute(gpu, &placeholder, &targets, Load::default(), &[]);
    assert_eq!(color.read_back(gpu).texel(1, 1), &[255, 0, 0, 255][..], "an empty clear still clears");
}
