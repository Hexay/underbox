//! On-disk cache of translated combos, keyed by a hash of everything that decides the output: the
//! expanded source (so every include's content), defines (combo values), stage, DXC flags and
//! library, and the naga and crate versions.

use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};

/// The naga wgpu 30 uses; bump with the dependency.
pub const NAGA_VERSION: &str = "30.0.1";

/// Bump whenever a SPIR-V pass's output changes: boots share one temp-dir cache across worktrees.
pub const PASSES_VERSION: &str = "4";

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Cache {
        Cache { dir: dir.into() }
    }

    /// `$UNDERBOX_SHADER_CACHE`, else `shader-cache` in the cargo target directory the running
    /// binary lives in (found by its `CACHEDIR.TAG`), else the system temp directory.
    pub fn default_dir() -> PathBuf {
        if let Some(d) = std::env::var_os("UNDERBOX_SHADER_CACHE") {
            return d.into();
        }
        let exe = std::env::current_exe().unwrap_or_default();
        exe.ancestors()
            .find(|a| a.join("CACHEDIR.TAG").is_file())
            .map_or_else(|| std::env::temp_dir().join("underbox-shader-cache"), |t| t.join("shader-cache"))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(&key[..2]).join(format!("{key}.json"))
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        serde_json::from_slice(&std::fs::read(self.path(key)).ok()?).ok()
    }

    /// Stores `value`; failures only cost a recompile later, so they are ignored.
    pub fn put<T: Serialize>(&self, key: &str, value: &T) {
        let path = self.path(key);
        let Ok(bytes) = serde_json::to_vec(value) else { return };
        let Some(dir) = path.parent() else { return };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        // Write then rename, so a concurrent reader never sees half a file.
        let tmp = dir.join(format!("{key}.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// A cache key: blake3 over length-prefixed parts.
pub fn key<'a>(parts: impl IntoIterator<Item = &'a [u8]>) -> String {
    let mut h = blake3::Hasher::new();
    for p in parts {
        h.update(&(p.len() as u64).to_le_bytes());
        h.update(p);
    }
    h.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_key_separation() {
        let dir = std::env::temp_dir().join(format!("underbox-cache-test-{}", std::process::id()));
        let cache = Cache::new(&dir);
        let k = key([b"ab".as_slice(), b"c"]);
        assert_ne!(k, key([b"a".as_slice(), b"bc"]));
        assert_eq!(cache.get::<Vec<u32>>(&k), None);
        cache.put(&k, &vec![1u32, 2, 3]);
        assert_eq!(cache.get::<Vec<u32>>(&k), Some(vec![1, 2, 3]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
