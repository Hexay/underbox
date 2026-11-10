//! Writing VPK v2 paks: single-file (as a map ships) or a `_dir.vpk` with numbered chunks. The MD5
//! and signature sections are left empty, which readers accept.

use super::{INLINE, SIGNATURE, TERMINATOR, crc32, normalize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Builder {
    files: BTreeMap<String, Vec<u8>>,
}

/// Where one file's data goes: its archive, its offset there, and how much stays in the tree.
struct Placement {
    archive: u16,
    offset: u32,
    preload: usize,
}

/// (dir, file, ext) as the tree spells them, " " for none.
fn split(name: &str) -> (String, String, String) {
    let (dir, file) = name.rsplit_once('/').unwrap_or((" ", name));
    let (stem, ext) = file.rsplit_once('.').unwrap_or((file, " "));
    (dir.to_owned(), stem.to_owned(), ext.to_owned())
}

fn cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, name: &str, data: impl Into<Vec<u8>>) -> &mut Self {
        self.files.insert(normalize(name), data.into());
        self
    }

    fn tree(&self, mut place: impl FnMut(&[u8]) -> Placement) -> Vec<u8> {
        type Files<'a> = Vec<(String, &'a Vec<u8>)>;
        let mut grouped: BTreeMap<String, BTreeMap<String, Files>> = BTreeMap::new();
        for (name, data) in &self.files {
            let (dir, file, ext) = split(name);
            grouped.entry(ext).or_default().entry(dir).or_default().push((file, data));
        }
        let mut out = Vec::new();
        for (ext, dirs) in grouped {
            cstr(&mut out, &ext);
            for (dir, files) in dirs {
                cstr(&mut out, &dir);
                for (file, data) in files {
                    cstr(&mut out, &file);
                    let p = place(data);
                    let rest = (data.len() - p.preload) as u32;
                    out.extend_from_slice(&crc32(data).to_le_bytes());
                    out.extend_from_slice(&(p.preload as u16).to_le_bytes());
                    out.extend_from_slice(&p.archive.to_le_bytes());
                    out.extend_from_slice(&p.offset.to_le_bytes());
                    out.extend_from_slice(&rest.to_le_bytes());
                    out.extend_from_slice(&TERMINATOR.to_le_bytes());
                    out.extend_from_slice(&data[..p.preload]);
                }
                out.push(0);
            }
            out.push(0);
        }
        out.push(0);
        out
    }

    fn header(tree_len: usize, data_len: usize) -> Vec<u8> {
        [SIGNATURE, 2, tree_len as u32, data_len as u32, 0, 0, 0].iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// One self-contained pak, every file's data inline after the tree.
    pub fn single(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let tree = self.tree(|bytes| {
            let offset = data.len() as u32;
            data.extend_from_slice(bytes);
            Placement { archive: INLINE, offset, preload: 0 }
        });
        [Self::header(tree.len(), data.len()), tree, data].concat()
    }

    /// `<base>_dir.vpk` in `dir` plus chunks `<base>_000.vpk`... of about `chunk_size` bytes; each
    /// file keeps up to `preload` leading bytes in the tree. Returns the `_dir.vpk` path.
    pub fn chunked(&self, dir: &Path, base: &str, chunk_size: usize, preload: usize) -> std::io::Result<PathBuf> {
        let mut chunks: Vec<Vec<u8>> = vec![Vec::new()];
        let tree = self.tree(|bytes| {
            let preload = preload.min(bytes.len());
            if chunks.last().unwrap().len() >= chunk_size {
                chunks.push(Vec::new());
            }
            let chunk = chunks.last_mut().unwrap();
            let offset = chunk.len() as u32;
            chunk.extend_from_slice(&bytes[preload..]);
            Placement { archive: (chunks.len() - 1) as u16, offset, preload }
        });
        for (i, chunk) in chunks.iter().enumerate() {
            std::fs::write(dir.join(format!("{base}_{i:03}.vpk")), chunk)?;
        }
        let path = dir.join(format!("{base}_dir.vpk"));
        std::fs::write(&path, [Self::header(tree.len(), 0), tree].concat())?;
        Ok(path)
    }
}
