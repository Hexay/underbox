//! Read-back frames and the frame-hash harness: exact hashes per driver, tolerant comparison
//! against a reference everywhere. See docs/renderer.md, "Frame-hash tests".

use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub texel_size: u32,
    /// Tightly packed rows, top row first.
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub texels: usize,
    /// Texels where any channel differs by more than the tolerance.
    pub differing: usize,
    pub max_delta: u8,
}

impl Comparison {
    pub fn differing_fraction(&self) -> f64 {
        self.differing as f64 / self.texels.max(1) as f64
    }
}

impl Frame {
    /// A frame filled with one texel value.
    pub fn filled(width: u32, height: u32, texel: &[u8]) -> Frame {
        let pixels = texel.repeat((width * height) as usize);
        Frame { width, height, texel_size: texel.len() as u32, pixels }
    }

    pub fn texel(&self, x: u32, y: u32) -> &[u8] {
        let at = ((y * self.width + x) * self.texel_size) as usize;
        &self.pixels[at..at + self.texel_size as usize]
    }

    pub fn set_texel(&mut self, x: u32, y: u32, value: &[u8]) {
        let at = ((y * self.width + x) * self.texel_size) as usize;
        self.pixels[at..at + self.texel_size as usize].copy_from_slice(value);
    }

    /// Fill texels whose centres lie in `[x0, x1) × [y0, y1)` (pixel coordinates, top-left origin).
    pub fn fill_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, value: &[u8]) {
        for y in 0..self.height {
            for x in 0..self.width {
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                if cx >= x0 && cx < x1 && cy >= y0 && cy < y1 {
                    self.set_texel(x, y, value);
                }
            }
        }
    }

    /// blake3 of the dimensions and pixels, as hex.
    pub fn hash(&self) -> String {
        let mut h = blake3::Hasher::new();
        h.update(&self.width.to_le_bytes());
        h.update(&self.height.to_le_bytes());
        h.update(&self.texel_size.to_le_bytes());
        h.update(&self.pixels);
        h.finalize().to_hex().to_string()
    }

    /// Per-channel comparison; a texel differs when any channel is more than `tolerance` apart.
    pub fn compare(&self, other: &Frame, tolerance: u8) -> Comparison {
        assert_eq!((self.width, self.height, self.texel_size), (other.width, other.height, other.texel_size), "frame shapes differ");
        let ts = self.texel_size as usize;
        let mut cmp = Comparison { texels: self.pixels.len() / ts.max(1), differing: 0, max_delta: 0 };
        for (a, b) in self.pixels.chunks(ts).zip(other.pixels.chunks(ts)) {
            let delta = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
            cmp.max_delta = cmp.max_delta.max(delta);
            if delta > tolerance {
                cmp.differing += 1;
            }
        }
        cmp
    }

    /// Write an RGBA8 frame as binary PPM (alpha dropped), for eyeballing a failed test.
    pub fn write_ppm(&self, path: &Path) -> std::io::Result<()> {
        let mut out = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        for t in self.pixels.chunks(self.texel_size as usize) {
            out.extend_from_slice(&t[..3.min(t.len())]);
        }
        std::fs::write(path, out)
    }

    /// Check this frame's hash against `golden` (lines of `name<TAB>adapter<TAB>hash`). Hashes are
    /// exact only per driver, so an adapter with no entry passes unchecked unless
    /// `UNDERBOX_BLESS=1`, which records it. Returns whether a recorded hash was checked.
    pub fn check_golden(&self, golden: &Path, name: &str, adapter: &str) -> bool {
        let text = std::fs::read_to_string(golden).unwrap_or_default();
        let hash = self.hash();
        let recorded = text.lines().find_map(|l| {
            let mut f = l.split('\t');
            (f.next() == Some(name) && f.next() == Some(adapter)).then(|| f.next().unwrap_or("").to_string())
        });
        if std::env::var_os("UNDERBOX_BLESS").is_some() {
            let mut lines: Vec<String> = text.lines().filter(|l| !l.starts_with(&format!("{name}\t{adapter}\t"))).map(String::from).collect();
            lines.push(format!("{name}\t{adapter}\t{hash}"));
            lines.sort();
            std::fs::write(golden, lines.join("\n") + "\n").expect("write golden hashes");
            return true;
        }
        match recorded {
            Some(want) => {
                assert_eq!(hash, want, "frame '{name}' on '{adapter}' changed (UNDERBOX_BLESS=1 re-records)");
                true
            }
            None => false,
        }
    }
}
