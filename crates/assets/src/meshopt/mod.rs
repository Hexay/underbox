//! meshoptimizer's vertex and index codecs (MIT, github.com/zeux/meshoptimizer `src/vertexcodec.cpp`,
//! `src/indexcodec.cpp`), the compression compiled meshes use for VBIB/MBUF buffers. Scalar ports of
//! the decoders (vertex v0/v1, index buffer v0/v1, index sequence) plus encoders for test fixtures.

mod index;
#[cfg(test)]
mod tests;
mod vertex;

pub use index::{decode_index_buffer, decode_index_sequence, encode_index_buffer, encode_index_sequence};
pub use vertex::{decode_vertex_buffer, encode_vertex_buffer};

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Not this codec's header, or a newer version.
    Header(u8),
    /// The stream ends early or has bytes left over.
    Size,
    /// A vertex block names a channel mode that doesn't exist.
    Channel(u8),
    /// Vertex size isn't a multiple of 4 in 4..=256, or the index count isn't whole triangles.
    Shape,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Header(h) => write!(f, "meshopt: header {h:#04x}"),
            Error::Size => f.write_str("meshopt: stream size doesn't match"),
            Error::Channel(c) => write!(f, "meshopt: vertex channel {c}"),
            Error::Shape => f.write_str("meshopt: bad vertex size or index count"),
        }
    }
}

impl std::error::Error for Error {}

/// LEB128-style varint, at most five bytes; a truncated stream reads as zeros (callers bound it).
fn decode_vbyte(data: &[u8], at: &mut usize) -> u32 {
    let byte = |at: &mut usize| {
        let b = data.get(*at).copied().unwrap_or(0);
        *at += 1;
        b
    };
    let lead = byte(at);
    if lead < 128 {
        return u32::from(lead);
    }
    let mut result = u32::from(lead & 127);
    let mut shift = 7;
    for _ in 0..4 {
        let group = byte(at);
        result |= u32::from(group & 127) << shift;
        shift += 7;
        if group < 128 {
            break;
        }
    }
    result
}

fn encode_vbyte(out: &mut Vec<u8>, mut v: u32) {
    loop {
        out.push((v & 127) as u8 | if v > 127 { 128 } else { 0 });
        v >>= 7;
        if v == 0 {
            break;
        }
    }
}

fn zigzag_delta(d: u32) -> u32 {
    (d << 1) ^ ((d as i32) >> 31) as u32
}

fn unzigzag_delta(v: u32) -> u32 {
    (v >> 1) ^ (-((v & 1) as i32)) as u32
}

