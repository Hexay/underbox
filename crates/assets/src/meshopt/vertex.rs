//! The vertex codec: blocks of up to 256 vertices, each 4-byte channel's bytes transposed into
//! byte streams of deltas from the previous vertex, packed in groups of 16 at 0/1/2/4/8 bits with
//! out-of-range values appended as literals. The first vertex (and v1's channel modes) trail the
//! stream.

use super::Error;

const HEADER: u8 = 0xa0;
const BLOCK_BYTES: usize = 8192;
const BLOCK_MAX: usize = 256;
const GROUP: usize = 16;
const DECODE_LIMIT: usize = 24;
const BITS_V0: [u8; 4] = [0, 2, 4, 8];
const BITS_V1: [u8; 5] = [0, 1, 2, 4, 8];

/// Part of the format: changing it breaks compatibility.
fn block_size(vertex_size: usize) -> usize {
    ((BLOCK_BYTES / vertex_size) & !(GROUP - 1)).min(BLOCK_MAX)
}

fn tail_size(version: u8, vertex_size: usize) -> (usize, usize) {
    let tail = vertex_size + if version == 0 { 0 } else { vertex_size / 4 };
    (tail, tail.max(if version == 0 { 32 } else { 24 }))
}

/// 1-bit groups store their bits in reverse order.
fn reverse_bits(b: u8) -> u8 {
    ((((u64::from(b) * 0x8020_0802) & 0x08_8442_2110).wrapping_mul(0x01_0101_0101)) >> 32) as u8
}

fn decode_group(data: &[u8], at: usize, out: &mut [u8], bits: u8) -> Result<usize, Error> {
    match bits {
        0 => {
            out[..GROUP].fill(0);
            Ok(at)
        }
        8 => {
            out[..GROUP].copy_from_slice(data.get(at..at + GROUP).ok_or(Error::Size)?);
            Ok(at + GROUP)
        }
        _ => {
            let per_byte = 8 / bits as usize;
            let packed = GROUP / per_byte;
            let sentinel = (1u8 << bits) - 1;
            let mut extra = at + packed;
            for (i, slot) in out[..GROUP].iter_mut().enumerate() {
                let mut byte = *data.get(at + i / per_byte).ok_or(Error::Size)?;
                if bits == 1 {
                    byte = reverse_bits(byte);
                }
                let enc = (byte >> (8 - bits as usize * (i % per_byte + 1))) & sentinel;
                *slot = if enc == sentinel {
                    extra += 1;
                    *data.get(extra - 1).ok_or(Error::Size)?
                } else {
                    enc
                };
            }
            Ok(extra)
        }
    }
}

fn decode_bytes(data: &[u8], mut at: usize, out: &mut [u8], size: usize, bits: &[u8]) -> Result<usize, Error> {
    let header_size = (size / GROUP).div_ceil(4);
    let header = data.get(at..at + header_size).ok_or(Error::Size)?;
    at += header_size;
    for i in (0..size).step_by(GROUP) {
        if data.len() - at < DECODE_LIMIT {
            return Err(Error::Size);
        }
        let g = i / GROUP;
        let k = (header[g / 4] >> ((g % 4) * 2)) & 3;
        at = decode_group(data, at, &mut out[i..], bits[k as usize])?;
    }
    Ok(at)
}

/// Undo one channel's deltas into `target` (block-local vertices of `vertex_size` bytes).
fn decode_deltas(buffer: &[u8], target: &mut [u8], k: usize, count: usize, vertex_size: usize, last: &[u8], channel: u8) -> Result<(), Error> {
    let width = match channel & 3 {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => return Err(Error::Channel(channel)),
    };
    let rot = (32 - u32::from(channel >> 4)) & 31;
    for lane in (0..4).step_by(width) {
        let gather = |src: &dyn Fn(usize) -> u8| (0..width).fold(0u32, |v, j| v | u32::from(src(j)) << (8 * j));
        let mut p = gather(&|j| last[k + lane + j]);
        for i in 0..count {
            let raw = gather(&|j| buffer[lane * count + i + count * j]);
            let v = match width {
                4 => raw.rotate_left(rot) ^ p,
                1 => u32::from((0u8.wrapping_sub(raw as u8 & 1) ^ (raw as u8 >> 1)).wrapping_add(p as u8)),
                _ => u32::from((0u16.wrapping_sub(raw as u16 & 1) ^ (raw as u16 >> 1)).wrapping_add(p as u16)),
            };
            for j in 0..width {
                target[i * vertex_size + k + lane + j] = (v >> (8 * j)) as u8;
            }
            p = v;
        }
    }
    Ok(())
}

fn decode_block(data: &[u8], mut at: usize, out: &mut [u8], count: usize, vertex_size: usize, last: &mut [u8], channels: &[u8], version: u8) -> Result<usize, Error> {
    let aligned = count.next_multiple_of(GROUP);
    let mut buffer = [0u8; BLOCK_MAX * 4 + GROUP];
    let control_size = if version == 0 { 0 } else { vertex_size / 4 };
    let control = data.get(at..at + control_size).ok_or(Error::Size)?.to_vec();
    at += control_size;
    for k in (0..vertex_size).step_by(4) {
        let ctrl_byte = if version == 0 { 0 } else { control[k / 4] };
        for j in 0..4 {
            let dst = &mut buffer[j * count..];
            match (ctrl_byte >> (j * 2)) & 3 {
                3 => {
                    dst[..count].copy_from_slice(data.get(at..at + count).ok_or(Error::Size)?);
                    at += count;
                }
                2 => dst[..count].fill(0),
                ctrl => at = decode_bytes(data, at, dst, aligned, if version == 0 { &BITS_V0 } else { &BITS_V1[ctrl as usize..] })?,
            }
        }
        let channel = if version == 0 { 0 } else { channels[k / 4] };
        decode_deltas(&buffer, out, k, count, vertex_size, last, channel)?;
    }
    last.copy_from_slice(&out[vertex_size * (count - 1)..vertex_size * count]);
    Ok(at)
}

/// `count` vertices of `vertex_size` bytes (a multiple of 4, at most 256).
pub fn decode_vertex_buffer(data: &[u8], count: usize, vertex_size: usize) -> Result<Vec<u8>, Error> {
    if vertex_size == 0 || vertex_size > 256 || !vertex_size.is_multiple_of(4) {
        return Err(Error::Shape);
    }
    let header = *data.first().ok_or(Error::Size)?;
    let version = header & 0x0f;
    if header & 0xf0 != HEADER || version > 1 {
        return Err(Error::Header(header));
    }
    let (tail, tail_pad) = tail_size(version, vertex_size);
    if data.len() - 1 < tail_pad {
        return Err(Error::Size);
    }
    let tail = &data[data.len() - tail..];
    let mut last = tail[..vertex_size].to_vec();
    let channels = &tail[vertex_size..];
    let mut out = vec![0u8; count * vertex_size];
    let block = block_size(vertex_size);
    let (mut at, mut offset) = (1, 0);
    while offset < count {
        let n = block.min(count - offset);
        at = decode_block(data, at, &mut out[offset * vertex_size..(offset + n) * vertex_size], n, vertex_size, &mut last, channels, version)?;
        offset += n;
    }
    if data.len() - at != tail_pad {
        return Err(Error::Size);
    }
    Ok(out)
}

/// Encoded size of one group at `bits`; `None` when it can't be (non-zero group at 0 bits).
fn group_size(group: &[u8], bits: u8) -> Option<usize> {
    match bits {
        0 => group.iter().all(|&b| b == 0).then_some(0),
        8 => Some(GROUP),
        _ => Some(GROUP * bits as usize / 8 + group.iter().filter(|&&b| b >= (1 << bits) - 1).count()),
    }
}

fn encode_group(out: &mut Vec<u8>, group: &[u8], bits: u8) {
    match bits {
        0 => {}
        8 => out.extend_from_slice(group),
        _ => {
            let per_byte = 8 / bits as usize;
            let sentinel = (1u8 << bits) - 1;
            for chunk in group.chunks(per_byte) {
                let byte = chunk.iter().fold(0u8, |b, &v| (b << bits) | v.min(sentinel));
                out.push(if bits == 1 { reverse_bits(byte) } else { byte });
            }
            out.extend(group.iter().filter(|&&v| v >= sentinel));
        }
    }
}

/// Each group at the cheapest of `bits` (literal 8 bits wins ties, as meshoptimizer's does).
fn encode_bytes(out: &mut Vec<u8>, bytes: &[u8], bits: &[u8]) -> usize {
    let start = out.len();
    let header = out.len();
    out.resize(out.len() + (bytes.len() / GROUP).div_ceil(4), 0);
    for (g, group) in bytes.chunks(GROUP).enumerate() {
        let best = (0..4).rev().min_by_key(|&k| group_size(group, bits[k]).unwrap_or(usize::MAX)).unwrap();
        out[header + g / 4] |= (best as u8) << ((g % 4) * 2);
        encode_group(out, group, bits[best]);
    }
    out.len() - start
}

/// Byte-delta encoding (channel mode 0 everywhere); version 1 also picks a per-byte control mode.
pub fn encode_vertex_buffer(vertices: &[u8], vertex_size: usize, version: u8) -> Vec<u8> {
    assert!(vertex_size > 0 && vertex_size <= 256 && vertex_size.is_multiple_of(4) && version <= 1);
    let count = vertices.len() / vertex_size;
    let mut out = vec![HEADER | version];
    let first = vertices.get(..vertex_size).map_or_else(|| vec![0; vertex_size], <[u8]>::to_vec);
    let mut last = first.clone();
    let block = block_size(vertex_size);
    for start in (0..count).step_by(block) {
        let n = block.min(count - start);
        let aligned = n.next_multiple_of(GROUP);
        let control_at = out.len();
        out.resize(out.len() + if version == 0 { 0 } else { vertex_size / 4 }, 0);
        for k in 0..vertex_size {
            let mut deltas = vec![0u8; aligned];
            let mut p = last[k];
            for (i, d) in deltas.iter_mut().take(n).enumerate() {
                let v = vertices[(start + i) * vertex_size + k];
                let diff = v.wrapping_sub(p);
                *d = (diff << 1) ^ (0u8.wrapping_sub(diff >> 7));
                p = v;
            }
            if version == 0 {
                encode_bytes(&mut out, &deltas, &BITS_V0);
                continue;
            }
            let ctrl = if deltas.iter().all(|&d| d == 0) {
                2
            } else {
                let size = |c: usize| encode_bytes(&mut Vec::new(), &deltas, &BITS_V1[c..]);
                [(0, size(0)), (1, size(1)), (3, n)].into_iter().min_by_key(|&(_, s)| s).unwrap().0
            };
            out[control_at + k / 4] |= (ctrl as u8) << ((k % 4) * 2);
            match ctrl {
                3 => out.extend_from_slice(&deltas[..n]),
                2 => {}
                c => {
                    encode_bytes(&mut out, &deltas, &BITS_V1[c..]);
                }
            }
        }
        last.copy_from_slice(&vertices[(start + n - 1) * vertex_size..(start + n) * vertex_size]);
    }
    let (tail, tail_pad) = tail_size(version, vertex_size);
    out.resize(out.len() + tail_pad - tail, 0);
    out.extend_from_slice(&first);
    out.resize(out.len() + tail - vertex_size, 0);
    out
}
