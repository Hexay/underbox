use super::*;

fn px<B>(bytes: B, format: i64, stride: usize) -> Pixels<B> {
    Pixels { bytes, format, stride }
}

fn half(x: f32) -> [u8; 2] {
    half::f16::from_f32(x).to_bits().to_le_bytes()
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

#[test]
fn swaps_bgra_to_rgba_with_strides() {
    let src = [10, 20, 30, 40, 50, 60, 70, 80, 0xEE, 0xEE];
    let mut dst = [0u8; 8];
    assert!(convert_pixels(px(&src[..], 12, 5), px(&mut dst[..], 0, 4), 1, 2));
    assert_eq!(dst, [30, 20, 10, 40, 80, 70, 60, 0xEE], "the 5th byte of row 0 is padding");
    assert!(!convert_pixels(px(&src[..4], 12, 0), px(&mut dst[..], 0, 0), 1, 2), "source too short");
}

#[test]
fn eight_bit_orders_and_grey() {
    let mut rgb = [0u8; 3];
    assert!(convert_pixels(px(&[1, 2, 3, 9][..], 16, 0), px(&mut rgb[..], 2, 0), 1, 1));
    assert_eq!(rgb, [3, 2, 1], "BGRX drops its X");
    let mut rgba = [0u8; 4];
    assert!(convert_pixels(px(&[200][..], 5, 0), px(&mut rgba[..], 0, 0), 1, 1));
    assert_eq!(rgba, [200, 200, 200, 255]);
    let mut grey = [0u8; 1];
    assert!(convert_pixels(px(&[30, 20, 10, 40][..], 0, 0), px(&mut grey[..], 5, 0), 1, 1));
    assert_eq!(grey[0], (0.299f32 * 30.0 + 0.587 * 20.0 + 0.114 * 10.0).round() as u8);
    assert!(!convert_pixels(px(&[0; 8][..], 13, 0), px(&mut [0; 64][..], 0, 0), 4, 4), "DXT1 isn't converted");
}

/// An HDR swap chain's readback (RGBA16F) to RGBA8888: clamped and rounded, no transfer.
#[test]
fn half_floats_narrow_to_eight_bit() {
    let src: Vec<u8> = [half(1.0), half(0.5), half(0.0), half(2.0)].concat();
    let mut dst = [0u8; 4];
    assert!(convert_pixels(px(&src[..], 24, 0), px(&mut dst[..], 0, 0), 1, 1));
    assert_eq!(dst, [255, 128, 0, 255]);
    let mut r16f = [0u8; 2];
    assert!(convert_pixels(px(&src[..], 24, 0), px(&mut r16f[..], 59, 0), 1, 1));
    assert_eq!(r16f, half(1.0), "R16F keeps red");
}

#[test]
fn sixteen_bit_and_float_round_trips() {
    let rgba8 = [255u8, 0, 51, 128];
    let mut u16s = [0u8; 8];
    assert!(convert_pixels(px(&rgba8[..], 0, 0), px(&mut u16s[..], 25, 0), 1, 1));
    assert_eq!(u16s, [0xFF, 0xFF, 0, 0, 0x33, 0x33, 0x80, 0x80], "8 to 16 bits replicates the byte");
    let mut f32s = [0u8; 16];
    assert!(convert_pixels(px(&u16s[..], 25, 0), px(&mut f32s[..], 29, 0), 1, 1));
    assert_eq!(floats(&f32s), [1.0, 0.0, 0.2, 128.0 / 255.0]);
    let mut r32 = [0u8; 4];
    assert!(convert_pixels(px(&f32s[..], 29, 0), px(&mut r32[..], 27, 0), 1, 1));
    assert_eq!(floats(&r32), [1.0]);
    let mut back = [0u8; 4];
    assert!(convert_pixels(px(&f32s[..], 29, 0), px(&mut back[..], 0, 0), 1, 1));
    assert_eq!(back, rgba8);
    let mut from_r32 = [0u8; 16];
    assert!(convert_pixels(px(&3.5f32.to_le_bytes()[..], 27, 0), px(&mut from_r32[..], 29, 0), 1, 1));
    assert_eq!(floats(&from_r32), [3.5, 0.0, 0.0, 1.0], "missing channels read 0, alpha 1; HDR kept");
}

#[test]
fn slot_checks_pointers_and_strides() {
    let src = [1u8, 2, 3, 4];
    let mut dst = [0u8; 4];
    let (s, d) = (RawPtr::from_raw(src.as_ptr().cast_mut().cast()), RawPtr::from_raw(dst.as_mut_ptr().cast()));
    assert!(convert(s, 12, d, 0, 1, 1, 0, 0));
    assert_eq!(dst, [3, 2, 1, 4]);
    assert!(!convert(RawPtr::from_raw(std::ptr::null_mut()), 12, d, 0, 1, 1, 0, 0));
    assert!(!convert(s, 12, d, 13, 1, 1, 0, 0), "to DXT1");
}
