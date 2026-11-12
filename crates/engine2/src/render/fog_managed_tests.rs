use super::{Constants, at};
use glam::{Mat4, Vec3};

/// Managed's constants for an eye at (10, 20, 30) looking down +x (right is -y, up +z), 90°
/// horizontal at 3:2, as `VolumetricFog.SetupConstants` fills them; one box volume.
fn constants() -> Constants {
    let mut w = vec![0f32; at::WIDTH_HEIGHT_DEPTH + 8];
    let (forward, right, up) = (Vec3::X, Vec3::NEG_Y, Vec3::Z);
    let (tan_x, tan_y, width, height) = (1.0, 1.0 / 1.5, 240.0, 160.0);
    let ray0 = forward - right * tan_x - up * tan_y;
    let set = |w: &mut Vec<f32>, i: usize, v: Vec3| w[i..i + 3].copy_from_slice(&v.to_array());
    set(&mut w, at::VIEW_DX, right * 2.0 * tan_x / width);
    set(&mut w, at::VIEW_DY, up * 2.0 * tan_y / height);
    set(&mut w, at::VIEW_CORNER, ray0);
    set(&mut w, at::VIEW_POS, Vec3::new(10.0, 20.0, 30.0));
    set(&mut w, at::VIEW_DIR, forward);
    w[at::NEAR_FAR..at::NEAR_FAR + 3].copy_from_slice(&[2000.0 / 64.0, 0.0, 2000.0]);
    w[at::ANISOTROPY_SCATTERING..at::ANISOTROPY_SCATTERING + 2].copy_from_slice(&[0.5, 2.0 * 0.0022]);
    // Fade from 100 to 300.
    w[at::FADE_IN_SCALE_BIAS..at::FADE_IN_SCALE_BIAS + 2].copy_from_slice(&[1.0 / 200.0, -0.5]);
    w[at::VOLUME_COUNT] = f32::from_bits(1);
    w[at::BOX_MIN..at::BOX_MIN + 4].copy_from_slice(&[-64.0, -32.0, -16.0, 1.0]);
    w[at::BOX_MAX..at::BOX_MAX + 4].copy_from_slice(&[64.0, 32.0, 16.0, 0.75]);
    w[at::BOX_COLOR_EXPONENT..at::BOX_COLOR_EXPONENT + 4].copy_from_slice(&[1.0, 0.5, 0.25, 2.0]);
    // Managed writes localToWorld.Inverted.Transpose() row by row: a translation by -(100, 0, 0).
    let m = Mat4::from_translation(Vec3::new(-100.0, 0.0, 0.0));
    w[at::BOX_WORLD_TO_VOLUME..at::BOX_WORLD_TO_VOLUME + 16].copy_from_slice(&m.transpose().to_cols_array());
    w[at::WIDTH_HEIGHT_DEPTH] = f32::from_bits(240);
    w[at::WIDTH_HEIGHT_DEPTH + 1] = f32::from_bits(160);
    Constants(w.iter().flat_map(|f| f.to_le_bytes()).collect())
}

#[test]
fn view_proj_matches_the_froxel_rays() {
    let vp = constants().view_proj().unwrap();
    let eye = Vec3::new(10.0, 20.0, 30.0);
    // 100 ahead, 50 right, 20 up: ndc x = 50 / 100, y = 20 / (100 / 1.5), w = depth.
    let clip = vp * (eye + Vec3::new(100.0, -50.0, 20.0)).extend(1.0);
    assert!((clip.w - 100.0).abs() < 1e-3, "{clip}");
    assert!((clip.x / clip.w - 0.5).abs() < 1e-5 && (clip.y / clip.w - 0.3).abs() < 1e-5, "{clip}");
    let view = crate::render::shading::view::View { view_proj: vp, ..Default::default() };
    assert!(view.camera_position().truncate().distance(eye) < 1e-2, "{}", view.camera_position());
    assert!(view.camera_axes().0.truncate().distance(Vec3::X) < 1e-4);
}

#[test]
fn volumes_and_settings_read_back() {
    let c = constants();
    let v = &c.volumes()[..];
    assert_eq!(v.len(), 1);
    assert_eq!((v[0].min, v[0].max, v[0].color), (Vec3::new(-64.0, -32.0, -16.0), Vec3::new(64.0, 32.0, 16.0), Vec3::new(1.0, 0.5, 0.25)));
    assert_eq!((v[0].strength, v[0].exponent, v[0].spherical), (0.75, 2.0, true));
    assert_eq!(v[0].world_to_volume.transform_point3(Vec3::new(100.0, 0.0, 0.0)), Vec3::ZERO);
    let s = c.settings();
    assert_eq!((s.draw_distance, s.anisotropy), (2000.0, 0.5));
    assert!((s.scattering - 2.0).abs() < 1e-5 && (s.fade_in.0 - 100.0).abs() < 1e-3 && (s.fade_in.1 - 300.0).abs() < 1e-3, "{:?} {}", s.fade_in, s.scattering);
}

/// One `GpuLight` as LightBinnerFeature packs it: rows of a VMatrix whose columns are
/// forward/left/up/position.
fn gpu_light(position: Vec3, forward: Vec3, color: Vec3, radius: f32, cone: (f32, f32), fog: f32) -> Vec<u8> {
    let mut w = [0f32; 39];
    w[0] = f32::from_bits(if cone.0 != 0.0 { 3 } else { 1 });
    let left = Vec3::Z.cross(forward).normalize_or_zero();
    let up = forward.cross(left);
    for (row, i) in [3, 7, 11].into_iter().enumerate() {
        w[i..i + 4].copy_from_slice(&[forward[row], left[row], up[row], position[row]]);
    }
    w[15..19].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
    w[19..22].copy_from_slice(&color.to_array());
    (w[25], w[26]) = (radius, radius * radius);
    (w[29], w[30]) = cone;
    w[33] = fog;
    w.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[test]
fn binned_lights_read_as_fog_lights() {
    let mut bytes = gpu_light(Vec3::new(1.0, 2.0, 3.0), Vec3::X, Vec3::new(2.0, 1.0, 0.5), 300.0, (0.0, 0.0), 0.5);
    bytes.extend(gpu_light(Vec3::new(-5.0, 0.0, 9.0), Vec3::NEG_Z, Vec3::ONE, 128.0, (0.9, 0.8), 1.0));
    // A light with fog off doesn't light the medium.
    bytes.extend(gpu_light(Vec3::ZERO, Vec3::X, Vec3::ONE, 100.0, (0.0, 0.0), 0.0));
    let lights = super::lights_from(&bytes);
    assert_eq!(lights.len(), 2);
    assert_eq!((lights[0].position, lights[0].radius, lights[0].color, lights[0].spot), (Vec3::new(1.0, 2.0, 3.0), 300.0, Vec3::new(1.0, 0.5, 0.25), None));
    assert_eq!(lights[1].spot, Some((Vec3::NEG_Z, 0.9, 0.8)));
    assert_eq!(lights[1].position, Vec3::new(-5.0, 0.0, 9.0));
}
