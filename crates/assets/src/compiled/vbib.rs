//! Vertex and index buffers: the binary `VBIB`/`MBUF` block, and the KV3 form newer meshes keep in
//! their DATA or CTRL (`m_vertexBuffers`, data inline or in an `MVTX`/`MIDX` block). Buffers come
//! out decompressed (zstd, then meshoptimizer).

use super::kv::{self, blob, int, string};
use super::{Error, block_at, format};
use crate::meshopt;
use ::kv::{Object, Value};

/// One input-layout field (`RenderInputLayoutField_t`).
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// Upper case: POSITION, NORMAL, TANGENT, TEXCOORD, COLOR, BLENDINDICES, BLENDWEIGHT...
    pub semantic: String,
    pub index: i32,
    /// A `DXGI_FORMAT`.
    pub format: u32,
    pub offset: u32,
    pub slot: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Buffer {
    pub count: usize,
    pub stride: usize,
    pub attributes: Vec<Attribute>,
    /// `count * stride` bytes, decompressed.
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Buffers {
    pub vertex: Vec<Buffer>,
    pub index: Vec<Buffer>,
}

/// How a writer stores buffer data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Packing {
    Raw,
    Meshopt,
    MeshoptZstd,
}

const SIZE_MASK: u32 = 0x3FF_FFFF;
/// Set when the data is *not* meshopt-encoded (VBIB.cs reads the bit inverted).
const NOT_MESHOPT: u32 = 0x400_0000;
const ZSTD: u32 = 0x800_0000;
const RECORD: usize = 24;
const ATTRIBUTE: usize = 56;

fn u32_at(b: &[u8], at: usize) -> Result<u32, Error> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| format("VBIB truncated"))
}

/// Undo the stored packing; data already `count * stride` long is raw whatever the flags say.
fn unpack(b: &Buffer, stored: &[u8], vertex: bool, meshopt: bool, zstd: bool) -> Result<Vec<u8>, Error> {
    let size = b.count * b.stride;
    if stored.len() >= size {
        return Ok(stored[..size].to_vec());
    }
    let mut owned = Vec::new();
    let data = if zstd {
        let mut src = stored;
        let decoder = ruzstd::decoding::StreamingDecoder::new(&mut src).map_err(|e| Error::Decompress(e.to_string()))?;
        std::io::Read::read_to_end(&mut std::io::Read::take(decoder, size as u64 * 2 + 1024), &mut owned).map_err(|e| Error::Decompress(e.to_string()))?;
        &owned[..]
    } else {
        stored
    };
    if !meshopt {
        return if data.len() == size { Ok(data.to_vec()) } else { Err(format(format!("buffer holds {} bytes, expected {size}", data.len()))) };
    }
    if vertex {
        return Ok(meshopt::decode_vertex_buffer(data, b.count, b.stride)?);
    }
    let indices = meshopt::decode_index_buffer(data, b.count)?;
    Ok(match b.stride {
        2 => indices.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect(),
        _ => indices.iter().flat_map(|&i| i.to_le_bytes()).collect(),
    })
}

fn read_record(block: &[u8], at: usize, vertex: bool) -> Result<Buffer, Error> {
    let flags = u32_at(block, at + 4)?;
    let mut b = Buffer { count: u32_at(block, at)? as usize, stride: (flags & SIZE_MASK) as usize, ..Default::default() };
    let attrs = at + 8 + u32_at(block, at + 8)? as usize;
    for i in 0..u32_at(block, at + 12)? as usize {
        let a = attrs + i * ATTRIBUTE;
        let name = block.get(a..a + 32).ok_or_else(|| format("VBIB attribute truncated"))?;
        let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(32)];
        let int_at = |o: usize| u32_at(block, a + 32 + o);
        b.attributes.push(Attribute { semantic: String::from_utf8_lossy(name).to_ascii_uppercase(), index: int_at(0)? as i32, format: int_at(4)?, offset: int_at(8)?, slot: int_at(12)? as i32 });
    }
    let data = at + 16 + u32_at(block, at + 16)? as usize;
    let stored = block.get(data..data + u32_at(block, at + 20)? as usize).ok_or_else(|| format("VBIB data out of bounds"))?;
    b.data = unpack(&b, stored, vertex, flags & NOT_MESHOPT == 0, flags & ZSTD != 0)?;
    Ok(b)
}

/// A binary `VBIB` (or `MBUF`) block.
pub fn read(block: &[u8]) -> Result<Buffers, Error> {
    let list = |offset_field: usize, vertex| -> Result<Vec<Buffer>, Error> {
        let first = offset_field + u32_at(block, offset_field)? as usize;
        (0..u32_at(block, offset_field + 4)? as usize).map(|i| read_record(block, first + i * RECORD, vertex)).collect()
    };
    Ok(Buffers { vertex: list(0, true)?, index: list(8, false)? })
}

fn kv_buffer(o: &Object, file: &[u8], vertex: bool) -> Result<Buffer, Error> {
    let mut b = Buffer { count: int(o, "m_nElementCount") as usize, stride: int(o, "m_nElementSizeInBytes") as usize, ..Default::default() };
    for f in kv::objects(o, "m_inputLayoutFields") {
        let semantic = match f.get("m_pSemanticName") {
            Some(Value::Binary(bytes)) => String::from_utf8_lossy(bytes).trim_end_matches('\0').to_owned(),
            _ => string(f, "m_pSemanticName"),
        };
        let field = |k| int(f, k);
        b.attributes.push(Attribute { semantic: semantic.to_ascii_uppercase(), index: field("m_nSemanticIndex") as i32, format: field("m_Format") as u32, offset: field("m_nOffset") as u32, slot: field("m_nSlot") as i32 });
    }
    let inline = blob(o, "m_pData");
    b.data = if !inline.is_empty() {
        unpack(&b, inline, vertex, true, false)?
    } else {
        let stored = block_at(file, kv::int_or(o, "m_nBlockIndex", -1))?;
        unpack(&b, stored, vertex, kv::bool(o, "m_bMeshoptCompressed"), kv::bool(o, "m_bCompressedZSTD"))?
    };
    Ok(b)
}

/// The KV3 form: `m_vertexBuffers`/`m_indexBuffers` of `o`, blocks addressed within `file`.
pub fn from_kv3(o: &Object, file: &[u8]) -> Result<Buffers, Error> {
    let list = |key, vertex| kv::objects(o, key).map(|b| kv_buffer(b, file, vertex)).collect::<Result<Vec<_>, _>>();
    Ok(Buffers { vertex: list("m_vertexBuffers", true)?, index: list("m_indexBuffers", false)? })
}

fn pack(b: &Buffer, vertex: bool, packing: Packing) -> (u32, Vec<u8>) {
    let encoded = || if vertex { meshopt::encode_vertex_buffer(&b.data, b.stride, 0) } else { meshopt::encode_index_buffer(&indices_of(b)) };
    let packed = match packing {
        Packing::Raw => (NOT_MESHOPT, b.data.clone()),
        Packing::Meshopt => (0, encoded()),
        Packing::MeshoptZstd => (ZSTD, ruzstd::encoding::compress_to_vec(&encoded()[..], ruzstd::encoding::CompressionLevel::Fastest)),
    };
    // Readers take anything at least the raw size as raw, so a stream that didn't shrink can't be stored.
    if packed.1.len() >= b.data.len() { (NOT_MESHOPT, b.data.clone()) } else { packed }
}

/// An index buffer's indices, widened.
pub fn indices_of(b: &Buffer) -> Vec<u32> {
    match b.stride {
        2 => b.data.as_chunks::<2>().0.iter().map(|c| u32::from(u16::from_le_bytes(*c))).collect(),
        _ => b.data.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect(),
    }
}

/// A binary `VBIB` block: header, records, attributes, then each buffer's data.
pub fn write(buffers: &Buffers, packing: Packing) -> Vec<u8> {
    let all: Vec<(&Buffer, bool)> = buffers.vertex.iter().map(|b| (b, true)).chain(buffers.index.iter().map(|b| (b, false))).collect();
    let mut out = vec![0u8; 16 + all.len() * RECORD];
    let put = |out: &mut Vec<u8>, at: usize, v: u32| out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut out, 0, 16);
    put(&mut out, 4, buffers.vertex.len() as u32);
    put(&mut out, 8, (16 + buffers.vertex.len() * RECORD - 8) as u32);
    put(&mut out, 12, buffers.index.len() as u32);
    for (i, (b, vertex)) in all.into_iter().enumerate() {
        let rec = 16 + i * RECORD;
        let (flags, data) = pack(b, vertex, packing);
        put(&mut out, rec, b.count as u32);
        put(&mut out, rec + 4, b.stride as u32 | flags);
        let attrs = out.len();
        put(&mut out, rec + 8, (attrs - (rec + 8)) as u32);
        put(&mut out, rec + 12, b.attributes.len() as u32);
        for a in &b.attributes {
            let mut name = [0u8; 32];
            name[..a.semantic.len().min(31)].copy_from_slice(&a.semantic.as_bytes()[..a.semantic.len().min(31)]);
            out.extend(name);
            [a.index as u32, a.format, a.offset, a.slot as u32, 0, 0].iter().for_each(|v| out.extend(v.to_le_bytes()));
        }
        out.resize(out.len().next_multiple_of(16), 0);
        let start = out.len();
        put(&mut out, rec + 16, (start - (rec + 16)) as u32);
        put(&mut out, rec + 20, data.len() as u32);
        out.extend(data);
    }
    out
}
