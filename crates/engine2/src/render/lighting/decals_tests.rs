use super::*;

fn decal(sort_order: u32) -> Decal {
    Decal { rotation: Quat::from_rotation_y(0.5), scale: Vec3::new(8.0, 64.0, 32.0), tint: Vec4::ONE, color: 3, sort_order, sampler: -1, attenuation: 1.0, color_mix: 1.0, ..Default::default() }
}

/// Managed's `Color.ToRgbe` for white: mantissa 64 + 128, exponent 1 + 128.
#[test]
fn white_packs_as_managed_rgbe() {
    assert_eq!(rgbe(Vec3::ONE), (192 | 192 << 8 | 192 << 16, 129));
    assert_eq!(rgbe(Vec3::ZERO), (0, 0));
    let (rgb, exp) = rgbe(Vec3::new(1.0, 0.35, 0.1));
    let back = |m: u32| (m as f32 - 128.0) * (exp as f32 - 135.0).exp2();
    assert!((back(rgb >> 8 & 0xFF) - 0.35).abs() < 0.01, "{rgb:x}");
}

#[test]
fn gpu_decal_projects_world_into_the_unit_cube() {
    let mut d = decal(0);
    d.position = Vec3::new(10.0, 20.0, 30.0);
    let packed = Decals::pack(vec![d.clone()]);
    let w = packed.gpu.iter().map(|w| f32::from_bits(*w)).collect::<Vec<_>>();
    let (offset, q, scale) = (Vec3::new(w[0], w[1], w[2]), Quat::from_xyzw(w[3], w[4], w[5], w[6]), Vec3::new(w[7], w[8], w[9]));
    let corner = d.position + d.rotation * (Vec3::new(0.5, -0.5, 0.5) * d.scale);
    assert!(((q * (corner + offset)) * scale).distance(Vec3::new(0.5, -0.5, 0.5)) < 1e-4);
    assert_eq!((packed.gpu[10] & 0xFFFF, packed.gpu[11] >> 24, packed.gpu[15]), (3, 255, u32::MAX));
}

/// Extras only for a decal that needs one; sorted by sort order, stable among equals.
#[test]
fn extras_and_sort_order() {
    let multiply = Decal { blend_mode: 1, ..decal(5) };
    let first = Decal { color: 7, ..decal(1) };
    let second = Decal { color: 9, ..decal(1) };
    let packed = Decals::pack(vec![multiply, first, second]);
    assert_eq!(packed.count, 3);
    assert_eq!([packed.gpu[10] & 0xFFFF, packed.gpu[16 + 10] & 0xFFFF], [7, 9]);
    assert_eq!(packed.gpu[32 + 15], 0);
    assert_eq!(packed.extra.len(), EXTRA_WORDS);
    assert_eq!((packed.extra[4], packed.extra[6], packed.extra[9]), (0xFF_FFFF, 1, u32::MAX));
}
