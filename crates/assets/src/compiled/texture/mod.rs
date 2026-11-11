//! Compiled textures (`.vtex_c`): a 40-byte `VTEX` header in DATA, extra-data entries, and the
//! pixels after the DATA block to the end of the file: mips smallest first, each mip's cube faces
//! and array slices back to back, optionally LZ4-compressed per mip. [`decode`] turns a mip into
//! RGBA8 or RGBA16F texels.

mod decode;
mod format;

pub use decode::{Image, Pixels, decode};
pub use format::{Format, mip_size};

use super::{Error, format as error};
use crate::container;

pub mod flags {
    pub const CLAMP_S: u16 = 0x1;
    pub const CLAMP_T: u16 = 0x2;
    pub const CLAMP_U: u16 = 0x4;
    pub const NO_LOD: u16 = 0x8;
    pub const CUBE: u16 = 0x10;
    pub const VOLUME: u16 = 0x20;
    pub const ARRAY: u16 = 0x40;
}

const HEADER: usize = 40;
const METADATA: u32 = 3;
const COMPRESSED_MIP_SIZE: u32 = 4;
const LZ4: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Texture {
    pub flags: u16,
    pub reflectivity: [f32; 4],
    pub width: u16,
    pub height: u16,
    /// Volume depth or array slice count.
    pub depth: u16,
    pub format: Format,
    /// The part of the image that's real (non-power-of-two sources), when the METADATA says.
    pub display: Option<(u16, u16)>,
    /// Each mip's bytes, largest (mip 0) first, decompressed. Image-file formats hold the file
    /// as their only level.
    pub levels: Vec<Vec<u8>>,
}

impl Texture {
    pub fn is_cube(&self) -> bool {
        self.flags & flags::CUBE != 0
    }

    /// Faces per slice: 6 for cubemaps.
    pub fn faces(&self) -> usize {
        if self.is_cube() { 6 } else { 1 }
    }

    /// (width, height, depth) of mip `level`; depth only shrinks for volumes.
    pub fn level_size(&self, level: usize) -> (usize, usize, usize) {
        let at = |s: u16| (usize::from(s) >> level).max(1);
        let depth = if self.flags & flags::VOLUME != 0 { at(self.depth) } else { usize::from(self.depth.max(1)) };
        (at(self.width), at(self.height), depth)
    }

    /// The bytes of one face of one slice (or volume layer) of mip `level`.
    pub fn surface(&self, level: usize, slice: usize, face: usize) -> Option<&[u8]> {
        let (w, h, depth) = self.level_size(level);
        let size = mip_size(self.format, w, h, 1);
        let i = slice * self.faces() + face;
        if face >= self.faces() || slice >= depth {
            return None;
        }
        self.levels.get(level)?.get(i * size..(i + 1) * size)
    }

    /// Bytes mip `level` takes uncompressed.
    fn level_bytes(&self, level: usize) -> usize {
        let (w, h, d) = self.level_size(level);
        mip_size(self.format, w, h, d * self.faces())
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, Error> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| error("VTEX truncated"))
}

/// (type, payload) of each extra-data entry; offsets count from their own field.
fn extras(data: &[u8]) -> Result<Vec<(u32, &[u8])>, Error> {
    let first = 32 + u32_at(data, 32)? as usize;
    (0..u32_at(data, 36)? as usize)
        .map(|i| {
            let at = first + i * 12;
            let start = at + 4 + u32_at(data, at + 4)? as usize;
            let payload = data.get(start..start + u32_at(data, at + 8)? as usize).ok_or_else(|| error("VTEX extra data out of bounds"))?;
            Ok((u32_at(data, at)?, payload))
        })
        .collect()
}

/// Per-mip stored sizes (mip 0 first) when the mips are LZ4-compressed.
fn compressed_sizes(extra: &[u8]) -> Result<Option<Vec<usize>>, Error> {
    if u32_at(extra, 0)? != LZ4 {
        return Ok(None);
    }
    let first = 4 + u32_at(extra, 4)? as usize;
    (0..u32_at(extra, 8)? as usize).map(|i| u32_at(extra, first + i * 4).map(|s| s as usize)).collect::<Result<_, _>>().map(Some)
}

pub fn read(file: &[u8]) -> Result<Texture, Error> {
    let block = container::blocks(file)?.into_iter().find(|b| &b.kind == b"DATA").ok_or(Error::Missing("DATA"))?;
    let data = &file[block.offset..block.offset + block.len];
    if data.len() < HEADER || u16_at(data, 0) != 1 {
        return Err(error("not a version 1 VTEX header"));
    }
    let f32_at = |at: usize| f32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    let mut t = Texture {
        flags: u16_at(data, 2),
        reflectivity: [f32_at(4), f32_at(8), f32_at(12), f32_at(16)],
        width: u16_at(data, 20),
        height: u16_at(data, 22),
        depth: u16_at(data, 24),
        format: Format(data[26]),
        ..Texture::default()
    };
    let mips = usize::from(data[27].max(1));
    let mut sizes = None;
    for (kind, payload) in extras(data)? {
        match kind {
            METADATA if payload.len() >= 6 => {
                let (w, h) = (u16_at(payload, 2), u16_at(payload, 4));
                t.display = (w > 0 && h > 0 && w <= t.width && h <= t.height).then_some((w, h));
            }
            COMPRESSED_MIP_SIZE => sizes = compressed_sizes(payload)?,
            _ => {}
        }
    }
    let pixels = &file[block.offset + block.len..];
    if t.format.is_image_file() {
        t.levels = vec![pixels.to_vec()];
        return Ok(t);
    }
    let mut at = 0;
    let mut levels = vec![Vec::new(); mips];
    for level in (0..mips).rev() {
        let size = t.level_bytes(level);
        let stored = sizes.as_ref().and_then(|s| s.get(level).copied()).filter(|&s| s < size).unwrap_or(size);
        let bytes = pixels.get(at..at + stored).ok_or_else(|| error(format!("mip {level} runs past the end of the file")))?;
        levels[level] = if stored < size { lz4_flex::block::decompress(bytes, size).map_err(|e| Error::Decompress(e.to_string()))? } else { bytes.to_vec() };
        at += stored;
    }
    t.levels = levels;
    Ok(t)
}

/// A `.vtex_c` holding `t`; mips are LZ4-compressed when `compress` and it saves space.
pub fn write(t: &Texture, compress: bool) -> Vec<u8> {
    let stored: Vec<Vec<u8>> = t
        .levels
        .iter()
        .map(|l| match lz4_flex::block::compress(l) {
            c if compress && c.len() < l.len() => c,
            _ => l.clone(),
        })
        .collect();
    let mut extra: Vec<(u32, Vec<u8>)> = Vec::new();
    if let Some((w, h)) = t.display {
        let mut m = vec![0u8; 128];
        m[2..4].copy_from_slice(&w.to_le_bytes());
        m[4..6].copy_from_slice(&h.to_le_bytes());
        extra.push((METADATA, m));
    }
    if compress && !t.format.is_image_file() {
        let mut m: Vec<u8> = [LZ4, 8, stored.len() as u32].iter().flat_map(|v| v.to_le_bytes()).collect();
        stored.iter().for_each(|s| m.extend((s.len() as u32).to_le_bytes()));
        extra.push((COMPRESSED_MIP_SIZE, m));
    }
    let mut data = Vec::with_capacity(HEADER);
    data.extend(1u16.to_le_bytes());
    data.extend(t.flags.to_le_bytes());
    t.reflectivity.iter().for_each(|f| data.extend(f.to_le_bytes()));
    [t.width, t.height, t.depth].iter().for_each(|v| data.extend(v.to_le_bytes()));
    data.extend([t.format.0, t.levels.len() as u8]);
    data.extend(0u32.to_le_bytes());
    data.extend(8u32.to_le_bytes());
    data.extend((extra.len() as u32).to_le_bytes());
    let mut payload_at = HEADER + extra.len() * 12;
    for (i, (kind, payload)) in extra.iter().enumerate() {
        let field = HEADER + i * 12 + 4;
        data.extend(kind.to_le_bytes());
        data.extend(((payload_at - field) as u32).to_le_bytes());
        data.extend((payload.len() as u32).to_le_bytes());
        payload_at += payload.len();
    }
    extra.iter().for_each(|(_, p)| data.extend(p));
    let mut file = super::write(1, &[], vec![(*b"DATA", data)]);
    stored.iter().rev().for_each(|s| file.extend(s));
    file
}
