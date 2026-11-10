//! Float RGBA images (Source 2's `FloatBitMap_t`): load from image files and raw texel buffers,
//! transform, resample, and encode into `ImageFormat` texels (bitmap lib's job in engine2).

pub(crate) mod bc;
pub mod file;
pub mod format;
mod psd;
mod resample;
#[cfg(test)]
mod tests;

/// Four float channels (r, g, b, a) per texel, x fastest, then y, then z.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FloatBitmap {
    pub width: usize,
    pub height: usize,
    pub depth: usize,
    pub texels: Vec<[f32; 4]>,
}

/// `FBMGammaType_t`: the gamma an LDR source is stored in. Texels are kept linear.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gamma {
    Linear,
    Srgb,
    Power(f32),
}

impl Gamma {
    pub fn from_native(v: i64) -> Self {
        match v {
            2 => Gamma::Srgb,
            3 => Gamma::Power(1.8),
            4 => Gamma::Power(2.0),
            5 => Gamma::Power(2.2),
            6 => Gamma::Power(2.5),
            _ => Gamma::Linear,
        }
    }

    pub fn to_linear(self, v: f32) -> f32 {
        match self {
            Gamma::Linear => v,
            Gamma::Srgb if v <= 0.04045 => v / 12.92,
            Gamma::Srgb => ((v + 0.055) / 1.055).powf(2.4),
            Gamma::Power(g) => v.max(0.0).powf(g),
        }
    }

    pub fn from_linear(self, v: f32) -> f32 {
        match self {
            Gamma::Linear => v,
            Gamma::Srgb if v <= 0.0031308 => v * 12.92,
            Gamma::Srgb => 1.055 * v.powf(1.0 / 2.4) - 0.055,
            Gamma::Power(g) => v.max(0.0).powf(1.0 / g),
        }
    }
}

impl FloatBitmap {
    /// All texels zero.
    pub fn new(width: usize, height: usize, depth: usize) -> Self {
        Self { width, height, depth, texels: vec![[0.0; 4]; width * height * depth] }
    }

    pub fn from_texels(width: usize, height: usize, texels: Vec<[f32; 4]>) -> Self {
        debug_assert_eq!(texels.len(), width * height);
        Self { width, height, depth: 1, texels }
    }

    fn index(&self, x: usize, y: usize, z: usize) -> usize {
        x + self.width * (y + self.height * z)
    }

    pub fn texel(&self, x: usize, y: usize, z: usize) -> [f32; 4] {
        self.texels[self.index(x, y, z)]
    }

    /// One channel, or 0 outside the image.
    pub fn pixel(&self, x: i32, y: i32, z: i32, c: i32) -> f32 {
        let inside = |v: i32, n: usize| usize::try_from(v).ok().filter(|&v| v < n);
        match (inside(x, self.width), inside(y, self.height), inside(z, self.depth), inside(c, 4)) {
            (Some(x), Some(y), Some(z), Some(c)) => self.texel(x, y, z)[c],
            _ => 0.0,
        }
    }

    pub fn pixel_wrapped(&self, x: i32, y: i32, z: i32, c: i32) -> f32 {
        let wrap = |v: i32, n: usize| if n == 0 { 0 } else { v.rem_euclid(n as i32) };
        self.pixel(wrap(x, self.width), wrap(y, self.height), wrap(z, self.depth), c)
    }

    pub fn pixel_clamped(&self, x: i32, y: i32, z: i32, c: i32) -> f32 {
        let clamp = |v: i32, n: usize| v.clamp(0, (n as i32 - 1).max(0));
        self.pixel(clamp(x, self.width), clamp(y, self.height), clamp(z, self.depth), c)
    }

    pub fn set_channel(&mut self, c: usize, value: f32) {
        if c < 4 {
            self.texels.iter_mut().for_each(|t| t[c] = value);
        }
    }

    /// Rebuild each slice as `w`x`h` with texel (x, y) taken from `src(x, y)` of the old slice.
    fn remap(&mut self, w: usize, h: usize, src: impl Fn(usize, usize) -> (usize, usize)) {
        let mut out = Vec::with_capacity(w * h * self.depth);
        for z in 0..self.depth {
            for y in 0..h {
                for x in 0..w {
                    let (sx, sy) = src(x, y);
                    out.push(self.texel(sx, sy, z));
                }
            }
        }
        (self.width, self.height, self.texels) = (w, h, out);
    }

    pub fn rotate_90_cw(&mut self) {
        let h = self.height;
        self.remap(self.height, self.width, |x, y| (y, h - 1 - x));
    }

    pub fn rotate_90_ccw(&mut self) {
        let w = self.width;
        self.remap(self.height, self.width, |x, y| (w - 1 - y, x));
    }

    pub fn rotate_180(&mut self) {
        let (w, h) = (self.width, self.height);
        self.remap(w, h, |x, y| (w - 1 - x, h - 1 - y));
    }

    /// Left and right swap.
    pub fn mirror_horizontally(&mut self) {
        let (w, h) = (self.width, self.height);
        self.remap(w, h, |x, y| (w - 1 - x, y));
    }

    /// Top and bottom swap.
    pub fn mirror_vertically(&mut self) {
        let (w, h) = (self.width, self.height);
        self.remap(w, h, |x, y| (x, h - 1 - y));
    }

    /// Resample each slice to `w`x`h`; edges clamp, or wrap when `clamp` is false.
    pub fn resize_2d(&mut self, w: usize, h: usize, clamp: bool) {
        *self = resample::resize(self, w, h, clamp);
    }
}
