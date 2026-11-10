//! Binary KeyValues3 (`KV3\x02`..`KV3\x05`), the payload of most compiled-resource blocks (the DATA
//! of worlds, world nodes and entity lumps). Layout: research/resource-container.md §4 and
//! research/world.md. Values come back as [`kv::Value`]; the legacy `VKV\x03` form isn't read.

mod decode;
mod encode;
#[cfg(test)]
mod tests;

pub use encode::{Compression, write};

use decode::{Ctx, Lanes};
use kv::Value;

const MAGIC_BASE: u32 = 0x4B56_3300;
const TRAILER: u32 = 0xFFEE_DD00;
/// The most an LZ4 blob frame decodes to; LZ4 headers carry it, others carry 0.
const LZ4_FRAME: usize = 16384;
const LZ4: u32 = 1;
const ZSTD: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Truncated,
    Magic(u32),
    Version(u32),
    Compression(u32),
    Decompress(String),
    Trailer(u32),
    Type(u8),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Truncated => write!(f, "binary KV3 is truncated"),
            Error::Magic(m) => write!(f, "not binary KV3 (magic {m:#010x})"),
            Error::Version(v) => write!(f, "binary KV3 version {v} isn't supported"),
            Error::Compression(c) => write!(f, "binary KV3 compression {c} isn't supported"),
            Error::Decompress(e) => write!(f, "binary KV3 decompression: {e}"),
            Error::Trailer(t) => write!(f, "binary KV3 trailer {t:#010x}"),
            Error::Type(t) => write!(f, "binary KV3 node type {t}"),
        }
    }
}

impl std::error::Error for Error {}

/// Whether `data` starts with a binary KV3 magic this reader handles.
pub fn is_binary_kv3(data: &[u8]) -> bool {
    data.get(..4).map(|m| u32::from_le_bytes(m.try_into().unwrap())).is_some_and(|m| m & 0xFFFF_FF00 == MAGIC_BASE && (2..=5).contains(&(m & 0xFF)))
}

pub(super) struct Cursor<'a> {
    data: &'a [u8],
}

impl<'a> Cursor<'a> {
    pub(super) fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.data.len() < n {
            return Err(Error::Truncated);
        }
        let (head, rest) = self.data.split_at(n);
        self.data = rest;
        Ok(head)
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub(super) fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    /// A count or size: negative ones are corrupt.
    pub(super) fn len(&mut self) -> Result<usize, Error> {
        usize::try_from(self.u32()? as i32).map_err(|_| Error::Truncated)
    }
}

/// The header fields reading needs; per-buffer sizes are filled in for every version.
#[derive(Default)]
struct Header {
    version: u32,
    method: u32,
    /// Lane counts of buffer 1 (1-, 2-, 4- and 8-byte values).
    counts1: [usize; 4],
    counts2: [usize; 4],
    count_types: usize,
    size_compressed_total: usize,
    count_blobs: usize,
    size_blob_bytes: usize,
    uncompressed: [usize; 2],
    compressed: [usize; 2],
    objects2: usize,
}

fn header(c: &mut Cursor, version: u32) -> Result<Header, Error> {
    c.take(16)?; // format GUID
    let mut h = Header { version, method: c.u32()?, ..Header::default() };
    if h.method > ZSTD {
        return Err(Error::Compression(h.method));
    }
    let (_dictionary, _frame_size) = (c.u16()?, c.u16()?);
    h.counts1 = [c.len()?, 0, c.len()?, c.len()?];
    h.count_types = c.len()?;
    c.take(4)?; // object and array count hints
    let size_uncompressed_total = c.len()?;
    h.size_compressed_total = c.len()?;
    (h.count_blobs, h.size_blob_bytes) = (c.len()?, c.len()?);
    if version >= 4 {
        h.counts1[1] = c.len()?;
        c.len()?; // size of the LZ4 blob frame sizes
    }
    if version >= 5 {
        h.uncompressed[0] = c.len()?;
        h.compressed[0] = c.len()?;
        h.uncompressed[1] = c.len()?;
        h.compressed[1] = c.len()?;
        h.counts2 = [c.len()?, c.len()?, c.len()?, c.len()?];
        c.len()?; // node count hint
        h.objects2 = c.len()?;
        c.take(8)?; // array and element count hints
    } else {
        (h.uncompressed[0], h.compressed[0]) = (size_uncompressed_total, h.size_compressed_total);
    }
    Ok(h)
}

/// `size` decompressed bytes from the next `compressed` bytes (the uncompressed form takes `size`).
fn inflate(c: &mut Cursor, method: u32, compressed: usize, size: usize) -> Result<Vec<u8>, Error> {
    let out = match method {
        LZ4 => lz4_flex::block::decompress(c.take(compressed)?, size).map_err(|e| Error::Decompress(e.to_string()))?,
        ZSTD => {
            let mut out = Vec::with_capacity(size);
            ruzstd::decoding::FrameDecoder::new().decode_all_to_vec(c.take(compressed)?, &mut out).map_err(|e| Error::Decompress(e.to_string()))?;
            out
        }
        _ => c.take(size)?.to_vec(),
    };
    if out.len() != size {
        return Err(Error::Decompress(format!("{} bytes, expected {size}", out.len())));
    }
    Ok(out)
}

/// Lanes laid out from `offset`, each aligned to its width relative to the buffer start. Before
/// v5 the 8-byte lane is aligned even when empty. Returns the lanes and the offset after them.
fn lanes(buf: &[u8], mut offset: usize, counts: [usize; 4], pre_v5: bool) -> Result<(Lanes<'_>, usize), Error> {
    let mut out = [&buf[..0]; 4];
    for (i, (width, count)) in [1usize, 2, 4, 8].into_iter().zip(counts).enumerate() {
        if count > 0 || (pre_v5 && width == 8) {
            offset = offset.next_multiple_of(width);
        }
        let end = offset + count * width;
        out[i] = buf.get(offset..end).ok_or(Error::Truncated)?;
        offset = end;
    }
    Ok((Lanes { b1: out[0], b2: out[1], b4: out[2], b8: out[3] }, offset))
}

/// `count` NUL-terminated UTF-8 strings from the front of `buf`, and the bytes they took.
fn strings(buf: &[u8], count: usize) -> Result<(Vec<String>, usize), Error> {
    let mut at = 0;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let len = buf.get(at..).and_then(|b| b.iter().position(|&x| x == 0)).ok_or(Error::Truncated)?;
        out.push(String::from_utf8_lossy(&buf[at..at + len]).into_owned());
        at += len + 1;
    }
    Ok((out, at))
}

fn trailer(c: &mut Cursor) -> Result<(), Error> {
    match c.u32()? {
        TRAILER => Ok(()),
        t => Err(Error::Trailer(t)),
    }
}

/// Decode a binary KV3 document.
pub fn read(data: &[u8]) -> Result<Value, Error> {
    let mut c = Cursor { data };
    let magic = c.u32()?;
    if magic & 0xFFFF_FF00 != MAGIC_BASE {
        return Err(Error::Magic(magic));
    }
    let version = magic & 0xFF;
    if !(2..=5).contains(&version) {
        return Err(Error::Version(version));
    }
    let h = header(&mut c, version)?;
    // Before v5, zstd compresses the blobs in the same stream as the buffer.
    let shared_zstd = version < 5 && h.method == ZSTD;
    let size1 = h.uncompressed[0] + if shared_zstd { h.size_blob_bytes } else { 0 };
    let raw1 = inflate(&mut c, h.method, h.compressed[0], size1)?;
    let buf1 = &raw1[..h.uncompressed[0]];
    let (mut lanes1, end1) = lanes(buf1, 0, h.counts1, version < 5)?;
    let mut count = Cursor { data: lanes1.b4 };
    let string_count = count.len()?;
    lanes1.b4 = count.data;
    let raw2 = if version >= 5 { inflate(&mut c, h.method, h.compressed[1], h.uncompressed[1])? } else { Vec::new() };
    let (strings, ctx_lanes, aux, types, object_lengths, tail) = if version >= 5 {
        let (strings, used) = strings(lanes1.b1, string_count)?;
        lanes1.b1 = &lanes1.b1[used..];
        let objects_end = h.objects2 * 4;
        let (lanes2, end2) = lanes(&raw2, objects_end, h.counts2, false)?;
        let types = raw2.get(end2..end2 + h.count_types).ok_or(Error::Truncated)?;
        (strings, lanes2, Some(lanes1), types, raw2.get(..objects_end).ok_or(Error::Truncated)?, &raw2[end2 + h.count_types..])
    } else {
        let (strings, used) = strings(buf1.get(end1..).ok_or(Error::Truncated)?, string_count)?;
        // Before v5 the type count includes the string bytes.
        let types_end = end1 + h.count_types;
        let types = buf1.get(end1 + used..types_end).ok_or(Error::Truncated)?;
        (strings, lanes1, None, types, &buf1[..0], &buf1[types_end..])
    };
    let mut tail = Cursor { data: tail };
    let blobs = if h.count_blobs == 0 {
        trailer(&mut tail)?;
        Vec::new()
    } else {
        let lengths = (0..h.count_blobs).map(|_| tail.len()).collect::<Result<Vec<_>, _>>()?;
        trailer(&mut tail)?;
        let blobs = read_blobs(&mut c, &h, &lengths, &mut tail, raw1.get(h.uncompressed[0]..).unwrap_or_default())?;
        trailer(&mut c)?;
        blobs
    };
    Ctx { version, types, object_lengths, strings, blobs: blobs.into_iter(), main: ctx_lanes, aux }.root()
}

/// The binary blobs after the buffers. `frames` holds the LZ4 frame sizes; `shared` is the blob
/// part of a pre-v5 zstd buffer.
fn read_blobs(c: &mut Cursor, h: &Header, lengths: &[usize], frames: &mut Cursor, shared: &[u8]) -> Result<Vec<Vec<u8>>, Error> {
    let all = match h.method {
        LZ4 => decode::lz4_frames(c, frames, h.size_blob_bytes, lengths)?,
        ZSTD if h.version >= 5 => {
            let compressed = h.size_compressed_total.checked_sub(h.compressed[0] + h.compressed[1]).ok_or(Error::Truncated)?;
            inflate(c, ZSTD, compressed, h.size_blob_bytes)?
        }
        ZSTD => shared.to_vec(),
        _ => c.take(h.size_blob_bytes)?.to_vec(),
    };
    let mut at = 0;
    lengths
        .iter()
        .map(|&n| {
            let blob = all.get(at..at + n).ok_or(Error::Truncated)?.to_vec();
            at += n;
            Ok(blob)
        })
        .collect()
}
