//! 8-bit planar YCbCr to RGBA8, fixed point.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Matrix {
    Bt601,
    Bt709,
}

impl Matrix {
    /// What a stream that doesn't say is most likely: BT.709 from 720p up, else BT.601.
    pub fn guess(height: usize) -> Self {
        if height >= 720 { Self::Bt709 } else { Self::Bt601 }
    }
}

/// Planes of one picture. Empty `u`/`v` is greyscale.
pub struct Planes<'a> {
    pub y: &'a [u8],
    pub u: &'a [u8],
    pub v: &'a [u8],
    pub y_stride: usize,
    pub uv_stride: usize,
    pub width: usize,
    pub height: usize,
    /// Chroma halved horizontally / vertically (4:2:0 is both).
    pub ss_x: bool,
    pub ss_y: bool,
}

/// (luma scale, Cr→R, Cb→G, Cr→G, Cb→B) in 16.16.
fn coefficients(m: Matrix, full_range: bool) -> [i32; 5] {
    let f = |v: f64| (v * 65536.0).round() as i32;
    let (kr, kb) = match m {
        Matrix::Bt601 => (0.299, 0.114),
        Matrix::Bt709 => (0.2126, 0.0722),
    };
    let kg = 1.0 - kr - kb;
    let (luma, chroma) = if full_range { (1.0, 1.0) } else { (255.0 / 219.0, 255.0 / 224.0) };
    let cr_r = 2.0 * (1.0 - kr) * chroma;
    let cb_b = 2.0 * (1.0 - kb) * chroma;
    [f(luma), f(cr_r), f(cb_b * kb / kg), f(cr_r * kr / kg), f(cb_b)]
}

pub fn to_rgba(p: &Planes, m: Matrix, full_range: bool) -> Vec<u8> {
    let [luma, cr_r, cb_g, cr_g, cb_b] = coefficients(m, full_range);
    let y_off = if full_range { 0 } else { 16 };
    let grey = p.u.is_empty() || p.v.is_empty();
    let mut out = vec![255u8; p.width * p.height * 4];
    for (row, line) in out.chunks_exact_mut(p.width * 4).enumerate() {
        let ys = &p.y[row * p.y_stride..];
        let crow = if p.ss_y { row / 2 } else { row } * p.uv_stride;
        for (x, px) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let y = (ys[x] as i32 - y_off) * luma;
            let (cb, cr) = if grey {
                (0, 0)
            } else {
                let cx = crow + if p.ss_x { x / 2 } else { x };
                (p.u[cx] as i32 - 128, p.v[cx] as i32 - 128)
            };
            let clamp = |v: i32| ((v + 32768) >> 16).clamp(0, 255) as u8;
            px[0] = clamp(y + cr_r * cr);
            px[1] = clamp(y - cb_g * cb - cr_g * cr);
            px[2] = clamp(y + cb_b * cb);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(y: u8, u: u8, v: u8, m: Matrix, full: bool) -> [u8; 4] {
        let p = Planes { y: &[y], u: &[u], v: &[v], y_stride: 1, uv_stride: 1, width: 1, height: 1, ss_x: false, ss_y: false };
        to_rgba(&p, m, full).try_into().unwrap()
    }

    #[test]
    fn limited_range_white_black_and_primaries() {
        assert_eq!(pixel(235, 128, 128, Matrix::Bt601, false), [255, 255, 255, 255]);
        assert_eq!(pixel(16, 128, 128, Matrix::Bt709, false), [0, 0, 0, 255]);
        // BT.601 red is (81, 90, 240).
        let [r, g, b, _] = pixel(81, 90, 240, Matrix::Bt601, false);
        assert!(r >= 253 && g <= 2 && b <= 2, "{r} {g} {b}");
        // BT.709 blue is (32, 240, 118).
        let [r, g, b, _] = pixel(32, 240, 118, Matrix::Bt709, false);
        assert!(r <= 2 && g <= 2 && b >= 253, "{r} {g} {b}");
        assert_eq!(pixel(128, 128, 128, Matrix::Bt601, true), [128, 128, 128, 255]);
    }

    #[test]
    fn subsampled_chroma_covers_two_by_two() {
        let p = Planes { y: &[235; 4], u: &[128], v: &[240], y_stride: 2, uv_stride: 1, width: 2, height: 2, ss_x: true, ss_y: true };
        let rgba = to_rgba(&p, Matrix::Bt601, false);
        assert!(rgba.chunks(4).all(|px| px == rgba[..4].as_ref()) && rgba[0] == 255 && rgba[1] < 200);
    }
}
