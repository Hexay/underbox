//! Compiled engine resources: models (`.vmdl_c`) and meshes (`.vmesh_c`), textures (`.vtex_c`),
//! materials (`.vmat_c`) and physics (`.vphys_c`, a model's PHYS block). Each reader has a writer
//! beside it for fixtures. Layouts and sources: research/resources.md.

pub mod anim;
pub mod fixture;
pub mod flex;
mod hull;
pub mod joint;
pub mod kv;
pub mod material;
pub mod mesh;
pub mod model;
mod model_write;
pub mod morph;
pub mod physics;
pub mod rig;
#[cfg(test)]
mod rigged_tests;
pub mod sound;
#[cfg(test)]
mod tests;
pub mod texture;
pub mod vbib;
pub mod vertex;
#[cfg(test)]
mod real_model_tests;

use crate::container::{self, Container, FourCC};
use crate::kv3bin::{self, Compression};
use ::kv::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Container(container::ReadError),
    Kv3(kv3bin::Error),
    Meshopt(crate::meshopt::Error),
    Decompress(String),
    /// A block the resource needs isn't there.
    Missing(&'static str),
    /// A resource decoded but doesn't have the shape its type needs.
    Format(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Container(e) => write!(f, "resource container: {e:?}"),
            Error::Kv3(e) => write!(f, "{e}"),
            Error::Meshopt(e) => write!(f, "{e}"),
            Error::Decompress(e) => write!(f, "decompress: {e}"),
            Error::Missing(b) => write!(f, "no {b} block"),
            Error::Format(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<container::ReadError> for Error {
    fn from(e: container::ReadError) -> Self {
        Error::Container(e)
    }
}

impl From<kv3bin::Error> for Error {
    fn from(e: kv3bin::Error) -> Self {
        Error::Kv3(e)
    }
}

impl From<crate::meshopt::Error> for Error {
    fn from(e: crate::meshopt::Error) -> Self {
        Error::Meshopt(e)
    }
}

pub fn format(message: impl Into<String>) -> Error {
    Error::Format(message.into())
}

/// The payload of a block a resource must have.
pub fn required<'a>(file: &'a [u8], kind: &FourCC, name: &'static str) -> Result<&'a [u8], Error> {
    container::block(file, kind)?.ok_or(Error::Missing(name))
}

/// A block holding binary KV3, decoded.
pub fn kv3_block(file: &[u8], kind: &FourCC, name: &'static str) -> Result<Value, Error> {
    Ok(kv3bin::read(required(file, kind, name)?)?)
}

/// The payload of the `index`th block (models address their mesh blocks by table index).
pub fn block_at(file: &[u8], index: i64) -> Result<&[u8], Error> {
    let blocks = container::blocks(file)?;
    let b = usize::try_from(index).ok().and_then(|i| blocks.get(i)).ok_or_else(|| format(format!("block index {index} out of range")))?;
    Ok(&file[b.offset..b.offset + b.len])
}

/// The resource reference list (`RERL`): (id, path) per entry.
pub fn references(file: &[u8]) -> Result<Vec<(u64, String)>, Error> {
    let Some(b) = container::block(file, b"RERL")? else { return Ok(Vec::new()) };
    let u32_at = |at: usize| b.get(at..at + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()) as usize).ok_or_else(|| format("RERL truncated"));
    let (entries, count) = (u32_at(0)?, u32_at(4)?);
    (0..count)
        .map(|i| {
            let at = entries + i * 16;
            let id = b.get(at..at + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| format("RERL truncated"))?;
            let start = at + 8 + u32_at(at + 8)?;
            let end = start + b.get(start..).and_then(|s| s.iter().position(|&c| c == 0)).ok_or_else(|| format("RERL string"))?;
            Ok((id, String::from_utf8_lossy(&b[start..end]).into_owned()))
        })
        .collect()
}

/// A reference list naming `paths` (ids are a stand-in hash: the real one is unknown, see
/// research/resource-container.md).
pub fn write_references(paths: &[&str]) -> Vec<u8> {
    let mut out: Vec<u8> = [8u32, paths.len() as u32].iter().flat_map(|w| w.to_le_bytes()).collect();
    let strings_at = out.len() + paths.len() * 16;
    let mut strings = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        let field = 8 + i * 16 + 8;
        let id = p.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
        out.extend(id.to_le_bytes());
        out.extend(((strings_at + strings.len() - field) as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        strings.extend_from_slice(p.as_bytes());
        strings.push(0);
    }
    out.extend(strings);
    out
}

/// A compiled resource: `RERL` for `refs`, then `blocks` (DATA is usually among them).
pub fn write(version: u16, refs: &[&str], blocks: Vec<(FourCC, Vec<u8>)>) -> Vec<u8> {
    let mut all = vec![(*b"RERL", write_references(refs))];
    all.extend(blocks);
    Container { version, blocks: all }.write()
}

/// Container `Version` of real's compiled models and materials. With 0, real engine2 takes the
/// blocks for pre-KV3 introspected data ("Failed to convert introspected block"); 2 it rejects.
pub const RESOURCE_VERSION: u16 = 1;

/// Binary KV3 as real's compiled resources store it (v2, LZ4): real engine2 can't read v4 or v5
/// blocks ("Unable to determine buffer encoding").
pub fn kv3(value: &Value) -> Vec<u8> {
    kv3bin::write(value, 2, Compression::Lz4)
}
