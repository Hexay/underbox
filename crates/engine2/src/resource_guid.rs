//! A resource's GUID, as real's resource database gives it (`ResolvePathToGuid`, every handle's
//! `GetGuid`): the compiled file's `GUID` block, which the resource compiler copies from the
//! source's `.meta` (`"guid"`, a .NET Guid string), which is read when the block is missing (our
//! compiled game resources). Without either it is zero.

use abi::Guid;

/// The GUID of the resource at `path` (`x.vmdl` or `x.vmdl_c`), if its files carry one.
pub fn of_path(path: &str) -> Option<Guid> {
    let source = path.strip_suffix("_c").unwrap_or(path);
    let compiled = crate::fs::read_unreported(&format!("{source}_c"), "GAME");
    if let Some(guid) = compiled.as_deref().and_then(block) {
        return Some(guid);
    }
    let meta = crate::fs::read_unreported(&format!("{source}.meta"), "GAME")?;
    let json: serde_json::Value = serde_json::from_slice(&meta).ok()?;
    parse(json.get("guid")?.as_str()?)
}

/// `guid` when managed passed one, else the one `path`'s files carry.
pub fn or_path(guid: Guid, path: &str) -> Guid {
    if guid != Guid::default() { guid } else { of_path(path).unwrap_or_default() }
}

fn block(file: &[u8]) -> Option<Guid> {
    let bytes = assets::container::block(file, b"GUID").ok()??;
    Some(Guid(bytes.get(..16)?.try_into().ok()?))
}

/// A .NET Guid string in its in-memory byte order: the first three groups little-endian.
pub fn parse(s: &str) -> Option<Guid> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut b = [0u8; 16];
    for (i, byte) in b.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    b[0..4].reverse();
    b[4..6].reverse();
    b[6..8].reverse();
    Some(Guid(b))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_dotnet_byte_order() {
        let g = super::parse("091bfde9-872e-46c2-b6ce-4a6948df5eea").unwrap();
        assert_eq!(g.0, [0xe9, 0xfd, 0x1b, 0x09, 0x2e, 0x87, 0xc2, 0x46, 0xb6, 0xce, 0x4a, 0x69, 0x48, 0xdf, 0x5e, 0xea]);
    }
}
