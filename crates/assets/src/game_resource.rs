//! s&box GameResources (`.scene`, `.prefab`, `.sound`...): JSON sources compiled into a
//! container whose `DATA` block is the JSON text plus a NUL (research/resource-container.md §5).

use crate::container::{self, Container, ReadError};
use crate::redi::{self, EditInfo};

/// What Facepunch's managed compiler records in REDI; VRF and others identify s&box files by it.
const COMPILER: (&str, &str, u32) = ("Sbox Data Compiler Version", "ManagedResourceCompiler", 1);
/// An RERL with no references: entry-array offset, then a count of 0.
const EMPTY_RERL: [u8; 8] = [8, 0, 0, 0, 0, 0, 0, 0];

/// GameResource extensions engine code defines, from sbox-public's `[AssetType]` attributes
/// (regenerated into abi/ by tools/regen.sh). Game-defined types are added when the game's
/// assemblies load. TODO: compile those too.
pub fn engine_extensions() -> impl Iterator<Item = &'static str> {
    include_str!("../../../abi/game_resource_extensions.txt").split_whitespace()
}

/// Compile a GameResource's source JSON. `source` is its path relative to the content root.
// TODO: AssetSystem.CompileGameResource's ScanJson rewrite (paths, embedded resources); stored as authored.
pub fn compile(json: &str, source: &str, version: u16) -> Vec<u8> {
    let mut data = json.trim_start_matches('\u{feff}').as_bytes().to_vec();
    data.push(0);
    let edit = EditInfo { input: Some((source, crc32(json.as_bytes()))), special: vec![COMPILER] };
    Container { version, blocks: vec![(*b"RERL", EMPTY_RERL.to_vec()), (*b"REDI", redi::write(&edit)), (*b"DATA", data)] }.write()
}

/// A compiled GameResource's JSON (`EngineGlue.ReadCompiledResourceFileJson`).
pub fn read_json(file: &[u8]) -> Result<Option<String>, ReadError> {
    Ok(container::block(file, b"DATA")?.map(|d| String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned()))
}

/// Native block lookups take a name; s&box's blob block is named `DBLOB`, which only fits a
/// FourCC as its first four bytes (unverified, research/resource-container.md open question 2).
pub fn fourcc(name: &str) -> Option<container::FourCC> {
    name.as_bytes().get(..4)?.try_into().ok()
}

/// CRC-32 (IEEE), REDI's input-file checksum.
fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, &b| (0..8).fold(crc ^ b as u32, |c, _| (c >> 1) ^ (0xEDB8_8320 & (c & 1).wrapping_neg())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_then_read() {
        let file = compile("{\"a\":1}", "scenes/x.scene", 1);
        assert_eq!(read_json(&file), Ok(Some("{\"a\":1}".into())));
    }

    #[test]
    fn crc() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn engine_types_include_scene() {
        assert!(engine_extensions().any(|e| e == "scene"));
    }
}
