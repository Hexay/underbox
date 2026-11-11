//! The sRGB transfer function on single channels (blit.wgsl does the same on the GPU).

/// sRGB-encoded `c` to linear.
pub fn decode(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Linear `c` to sRGB-encoded.
pub fn encode(c: f32) -> f32 {
    let c = c.max(0.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trips() {
        for c in [0.0, 0.01, 0.25, 0.5, 0.75, 1.0] {
            assert!((super::encode(super::decode(c)) - c).abs() < 1e-5, "{c}");
        }
        assert!((super::decode(0.5) - 0.214_041).abs() < 1e-5);
    }
}
