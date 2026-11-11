//! One texture surface to RGBA8 (LDR formats) or RGBA16F (HDR formats) on the CPU. Block formats
//! go through bcdec (BC1-BC7); single channels land in red (green too for two), as a GPU samples
//! them; I8/IA88 are greyscale. ETC2/EAC, R32_UINT and WebP aren't decoded.

use super::{Format, Texture};
use half::f16;

#[derive(Debug, Clone, PartialEq)]
pub enum Pixels {
    Rgba8(Vec<u8>),
    /// Half floats, four per texel.
    Rgba16F(Vec<u16>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Pixels,
}

/// `out` texels from 4x4 blocks: `block` decodes one block into a 4x4 array of `N`-element texels.
fn blocks<T: Copy + Default, const N: usize>(data: &[u8], w: usize, h: usize, size: usize, block: impl Fn(&[u8], &mut [T])) -> Option<Vec<[T; N]>> {
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut out = vec![[T::default(); N]; w * h];
    let mut texels = vec![T::default(); 16 * N];
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * size;
            block(data.get(at..at + size)?, &mut texels);
            for (y, x) in (0..4).flat_map(|y| (0..4).map(move |x| (y, x))) {
                let (px, py) = (bx * 4 + x, by * 4 + y);
                if px < w && py < h {
                    out[py * w + px].copy_from_slice(&texels[(y * 4 + x) * N..(y * 4 + x + 1) * N]);
                }
            }
        }
    }
    Some(out)
}

fn ldr(texels: Vec<[u8; 4]>) -> Pixels {
    Pixels::Rgba8(texels.into_iter().flatten().collect())
}

fn hdr(texels: impl Iterator<Item = [f32; 4]>) -> Pixels {
    Pixels::Rgba16F(texels.flat_map(|t| t.map(|c| f16::from_f32(c).to_bits())).collect())
}

/// Texels of `bytes`-wide channels read by `f`, `n` channels per texel, padded (0, 0, 1).
fn channels(data: &[u8], texels: usize, n: usize, bytes: usize, f: impl Fn(&[u8]) -> f32) -> Option<Pixels> {
    let stride = n * bytes;
    let texel = |i: usize| -> [f32; 4] { std::array::from_fn(|c| if c < n { f(&data[i * stride + c * bytes..][..bytes]) } else if c == 3 { 1.0 } else { 0.0 }) };
    (data.len() >= texels * stride).then(|| hdr((0..texels).map(texel)))
}

fn u16_of(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn f32_of(b: &[u8]) -> f32 {
    f32::from_le_bytes(b.try_into().unwrap())
}

fn decode_surface(format: Format, data: &[u8], w: usize, h: usize) -> Option<Pixels> {
    let n = w * h;
    let gray = |v: &[u8]| v.iter().map(|&i| [i, i, i, 255]).collect::<Vec<_>>();
    Some(match format {
        Format::DXT1 => ldr(blocks::<u8, 4>(data, w, h, 8, |b, o| bcdec_rs::bc1(b, o, 16))?.into_iter().map(|[r, g, b, _]| [r, g, b, 255]).collect()),
        Format::DXT5 => ldr(blocks::<u8, 4>(data, w, h, 16, |b, o| bcdec_rs::bc3(b, o, 16))?),
        Format::BC7 => ldr(blocks::<u8, 4>(data, w, h, 16, |b, o| bcdec_rs::bc7(b, o, 16))?),
        Format::ATI1N => ldr(blocks::<u8, 1>(data, w, h, 8, |b, o| bcdec_rs::bc4(b, o, 4, false))?.into_iter().map(|[r]| [r, 0, 0, 255]).collect()),
        Format::ATI2N => ldr(blocks::<u8, 2>(data, w, h, 16, |b, o| bcdec_rs::bc5(b, o, 8, false))?.into_iter().map(|[r, g]| [r, g, 0, 255]).collect()),
        Format::BC6H => {
            let halves = blocks::<u16, 3>(data, w, h, 16, |b, o| bcdec_rs::bc6h_half(b, o, 12, false))?;
            Pixels::Rgba16F(halves.into_iter().flat_map(|[r, g, b]| [r, g, b, f16::ONE.to_bits()]).collect())
        }
        Format::RGBA8888 => Pixels::Rgba8(data.get(..n * 4)?.to_vec()),
        Format::BGRA8888 => Pixels::Rgba8(data.get(..n * 4)?.as_chunks::<4>().0.iter().flat_map(|c| [c[2], c[1], c[0], c[3]]).collect()),
        Format::I8 => ldr(gray(data.get(..n)?)),
        Format::IA88 => ldr(data.get(..n * 2)?.as_chunks::<2>().0.iter().map(|c| [c[0], c[0], c[0], c[1]]).collect()),
        Format::A8 => ldr(data.get(..n)?.iter().map(|&a| [255, 255, 255, a]).collect()),
        Format::R8_UNORM => ldr(data.get(..n)?.iter().map(|&r| [r, 0, 0, 255]).collect()),
        Format::R16 => channels(data, n, 1, 2, |b| f32::from(u16_of(b)) / 65535.0)?,
        Format::RG1616 => channels(data, n, 2, 2, |b| f32::from(u16_of(b)) / 65535.0)?,
        Format::RGBA16161616 => channels(data, n, 4, 2, |b| f32::from(u16_of(b)) / 65535.0)?,
        Format::R16F | Format::RG1616F | Format::RGBA16161616F => {
            let c = [Format::R16F, Format::RG1616F, Format::RGBA16161616F].iter().position(|f| *f == format).map(|i| 1 << i)?;
            channels(data, n, c, 2, |b| f16::from_bits(u16_of(b)).to_f32())?
        }
        Format::R32F => channels(data, n, 1, 4, f32_of)?,
        Format::RG3232F => channels(data, n, 2, 4, f32_of)?,
        Format::RGB323232F => channels(data, n, 3, 4, f32_of)?,
        Format::RGBA32323232F => channels(data, n, 4, 4, f32_of)?,
        _ => return None,
    })
}

fn image_file(data: &[u8]) -> Option<Image> {
    let kind = crate::bitmap::file::Kind::sniff(data)?;
    let bmp = crate::bitmap::file::load(data, kind, crate::bitmap::Gamma::Linear)?;
    let texels = bmp.texels.iter().map(|t| t.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)).collect();
    Some(Image { width: bmp.width, height: bmp.height, pixels: ldr(texels) })
}

/// Face `face` of slice `slice` of mip `level`; `None` for formats we don't decode.
pub fn decode(t: &Texture, level: usize, slice: usize, face: usize) -> Option<Image> {
    if t.format.is_image_file() {
        return image_file(t.levels.first()?);
    }
    let (w, h, _) = t.level_size(level);
    let surface = t.surface(level, slice, face)?;
    Some(Image { width: w, height: h, pixels: decode_surface(t.format, surface, w, h)? })
}
