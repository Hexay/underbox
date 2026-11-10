//! Photoshop `.psd`: the merged (flattened) image only, grayscale or RGB, 8/16/32 bits, raw or
//! PackBits. Layers are skipped, as the texture compiler reads only the composite.

use super::FloatBitmap;

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = (self.0.get(..n)?, self.0.get(n..)?);
        self.0 = rest;
        Some(head)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn skip_section(&mut self) -> Option<()> {
        let n = self.u32()? as usize;
        self.take(n).map(drop)
    }
}

/// PackBits: a count byte n, then n+1 literals (n < 128) or one byte repeated 257-n times.
fn unpack_bits(mut src: &[u8], len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        let (&n, rest) = src.split_first()?;
        src = rest;
        match n {
            0..=127 => {
                let n = n as usize + 1;
                out.extend_from_slice(src.get(..n)?);
                src = &src[n..];
            }
            128 => {}
            _ => {
                let (&b, rest) = src.split_first()?;
                src = rest;
                out.extend(std::iter::repeat_n(b, 257 - n as usize));
            }
        }
    }
    out.truncate(len);
    Some(out)
}

pub fn decode(bytes: &[u8]) -> Option<FloatBitmap> {
    let mut r = Reader(bytes);
    if r.take(4)? != b"8BPS" || r.u16()? != 1 {
        return None;
    }
    r.take(6)?;
    let channels = r.u16()? as usize;
    let (h, w) = (r.u32()? as usize, r.u32()? as usize);
    let depth = r.u16()? as usize;
    let mode = r.u16()?;
    let colour_channels = match mode {
        1 => 1,
        3 => 3,
        _ => return None,
    };
    if channels < colour_channels || !matches!(depth, 8 | 16 | 32) {
        return None;
    }
    (0..3).try_for_each(|_| r.skip_section())?;

    let row = w * depth / 8;
    let planes: Vec<Vec<u8>> = match r.u16()? {
        0 => (0..channels).map(|_| r.take(row * h).map(<[u8]>::to_vec)).collect::<Option<_>>()?,
        1 => {
            let counts: Vec<usize> = (0..channels * h).map(|_| r.u16().map(usize::from)).collect::<Option<_>>()?;
            let mut planes = vec![Vec::with_capacity(row * h); channels];
            for (i, &n) in counts.iter().enumerate() {
                planes[i / h].extend(unpack_bits(r.take(n)?, row)?);
            }
            planes
        }
        _ => return None,
    };
    let sample = |plane: &[u8], i: usize| -> f32 {
        match depth {
            8 => f32::from(plane[i]) / 255.0,
            16 => f32::from(u16::from_be_bytes([plane[2 * i], plane[2 * i + 1]])) / 65535.0,
            _ => f32::from_be_bytes(plane[4 * i..4 * i + 4].try_into().unwrap()),
        }
    };
    let texels = (0..w * h)
        .map(|i| {
            let c = |k: usize| sample(&planes[k], i);
            let a = if channels > colour_channels { c(colour_channels) } else { 1.0 };
            if colour_channels == 1 { [c(0), c(0), c(0), a] } else { [c(0), c(1), c(2), a] }
        })
        .collect();
    Some(FloatBitmap::from_texels(w, h, texels))
}

/// An uncompressed 8-bit RGBA `.psd`, for tests.
#[cfg(test)]
pub fn encode_rgba8(w: usize, h: usize, rgba: &[[u8; 4]]) -> Vec<u8> {
    let mut out = b"8BPS".to_vec();
    out.extend(1u16.to_be_bytes());
    out.extend([0; 6]);
    out.extend(4u16.to_be_bytes());
    out.extend((h as u32).to_be_bytes());
    out.extend((w as u32).to_be_bytes());
    out.extend(8u16.to_be_bytes());
    out.extend(3u16.to_be_bytes());
    out.extend([0; 12]);
    out.extend(0u16.to_be_bytes());
    (0..4).for_each(|c| out.extend(rgba.iter().map(|t| t[c])));
    out
}
