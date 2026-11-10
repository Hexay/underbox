use super::file::{self, Kind};
use super::{FloatBitmap, Gamma, bc, format, psd};

/// 3 wide, 2 tall; red holds the texel index, alpha 1.
fn indexed() -> FloatBitmap {
    FloatBitmap::from_texels(3, 2, (0..6).map(|i| [i as f32, 0.0, 0.0, 1.0]).collect())
}

fn reds(b: &FloatBitmap) -> Vec<f32> {
    b.texels.iter().map(|t| t[0]).collect()
}

#[test]
fn rotations_and_mirrors() {
    // 0 1 2
    // 3 4 5
    let mut b = indexed();
    b.rotate_90_cw();
    assert_eq!((b.width, b.height, reds(&b)), (2, 3, vec![3.0, 0.0, 4.0, 1.0, 5.0, 2.0]));
    let mut b = indexed();
    b.rotate_90_ccw();
    assert_eq!(reds(&b), vec![2.0, 5.0, 1.0, 4.0, 0.0, 3.0]);
    let mut b = indexed();
    b.rotate_180();
    assert_eq!(reds(&b), vec![5.0, 4.0, 3.0, 2.0, 1.0, 0.0]);
    let mut b = indexed();
    b.mirror_horizontally();
    assert_eq!(reds(&b), vec![2.0, 1.0, 0.0, 5.0, 4.0, 3.0]);
    let mut b = indexed();
    b.mirror_vertically();
    assert_eq!(reds(&b), vec![3.0, 4.0, 5.0, 0.0, 1.0, 2.0]);
}

#[test]
fn pixel_addressing() {
    let b = indexed();
    assert_eq!(b.pixel(1, 1, 0, 0), 4.0);
    assert_eq!(b.pixel(3, 0, 0, 0), 0.0, "outside reads zero");
    assert_eq!(b.pixel_wrapped(-1, 0, 0, 0), 2.0);
    assert_eq!(b.pixel_wrapped(4, 3, 0, 0), 4.0);
    assert_eq!(b.pixel_clamped(9, -4, 0, 0), 2.0);
    assert_eq!(b.pixel(0, 0, 0, 3), 1.0);
}

#[test]
fn resize_averages_down_and_interpolates_up() {
    let mut flat = FloatBitmap::from_texels(5, 3, vec![[0.25, 0.5, 0.75, 1.0]; 15]);
    flat.resize_2d(8, 2, true);
    assert!(flat.texels.iter().all(|t| t.iter().zip([0.25, 0.5, 0.75, 1.0]).all(|(a, b)| (a - b).abs() < 1e-5)));

    let mut checker = FloatBitmap::from_texels(4, 4, (0..16).map(|i| [((i % 4 + i / 4) % 2) as f32; 4]).collect());
    checker.resize_2d(2, 2, true);
    assert!(checker.texels.iter().all(|t| (t[0] - 0.5).abs() < 1e-5), "{:?}", checker.texels);

    let mut ramp = FloatBitmap::from_texels(2, 1, vec![[0.0; 4], [1.0; 4]]);
    ramp.resize_2d(4, 1, true);
    let r = reds(&ramp);
    assert!(r.windows(2).all(|w| w[0] <= w[1]) && r[0] == 0.0 && r[3] == 1.0, "{r:?}");
}

#[test]
fn raw_formats_round_trip() {
    let src = FloatBitmap::from_texels(2, 1, vec![[1.0, 0.5, 0.0, 0.25], [0.0, 0.2, 0.4, 1.0]]);
    for fmt in [0, 1, 11, 12, 24, 25, 29] {
        let bytes = format::encode(&src, fmt, false).unwrap();
        assert_eq!(Some(bytes.len()), format::byte_size(fmt, 2, 1));
        let back = format::decode(&bytes, 2, 1, fmt, Gamma::Linear).unwrap();
        for (a, b) in back.texels.iter().flatten().zip(src.texels.iter().flatten()) {
            assert!((a - b).abs() < 3e-3, "format {fmt}: {a} vs {b}");
        }
    }
    assert_eq!(format::encode(&src, 0, false).unwrap(), vec![255, 128, 0, 64, 0, 51, 102, 255]);
    assert_eq!(format::encode(&src, 12, false).unwrap()[..4], [0, 128, 255, 64], "BGRA");
    assert_eq!(format::encode(&src, 2, false).unwrap().len(), 6, "RGB888 drops alpha");
    let grey = format::decode(&[200], 1, 1, 5, Gamma::Linear).unwrap();
    assert_eq!(grey.texels[0][1], 200.0 / 255.0);
    assert_eq!(grey.texels[0][3], 1.0, "no stored alpha reads opaque");
    assert!(format::encode(&src, 85, false).is_none(), "BC6H isn't encoded");
}

#[test]
fn srgb_round_trip() {
    let src = FloatBitmap::from_texels(1, 1, vec![[0.2159, 0.5, 1.0, 0.5]]);
    let bytes = format::encode(&src, 0, true).unwrap();
    assert_eq!(bytes[0], 128, "linear 0.2159 is sRGB 128/255");
    assert_eq!(bytes[3], 128, "alpha stays linear");
    let back = format::decode(&bytes, 1, 1, 0, Gamma::Srgb).unwrap();
    assert!((back.texels[0][0] - 0.2159).abs() < 3e-3);
}

#[test]
fn block_compression() {
    let solid = [[200, 100, 50, 255]; 16];
    let decoded = bc::decode_bc1(&bc::bc1(&solid));
    assert!(decoded.iter().all(|c| c.iter().zip([200, 100, 50]).all(|(a, b)| a.abs_diff(b) <= 4)));

    let ramp: bc::Block = std::array::from_fn(|i| [(i * 16) as u8, 255 - (i * 16) as u8, 30, 255]);
    let decoded = bc::decode_bc1(&bc::bc1(&ramp));
    for (d, s) in decoded.iter().zip(&ramp) {
        // 4 entries over a 240 range: half a step (40) is only reachable if green followed red's diagonal.
        assert!(d[0].abs_diff(s[0]) <= 44 && d[1].abs_diff(s[1]) <= 44, "{d:?} vs {s:?}");
    }

    let alpha: [u8; 16] = std::array::from_fn(|i| (i * 17) as u8);
    let decoded = bc::decode_bc4(&bc::bc4(&alpha));
    assert!(decoded.iter().zip(&alpha).all(|(a, b)| a.abs_diff(*b) <= 19));

    let img = FloatBitmap::from_texels(5, 3, vec![[1.0; 4]; 15]);
    assert_eq!(format::encode(&img, 13, false).unwrap().len(), 2 * 8, "partial blocks round up");
    assert_eq!(format::encode(&img, 15, false).unwrap().len(), 2 * 16);
}

#[test]
fn files_round_trip() {
    let src = FloatBitmap::from_texels(2, 2, vec![[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 0.5], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0, 0.0]]);
    let same = |b: &FloatBitmap, tol: f32| b.texels.iter().flatten().zip(src.texels.iter().flatten()).all(|(a, b)| (a - b).abs() <= tol);
    let byte = 1.0 / 255.0;

    let tga = file::load(&file::write_tga(&src), Kind::Tga, Gamma::Linear).unwrap();
    assert!(same(&tga, byte), "{tga:?}");
    let exr = file::load(&file::write_exr(&src).unwrap(), Kind::Exr, Gamma::Linear).unwrap();
    assert!(same(&exr, 1e-6));
    let pfm = file::load(&file::write_pfm(&src), Kind::Pfm, Gamma::Linear).unwrap();
    assert!(pfm.texels.iter().zip(&src.texels).all(|(a, b)| a[..3] == b[..3]), "PFM keeps colour only");

    let rgba8: Vec<[u8; 4]> = src.texels.iter().map(|t| t.map(|v| (v * 255.0) as u8)).collect();
    let psd = file::load(&psd::encode_rgba8(2, 2, &rgba8), Kind::Psd, Gamma::Linear).unwrap();
    assert!(same(&psd, byte));

    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(2, 2, rgba8.concat()).unwrap().write_to(&mut png, image::ImageFormat::Png).unwrap();
    let png = png.into_inner();
    assert_eq!(Kind::sniff(&png), Some(Kind::Png));
    assert!(same(&file::load(&png, Kind::Png, Gamma::Linear).unwrap(), byte));
    let srgb = file::load(&png, Kind::Png, Gamma::Srgb).unwrap();
    assert_eq!(srgb.texels[3][3], 0.0, "gamma leaves alpha alone");
    assert!(file::load(b"garbage", Kind::Png, Gamma::Linear).is_none());
}
