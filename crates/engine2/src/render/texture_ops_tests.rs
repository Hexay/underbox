use super::*;
use crate::render::texture::{TSPEC_TEXTURE_ARRAY, TSPEC_VOLUME_TEXTURE};
use abi::structs::{Rect3D, TextureCreationConfig_t};

const RGBA8888: i32 = 0;
const R32_UINT: i32 = 87;

fn gpu() -> &'static render::Gpu {
    device::create().expect("a wgpu adapter (lavapipe on headless Linux)")
}

fn make(size: i16, depth: i16, mips: i16, flags: u32, format: i32, data: &mut [u8]) -> texture::Ref {
    let mut config = TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (size, size, depth, mips);
    (config.m_n_flags, config.m_n_image_format) = (flags, format);
    texture::create(Some(c"texture ops"), true, config, data)
}

fn with_gpu<R>(t: texture::Ref, f: impl FnOnce(&render::Texture) -> R) -> R {
    f(texture::resolve(t).unwrap().gpu.as_ref().expect("a GPU side"))
}

/// Mip `mip`, layer `layer` of `t`, read through a copy into a fresh one-layer texture.
fn read(t: texture::Ref, mip: u32, layer: u32) -> Vec<u8> {
    let size = with_gpu(t, |g| g.mip_size(mip).0) as i16;
    let out = make(size, 1, 1, 0, RGBA8888, &mut []);
    assert!(copy(t, out, None, (0, 0), [mip, layer, 0, 0]), "copy mip {mip} layer {layer} out");
    let px = with_gpu(out, |g| g.read_back(gpu()).pixels);
    crate::resource::destroy_handle(out);
    px
}

#[test]
fn copy_texture_takes_any_mip_and_slice() {
    gpu();
    let mut layers: Vec<u8> = (0..4 * 4 * 4 * 2).map(|i| i as u8).collect();
    let src = make(4, 2, 2, TSPEC_TEXTURE_ARRAY, RGBA8888, &mut layers);
    let mip1: Vec<u8> = (0..2 * 2 * 4 * 2).map(|i| 200 - i as u8).collect();
    with_gpu(src, |g| g.upload_smaller_mips(gpu(), &mip1));
    let dst = make(4, 3, 2, TSPEC_TEXTURE_ARRAY, RGBA8888, &mut []);
    assert!(copy(src, dst, None, (0, 0), [1, 1, 1, 2]));
    assert_eq!(read(dst, 1, 2), mip1[16..], "source mip 1 slice 1 into destination mip 1 slice 2");
    assert!(copy(src, dst, Some([1, 1, 2, 2]), (2, 0), [0, 0, 0, 0]), "a rect at an offset");
    assert_eq!(read(dst, 0, 0)[8..16], layers[20..28], "source texels (1, 1) and (2, 1) at destination (2, 0)");
    assert!(!copy(src, dst, None, (0, 0), [2, 0, 0, 0]), "no mip 2");
    for t in [src, dst] {
        crate::resource::destroy_handle(t);
    }
}

#[test]
fn copy_texture_with_no_slices_copies_the_whole_resource() {
    gpu();
    let mut faces: Vec<u8> = (0..2 * 2 * 4 * 2).map(|i| i as u8 + 1).collect();
    let src = make(2, 2, 2, TSPEC_TEXTURE_ARRAY, RGBA8888, &mut faces);
    with_gpu(src, |g| g.upload_smaller_mips(gpu(), &[7; 8]));
    let dst = make(2, 2, 2, TSPEC_TEXTURE_ARRAY, RGBA8888, &mut []);
    assert!(copy(src, dst, None, (0, 0), [0; 4]));
    assert_eq!((read(dst, 0, 1), read(dst, 1, 1)), (faces[16..].to_vec(), vec![7; 4]));
    for t in [src, dst] {
        crate::resource::destroy_handle(t);
    }
}

#[test]
fn generate_mip_maps_box_filters_down_the_chain() {
    gpu();
    let mut checker: Vec<u8> = (0..16).flat_map(|i| if (i % 4 + i / 4) % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
    let t = make(4, 1, 3, 0, RGBA8888, &mut checker);
    assert!(generate_mips(t));
    for (mip, len) in [(1, 4), (2, 1)] {
        let px = read(t, mip, 0);
        assert_eq!(px.len(), len * 4);
        assert!(px.chunks(4).all(|p| p[..3].iter().all(|c| c.abs_diff(128) <= 1)), "mip {mip}: {px:?}");
    }
    crate::resource::destroy_handle(t);
}

#[test]
fn clear_texture_uint_fills_every_texel() {
    gpu();
    let t = make(4, 1, 2, 0, R32_UINT, &mut [0; 4 * 4 * 4]);
    assert!(clear_uint(t, 0xffff_fffe));
    let px = with_gpu(t, |g| g.read_region(gpu(), 1, [0, 0, 2, 2]).unwrap().pixels);
    assert!(px.chunks(4).all(|p| u32::from_le_bytes(p.try_into().unwrap()) == 0xffff_fffe), "{px:?}");
    let rgba = make(1, 1, 1, 0, RGBA8888, &mut [0; 4]);
    assert!(clear_uint(rgba, 0x1234));
    assert_eq!(with_gpu(rgba, |g| g.read_back(gpu()).pixels), [0x34; 4], "each component the value's low byte");
    for t in [t, rgba] {
        crate::resource::destroy_handle(t);
    }
}

#[test]
fn volume_textures_are_3d_on_the_gpu() {
    gpu();
    let mut texels: Vec<u8> = (0..2 * 2 * 2 * 4).map(|i| i as u8).collect();
    let t = make(2, 2, 1, TSPEC_VOLUME_TEXTURE, RGBA8888, &mut texels);
    assert_eq!(with_gpu(t, |g| g.dimension()), render::wgpu::TextureViewDimension::D3);
    assert_eq!(read(t, 0, 1), texels[16..]);
    texture::set_data(t, &[9; 4], Rect3D { x: 1, y: 0, z: 1, width: 1, height: 1, depth: 1 });
    assert_eq!(read(t, 0, 1)[4..8], [9; 4], "a box upload lands in its depth slice");
    crate::resource::destroy_handle(t);
}
