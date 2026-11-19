//! A face is the scene test's camera render (scene/tests.rs) in its array layer; the others stay.

use super::render;
use crate::render::scene::{camera, tests};
use crate::render::texture::{self, TSPEC_CUBE_TEXTURE};
use crate::render::device;
use abi::structs::TextureCreationConfig_t;
use render::wgpu;

/// `TSPEC_RENDER_TARGET`.
const RENDER_TARGET: u32 = 0x0001;
const RED: wgpu::Color = wgpu::Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };

/// Layer `face` of `cube`, read back through a 2D copy.
fn face(cube: texture::Ref, face: u32) -> render::Frame {
    let gpu = device::gpu().unwrap();
    let c = texture::resolve(cube).unwrap();
    let c = c.gpu.as_ref().unwrap();
    let plain = render::Texture::new(gpu, render::TextureDesc::render_target("face", c.desc.width, c.desc.height, c.desc.format));
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let src = wgpu::TexelCopyTextureInfo { texture: &c.texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: face }, aspect: wgpu::TextureAspect::All };
    encoder.copy_texture_to_texture(src, plain.texture.as_image_copy(), wgpu::Extent3d { width: c.desc.width, height: c.desc.height, depth_or_array_layers: 1 });
    gpu.submit(encoder.finish());
    plain.read_back(gpu)
}

#[test]
fn each_call_renders_one_face() {
    device::create().expect("a wgpu adapter");
    let (_world, cam) = tests::scene();
    let mut config = TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (tests::SIZE as i16, tests::SIZE as i16, 1, 1);
    config.m_n_flags = TSPEC_CUBE_TEXTURE | RENDER_TARGET;
    let cube = texture::create(Some(c"cube"), true, config, &mut []);
    let gpu = device::gpu().unwrap();
    for layer in 0..6 {
        texture::resolve(cube).unwrap().gpu.as_ref().unwrap().clear_layer(gpu, RED, 0.0, layer);
    }
    let plain = texture::create_target("plain", tests::SIZE, tests::SIZE, 0);
    assert!(camera::render_to_texture(cam, plain, Default::default(), Default::default()));
    render(cam, cube, 3);
    render(cam, cube, 6);
    assert_eq!(face(cube, 3).pixels, tests::read(plain).pixels, "face 3 is the camera's render");
    for other in [0, 1, 2, 4, 5] {
        assert_eq!(face(cube, other).texel(32, 32), [255, 0, 0, 255], "face {other} untouched");
    }
    for t in [cube, plain] {
        crate::resource::destroy_handle(t);
    }
}
