use super::*;

/// Steady-state RMS per ear, in dB, of a 440 Hz sine at peak 0.5 through one effect.
fn levels(direction: [f32; 3], interpolation: Interpolation, blend: f32) -> [f64; 2] {
    let mut fx = Binaural::new().expect("default HRTF");
    let mut acc = [0.0f64; 2];
    let mut n = 0;
    for block in 0..20 {
        let mut input = [[0.0f32; FRAME]];
        for (i, s) in input[0].iter_mut().enumerate() {
            let t = (block * FRAME + i) as f64 / RATE as f64;
            *s = (0.5 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as f32;
        }
        let mut out = [[0.0f32; FRAME]; 2];
        assert!(fx.apply(direction, interpolation, blend, &mut input, &mut out));
        if block >= 4 {
            for (a, ear) in acc.iter_mut().zip(&out) {
                *a += ear.iter().map(|&v| (v as f64).powi(2)).sum::<f64>();
            }
            n += FRAME;
        }
    }
    acc.map(|a| 10.0 * (a / n as f64).log10())
}

fn near(got: [f64; 2], want: [f64; 2]) -> bool {
    got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.02)
}

#[test]
fn ahead_matches_real_engine2s_capture() {
    // Real's `ahead200` and `ui` in corpus/sound (docs/differential-audio.md).
    let ahead = levels([0.0, 0.0, -1.0], Interpolation::Bilinear, 1.0);
    assert!(near(ahead, [-12.50, -12.33]), "{ahead:?}");
}

#[test]
fn nearest_lookup_ignores_spatial_blend_for_mono() {
    let local = levels([0.0, 0.0, -1.0], Interpolation::Nearest, 0.1);
    assert!(near(local, [-12.50, -12.33]), "{local:?}");
    let dry = levels([0.0, 0.0, -1.0], Interpolation::Bilinear, 0.0);
    assert!(near(dry, [-9.03, -9.03]), "bilinear blends to the dry signal: {dry:?}");
}

#[test]
fn a_source_on_the_left_is_louder_in_the_left_ear() {
    let [l, r] = levels([-1.0, 0.0, 0.0], Interpolation::Bilinear, 1.0);
    assert!(l - r > 4.0, "{l} {r}");
}
