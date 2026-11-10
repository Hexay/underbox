//! The Source 2 compiled-resource container (`*_c`): a 16-byte header, a block table, then the
//! blocks. Layout and sources: research/resource-container.md §1–2. All little-endian.

/// Every container this format version has; readers reject others.
const HEADER_VERSION: u16 = 12;
const HEADER_SIZE: usize = 16;
const ENTRY_SIZE: usize = 12;
/// Valve aligns block payloads to 4 bytes with zero padding (§2).
const BLOCK_ALIGN: usize = 4;

/// A block's 4-character type, as written in the file (`DATA`, `RERL`, `REDI`...).
pub type FourCC = [u8; 4];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Container {
    /// The resource type's own version (a game resource's `ResourceVersion`).
    pub version: u16,
    pub blocks: Vec<(FourCC, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReadError {
    Truncated,
    HeaderVersion(u16),
    BlockOutOfBounds(FourCC),
}

/// One block read in place: its type and where its payload sits in the file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockRef {
    pub kind: FourCC,
    pub offset: usize,
    pub len: usize,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The block table of a container in `file`, bounds-checked against the file.
pub fn blocks(file: &[u8]) -> Result<Vec<BlockRef>, ReadError> {
    let header_version = u16_at(file, 4).ok_or(ReadError::Truncated)?;
    if header_version != HEADER_VERSION {
        return Err(ReadError::HeaderVersion(header_version));
    }
    let table = 8 + u32_at(file, 8).ok_or(ReadError::Truncated)? as usize;
    let count = u32_at(file, 12).ok_or(ReadError::Truncated)? as usize;
    (0..count)
        .map(|i| {
            let entry = table + i * ENTRY_SIZE;
            let kind: FourCC = file.get(entry..entry + 4).ok_or(ReadError::Truncated)?.try_into().unwrap();
            // The offset counts from the offset field itself (entry + 4).
            let offset = entry + 4 + u32_at(file, entry + 4).ok_or(ReadError::Truncated)? as usize;
            let len = u32_at(file, entry + 8).ok_or(ReadError::Truncated)? as usize;
            if offset.checked_add(len).is_none_or(|end| end > file.len()) {
                return Err(ReadError::BlockOutOfBounds(kind));
            }
            Ok(BlockRef { kind, offset, len })
        })
        .collect()
}

/// The payload of the first block of type `kind`.
pub fn block<'a>(file: &'a [u8], kind: &FourCC) -> Result<Option<&'a [u8]>, ReadError> {
    Ok(blocks(file)?.into_iter().find(|b| &b.kind == kind).map(|b| &file[b.offset..b.offset + b.len]))
}

impl Container {
    pub fn read(file: &[u8]) -> Result<Self, ReadError> {
        let version = u16_at(file, 6).ok_or(ReadError::Truncated)?;
        let blocks = blocks(file)?.into_iter().map(|b| (b.kind, file[b.offset..b.offset + b.len].to_vec())).collect();
        Ok(Self { version, blocks })
    }

    pub fn write(&self) -> Vec<u8> {
        let table_end = HEADER_SIZE + self.blocks.len() * ENTRY_SIZE;
        let mut out = vec![0u8; table_end];
        out[4..6].copy_from_slice(&HEADER_VERSION.to_le_bytes());
        out[6..8].copy_from_slice(&self.version.to_le_bytes());
        out[8..12].copy_from_slice(&8u32.to_le_bytes());
        out[12..16].copy_from_slice(&(self.blocks.len() as u32).to_le_bytes());
        for (i, (kind, payload)) in self.blocks.iter().enumerate() {
            out.resize(out.len().next_multiple_of(BLOCK_ALIGN), 0);
            let entry = HEADER_SIZE + i * ENTRY_SIZE;
            let rel = (out.len() - (entry + 4)) as u32;
            out[entry..entry + 4].copy_from_slice(kind);
            out[entry + 4..entry + 8].copy_from_slice(&rel.to_le_bytes());
            out[entry + 8..entry + 12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(payload);
        }
        let size = out.len() as u32;
        out[0..4].copy_from_slice(&size.to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let c = Container { version: 1, blocks: vec![(*b"RERL", vec![8, 0, 0, 0, 0, 0, 0, 0]), (*b"DATA", b"{}\0".to_vec())] };
        let bytes = c.write();
        assert_eq!(u32_at(&bytes, 0), Some(bytes.len() as u32));
        assert_eq!(Container::read(&bytes), Ok(c));
        assert_eq!(block(&bytes, b"DATA").unwrap(), Some(&b"{}\0"[..]));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(blocks(&[0; 3]), Err(ReadError::Truncated));
        let mut bytes = Container { version: 1, blocks: vec![(*b"DATA", vec![1, 2, 3])] }.write();
        bytes[24..28].copy_from_slice(&1000u32.to_le_bytes());
        assert_eq!(blocks(&bytes), Err(ReadError::BlockOutOfBounds(*b"DATA")));
    }
}
