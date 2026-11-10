//! Image files to and from [`FloatBitmap`]: PNG, JPEG, TGA, TIFF, BMP and EXR through `image`,
//! PSD and PFM by hand.

use super::{FloatBitmap, Gamma, psd};
use image::{DynamicImage, ImageFormat};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Png,
    Jpeg,
    Tga,
    Tiff,
    Bmp,
    Exr,
    Psd,
    Pfm,
}

impl Kind {
    pub fn from_extension(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "png" => Kind::Png,
            "jpg" | "jpeg" => Kind::Jpeg,
            "tga" => Kind::Tga,
            "tif" | "tiff" => Kind::Tiff,
            "bmp" => Kind::Bmp,
            "exr" => Kind::Exr,
            "psd" => Kind::Psd,
            "pfm" => Kind::Pfm,
            _ => return None,
        })
    }

    /// From the file's magic bytes; TGA has none.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"8BPS") {
            return Some(Kind::Psd);
        }
        if bytes.starts_with(b"PF\n") || bytes.starts_with(b"Pf\n") {
            return Some(Kind::Pfm);
        }
        Some(match image::guess_format(bytes).ok()? {
            ImageFormat::Png => Kind::Png,
            ImageFormat::Jpeg => Kind::Jpeg,
            ImageFormat::Tiff => Kind::Tiff,
            ImageFormat::Bmp => Kind::Bmp,
            ImageFormat::OpenExr => Kind::Exr,
            _ => return None,
        })
    }

    fn is_hdr(self) -> bool {
        matches!(self, Kind::Exr | Kind::Pfm)
    }
}

/// Decode `bytes` as `kind`; LDR colour is converted from `gamma` to linear.
pub fn load(bytes: &[u8], kind: Kind, gamma: Gamma) -> Option<FloatBitmap> {
    let mut bmp = match kind {
        Kind::Psd => psd::decode(bytes)?,
        Kind::Pfm => read_pfm(bytes)?,
        _ => {
            let format = match kind {
                Kind::Png => ImageFormat::Png,
                Kind::Jpeg => ImageFormat::Jpeg,
                Kind::Tga => ImageFormat::Tga,
                Kind::Tiff => ImageFormat::Tiff,
                Kind::Bmp => ImageFormat::Bmp,
                _ => ImageFormat::OpenExr,
            };
            let img = image::load_from_memory_with_format(bytes, format).ok()?.to_rgba32f();
            let (w, h) = (img.width() as usize, img.height() as usize);
            FloatBitmap::from_texels(w, h, img.pixels().map(|p| p.0).collect())
        }
    };
    if !kind.is_hdr() && gamma != Gamma::Linear {
        bmp.texels.iter_mut().for_each(|t| (0..3).for_each(|c| t[c] = gamma.to_linear(t[c])));
    }
    Some(bmp)
}

/// Load a file, by its extension or else its contents.
pub fn load_path(path: &Path, gamma: Gamma) -> Option<FloatBitmap> {
    let bytes = std::fs::read(path).ok()?;
    let kind = Kind::from_extension(path).or_else(|| Kind::sniff(&bytes))?;
    load(&bytes, kind, gamma)
}

/// Portable float map: `PF` (RGB) or `Pf` (grey), size, scale (negative: little-endian), then
/// rows bottom to top.
fn read_pfm(bytes: &[u8]) -> Option<FloatBitmap> {
    let mut fields = Vec::new();
    let mut pos = 0;
    while fields.len() < 4 {
        while bytes.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        let start = pos;
        while !bytes.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        fields.push(std::str::from_utf8(&bytes[start..pos]).ok()?);
    }
    let data = bytes.get(pos + 1..)?;
    let channels = match fields[0] {
        "PF" => 3,
        "Pf" => 1,
        _ => return None,
    };
    let (w, h): (usize, usize) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
    let little = fields[3].parse::<f32>().ok()? < 0.0;
    let floats: Vec<f32> = data
        .as_chunks::<4>()
        .0
        .iter()
        .take(w * h * channels)
        .map(|&b| if little { f32::from_le_bytes(b) } else { f32::from_be_bytes(b) })
        .collect();
    if floats.len() < w * h * channels {
        return None;
    }
    let mut texels = vec![[0.0; 4]; w * h];
    for (i, px) in floats.chunks_exact(channels).enumerate() {
        let (x, y) = (i % w, h - 1 - i / w);
        texels[x + y * w] = if channels == 3 { [px[0], px[1], px[2], 1.0] } else { [px[0], px[0], px[0], 1.0] };
    }
    Some(FloatBitmap::from_texels(w, h, texels))
}

pub fn write_pfm(bmp: &FloatBitmap) -> Vec<u8> {
    let mut out = format!("PF\n{} {}\n-1.0\n", bmp.width, bmp.height).into_bytes();
    for y in (0..bmp.height).rev() {
        for x in 0..bmp.width {
            bmp.texel(x, y, 0)[..3].iter().for_each(|v| out.extend(v.to_le_bytes()));
        }
    }
    out
}

/// Uncompressed 32-bit TGA, top row first, texels clamped to 0..1 without gamma.
pub fn write_tga(bmp: &FloatBitmap) -> Vec<u8> {
    let mut out = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    out.extend((bmp.width as u16).to_le_bytes());
    out.extend((bmp.height as u16).to_le_bytes());
    out.extend([32, 0x28]);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    for t in &bmp.texels[..bmp.width * bmp.height] {
        out.extend([byte(t[2]), byte(t[1]), byte(t[0]), byte(t[3])]);
    }
    out
}

/// RGBA float EXR. Compression is the encoder's choice.
pub fn write_exr(bmp: &FloatBitmap) -> Option<Vec<u8>> {
    let raw: Vec<f32> = bmp.texels[..bmp.width * bmp.height].iter().flatten().copied().collect();
    let img = image::Rgba32FImage::from_raw(bmp.width as u32, bmp.height as u32, raw)?;
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba32F(img).write_to(&mut out, ImageFormat::OpenExr).ok()?;
    Some(out.into_inner())
}
