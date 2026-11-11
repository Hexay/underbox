use super::*;
use abi::class;
use abi::structs::Color32;

fn target(size: u32) -> texture::Ref {
    device::create().expect("a wgpu adapter (lavapipe on headless Linux)");
    texture::create_target("readback", size, size, RGBA8888)
}

#[test]
fn sync_read_converts_and_strides() {
    let t = target(8);
    texture::clear(t, Color32 { r: 10, g: 20, b: 30, a: 255 });
    let mut src = NativeRect { x: 2, y: 2, w: 3, h: 2 };
    let mut dst = NativeRect { x: 1, y: 0, w: 3, h: 2 };
    let stride = 20;
    let mut out = vec![0u8; stride * 2];
    assert!(read_pixels(t, &mut src, 0, 0, &mut dst, RawPtr::from_raw(out.as_mut_ptr().cast()), BGRA8888.into(), stride as i32));
    assert_eq!(&out[..4], &[0; 4], "left of dstRect untouched");
    assert_eq!(&out[4..8], &[30, 20, 10, 255]);
    assert_eq!(&out[stride + 12..stride + 16], &[30, 20, 10, 255]);
    let mut whole = NativeRect::default();
    assert!(!read_pixels(t, &mut whole, 0, 3, &mut dst, RawPtr::from_raw(out.as_mut_ptr().cast()), BGRA8888.into(), 0), "no mip 3");
}

/// Managed clears plain textures too (Sandbox's physgun graph); they aren't render attachments.
#[test]
fn clearing_a_plain_texture_uploads_the_colour() {
    device::create().expect("a wgpu adapter (lavapipe on headless Linux)");
    let mut config = abi::structs::TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels, config.m_n_image_format) = (4, 1, 1, 1, RGBA8888.into());
    let t = texture::create(None, true, config, &mut []);
    texture::clear(t, Color32 { r: 5, g: 6, b: 7, a: 8 });
    let mut out = vec![0u8; 16];
    assert!(read_pixels(t, &mut NativeRect::default(), 0, 0, &mut NativeRect::default(), RawPtr::from_raw(out.as_mut_ptr().cast()), RGBA8888.into(), 16));
    assert_eq!(out, [5, 6, 7, 8].repeat(4));
}

#[test]
fn async_read_holds_bytes_until_done() {
    let t = target(4);
    texture::clear(t, Color32 { r: 1, g: 2, b: 3, a: 4 });
    let cb: NativeRef<class::CReadTexturePixelsManagedCallback> = objects::create(Pending::default());
    read_async(NativeRef::default(), t, cb.cast(), NativeRect::default(), 0, 0, true);
    let held = objects::get::<_, Pending>(cb).unwrap().data.lock().unwrap().clone();
    assert_eq!(held, [1, 2, 3, 4].repeat(16));
    done(cb);
    assert!(objects::get::<_, Pending>(cb).is_none(), "deleted when done");
}
