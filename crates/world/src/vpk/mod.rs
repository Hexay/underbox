//! Valve pak files (VPK v1 and v2): a directory tree of files grouped by extension then folder,
//! whose data sits after the tree (archive index 0x7FFF) or in numbered chunk files beside a
//! `<name>_dir.vpk`. Layout: research/world.md (VPK).

mod crc;
mod write;

pub use crc::crc32;
pub use write::Builder;

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const SIGNATURE: u32 = 0x55AA_1234;
/// The archive index of data stored in the directory file itself, after the tree.
pub const INLINE: u16 = 0x7FFF;
const TERMINATOR: u16 = 0xFFFF;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Io(String),
    Signature(u32),
    Version(u32),
    Truncated,
    /// The named file's data doesn't match its CRC.
    Crc(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "vpk: {e}"),
            Error::Signature(s) => write!(f, "vpk: bad signature {s:#010x}"),
            Error::Version(v) => write!(f, "vpk: version {v} isn't supported"),
            Error::Truncated => write!(f, "vpk: directory tree is truncated"),
            Error::Crc(name) => write!(f, "vpk: {name} fails its CRC"),
        }
    }
}

fn io(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

#[derive(Debug, Clone)]
struct Entry {
    crc: u32,
    preload: Vec<u8>,
    archive: u16,
    offset: u32,
    len: u32,
}

/// An open pak: its directory in memory, file data read on demand.
#[derive(Debug)]
pub struct Vpk {
    path: PathBuf,
    /// Where inline (0x7FFF) data starts in the directory file.
    data_start: u64,
    entries: HashMap<String, Entry>,
}

/// Pak paths compare case- and separator-blind, as Source's filesystem does.
pub fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}

struct Tree<'a> {
    data: &'a [u8],
}

impl Tree<'_> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let head = self.data.get(..N).ok_or(Error::Truncated)?;
        self.data = &self.data[N..];
        Ok(head.try_into().unwrap())
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.bytes()?))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn string(&mut self) -> Result<String, Error> {
        let end = self.data.iter().position(|&b| b == 0).ok_or(Error::Truncated)?;
        let s = String::from_utf8_lossy(&self.data[..end]).into_owned();
        self.data = &self.data[end + 1..];
        Ok(s)
    }
    fn take(&mut self, n: usize) -> Result<Vec<u8>, Error> {
        let head = self.data.get(..n).ok_or(Error::Truncated)?.to_vec();
        self.data = &self.data[n..];
        Ok(head)
    }
}

/// `dir/file.ext` from the tree's parts; a single space stands for "none".
fn join(dir: &str, file: &str, ext: &str) -> String {
    let mut out = String::new();
    if !dir.trim().is_empty() {
        out.push_str(dir.trim_matches('/'));
        out.push('/');
    }
    out.push_str(file);
    if !ext.trim().is_empty() {
        out.push('.');
        out.push_str(ext);
    }
    normalize(&out)
}

fn parse_tree(tree: &[u8]) -> Result<HashMap<String, Entry>, Error> {
    let mut t = Tree { data: tree };
    let mut entries = HashMap::new();
    loop {
        let ext = t.string()?;
        if ext.is_empty() {
            return Ok(entries);
        }
        loop {
            let dir = t.string()?;
            if dir.is_empty() {
                break;
            }
            loop {
                let file = t.string()?;
                if file.is_empty() {
                    break;
                }
                let (crc, preload_len, archive, offset, len) = (t.u32()?, t.u16()?, t.u16()?, t.u32()?, t.u32()?);
                if t.u16()? != TERMINATOR {
                    return Err(Error::Truncated);
                }
                let preload = t.take(preload_len.into())?;
                entries.insert(join(&dir, &file, &ext), Entry { crc, preload, archive, offset, len });
            }
        }
    }
}

impl Vpk {
    /// Read the header and directory tree of the pak at `path` (a single-file pak or a `_dir.vpk`).
    pub fn open(path: &Path) -> Result<Vpk, Error> {
        let mut f = std::fs::File::open(path).map_err(io)?;
        let mut head = [0u8; 12];
        f.read_exact(&mut head).map_err(io)?;
        let word = |i: usize| u32::from_le_bytes(head[i * 4..i * 4 + 4].try_into().unwrap());
        if word(0) != SIGNATURE {
            return Err(Error::Signature(word(0)));
        }
        let header_len = match word(1) {
            1 => 12,
            2 => 28,
            v => return Err(Error::Version(v)),
        };
        let mut tree = vec![0u8; word(2) as usize];
        f.seek(SeekFrom::Start(header_len)).map_err(io)?;
        f.read_exact(&mut tree).map_err(io)?;
        Ok(Vpk { path: path.to_owned(), data_start: header_len + tree.len() as u64, entries: parse_tree(&tree)? })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(&normalize(name))
    }

    /// Every file's normalized path.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// The chunk file holding archive `index`: `<name>_NNN.vpk` beside `<name>_dir.vpk`.
    fn chunk(&self, index: u16) -> PathBuf {
        let stem = self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let base = stem.strip_suffix("_dir").unwrap_or(&stem);
        self.path.with_file_name(format!("{base}_{index:03}.vpk"))
    }

    /// A file's contents (`None` if the pak doesn't have it), checked against its CRC.
    pub fn read(&self, name: &str) -> Option<Result<Vec<u8>, Error>> {
        let key = normalize(name);
        let e = self.entries.get(&key)?;
        Some(self.read_entry(&key, e))
    }

    fn read_entry(&self, key: &str, e: &Entry) -> Result<Vec<u8>, Error> {
        let mut out = e.preload.clone();
        if e.len > 0 {
            let (file, at) = match e.archive {
                INLINE => (self.path.clone(), self.data_start + u64::from(e.offset)),
                i => (self.chunk(i), u64::from(e.offset)),
            };
            let mut f = std::fs::File::open(&file).map_err(io)?;
            f.seek(SeekFrom::Start(at)).map_err(io)?;
            let start = out.len();
            out.resize(start + e.len as usize, 0);
            f.read_exact(&mut out[start..]).map_err(io)?;
        }
        // A zero CRC counts as "not recorded".
        if e.crc != 0 && crc32(&out) != e.crc {
            return Err(Error::Crc(key.to_owned()));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
