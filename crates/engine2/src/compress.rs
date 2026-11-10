//! `LZ4Glue`: standard LZ4 block and frame formats (snapshots go to real clients' decoders).
//! HC levels compress as regular LZ4: same format, lower ratio. TODO: an HC encoder.

use abi::slots::lz4_glue as lz4;
use abi::{RawPtr, Registry, bind};
use lz4_flex::frame::{FrameDecoder, FrameEncoder, FrameInfo};
use std::ffi::c_void;
use std::io::{Read, Write};

const FRAME_MAGIC: u32 = 0x184D_2204;
/// Worst-case frame header: magic, FLG, BD, content size, dictionary id, header checksum.
const FRAME_HEADER_MAX: usize = 4 + 2 + 8 + 4 + 1;
const FRAME_BLOCK: usize = 64 * 1024;

fn block_bound(n: usize) -> usize {
    lz4_flex::block::get_maximum_output_size(n)
}

/// Every block stored uncompressed (4-byte size each), the end mark and content checksum.
fn frame_bound(n: usize) -> usize {
    FRAME_HEADER_MAX + n + 4 * n.div_ceil(FRAME_BLOCK).max(1) + 4 + 4
}

pub fn compress_block(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    lz4_flex::block::compress_into(src, dst).ok()
}

pub fn decompress_block(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    lz4_flex::block::decompress_into(src, dst).ok()
}

pub fn compress_frame(src: &[u8]) -> Option<Vec<u8>> {
    let info = FrameInfo::new().content_size(Some(src.len() as u64));
    let mut enc = FrameEncoder::with_frame_info(info, Vec::with_capacity(frame_bound(src.len())));
    enc.write_all(src).ok()?;
    enc.finish().ok()
}

/// The frame's declared content size: 0 if it has none, `None` if it isn't an LZ4 frame.
pub fn frame_content_size(src: &[u8]) -> Option<u64> {
    let magic = u32::from_le_bytes(src.get(..4)?.try_into().ok()?);
    let flg = *src.get(4)?;
    if magic != FRAME_MAGIC || flg >> 6 != 1 {
        return None;
    }
    if flg & 0x08 == 0 {
        return Some(0);
    }
    Some(u64::from_le_bytes(src.get(6..14)?.try_into().ok()?))
}

pub fn decompress_frame(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    let mut out = Vec::with_capacity(dst.len());
    FrameDecoder::new(src).take(dst.len() as u64 + 1).read_to_end(&mut out).ok()?;
    let dst = dst.get_mut(..out.len())?;
    dst.copy_from_slice(&out);
    Some(out.len())
}

/// # Safety
/// `p` must point at `len` readable (or, with `mut`, writable) bytes for the call.
unsafe fn bytes<'a>(p: RawPtr<c_void>, len: i32) -> &'a mut [u8] {
    if p.is_null() || len <= 0 { &mut [] } else { unsafe { std::slice::from_raw_parts_mut(p.as_ptr().cast(), len as usize) } }
}

fn len(n: Option<usize>) -> i32 {
    n.map_or(-1, |n| n as i32)
}

pub fn register(r: &mut Registry) {
    bind!(r, lz4::COMPRESS_BOUND, |n| block_bound(n.max(0) as usize) as i32);
    bind!(r, lz4::COMPRESS_FRAME_BOUND, |n| frame_bound(n.max(0) as usize) as i32);
    // SAFETY (all below): managed pins both spans and passes their lengths.
    bind!(r, lz4::COMPRESS, |src, dst, n, cap| len(unsafe { compress_block(bytes(src, n), bytes(dst, cap)) }).max(0));
    bind!(r, lz4::COMPRESS_HC, |src, dst, n, cap, _level| len(unsafe { compress_block(bytes(src, n), bytes(dst, cap)) }).max(0));
    bind!(r, lz4::DECOMPRESS, |src, dst, n, cap| len(unsafe { decompress_block(bytes(src, n), bytes(dst, cap)) }));
    bind!(r, lz4::COMPRESS_FRAME, |src, dst, n, cap, _level| {
        let (src, dst) = unsafe { (bytes(src, n), bytes(dst, cap)) };
        match compress_frame(src) {
            Some(f) if f.len() <= dst.len() => {
                dst[..f.len()].copy_from_slice(&f);
                f.len() as i32
            }
            _ => 0,
        }
    });
    bind!(r, lz4::GET_FRAME_CONTENT_SIZE, |src, n| frame_content_size(unsafe { bytes(src, n) }).map_or(-1, |s| s as i64));
    bind!(r, lz4::DECOMPRESS_FRAME, |src, dst, n, cap| len(unsafe { decompress_frame(bytes(src, n), bytes(dst, cap)) }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        (0..200_000u32).map(|i| (i % 251) as u8 ^ (i / 1000) as u8).collect()
    }

    #[test]
    fn block_round_trip() {
        let src = sample();
        let mut packed = vec![0; block_bound(src.len())];
        let n = compress_block(&src, &mut packed).unwrap();
        assert!(n < src.len());
        let mut out = vec![0; src.len()];
        assert_eq!(decompress_block(&packed[..n], &mut out), Some(src.len()));
        assert_eq!(out, src);
        assert_eq!(decompress_block(&packed[..n], &mut out[..10]), None, "too small a destination fails");
    }

    #[test]
    fn frame_round_trip_with_content_size() {
        let src = sample();
        let frame = compress_frame(&src).unwrap();
        assert!(frame.len() <= frame_bound(src.len()));
        assert_eq!(frame_content_size(&frame), Some(src.len() as u64));
        let mut out = vec![0; src.len()];
        assert_eq!(decompress_frame(&frame, &mut out), Some(src.len()));
        assert_eq!(out, src);
        assert_eq!(frame_content_size(b"not a frame"), None);
    }

    #[test]
    fn incompressible_frame_fits_bound() {
        let mut x = 0x1234_5678u32;
        let noise: Vec<u8> = (0..150_000).map(|_| { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as u8 }).collect();
        assert!(compress_frame(&noise).unwrap().len() <= frame_bound(noise.len()));
    }
}
