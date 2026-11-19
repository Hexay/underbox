//! `LZ4Glue`: standard LZ4 block and frame formats (snapshots go to real clients' decoders).
//! HC levels (blocks above 1, frames from 3, as liblz4) use [`crate::compress_hc`].

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

pub fn compress_block_hc(src: &[u8], dst: &mut [u8], level: i32) -> Option<usize> {
    let packed = crate::compress_hc::compress(src, level);
    dst.get_mut(..packed.len())?.copy_from_slice(&packed);
    Some(packed.len())
}

pub fn decompress_block(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    lz4_flex::block::decompress_into(src, dst).ok()
}

/// liblz4's frame API compresses at HC from level 3 (`LZ4HC_CLEVEL_MIN`).
pub fn compress_frame(src: &[u8], level: i32) -> Option<Vec<u8>> {
    if level >= 3 {
        return Some(crate::compress_hc::compress_frame(src, level));
    }
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
    bind!(r, lz4::COMPRESS_HC, |src, dst, n, cap, level| len(unsafe { compress_block_hc(bytes(src, n), bytes(dst, cap), level) }).max(0));
    bind!(r, lz4::DECOMPRESS, |src, dst, n, cap| len(unsafe { decompress_block(bytes(src, n), bytes(dst, cap)) }));
    bind!(r, lz4::COMPRESS_FRAME, |src, dst, n, cap, level| {
        let (src, dst) = unsafe { (bytes(src, n), bytes(dst, cap)) };
        match compress_frame(src, level) {
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
        let frame = compress_frame(&src, 1).unwrap();
        assert!(frame.len() <= frame_bound(src.len()));
        assert_eq!(frame_content_size(&frame), Some(src.len() as u64));
        let mut out = vec![0; src.len()];
        assert_eq!(decompress_frame(&frame, &mut out), Some(src.len()));
        assert_eq!(out, src);
        assert_eq!(frame_content_size(b"not a frame"), None);
    }

    /// Repetitive text with far, near and overlapping matches.
    fn text() -> Vec<u8> {
        let words = ["alpha ", "beta ", "gamma ", "delta ", "epsilon ", "zeta "];
        (0..30_000u32).flat_map(|i| words[(i.wrapping_mul(2_654_435_761) >> 29) as usize % 6].bytes()).chain([b'x'; 300]).collect()
    }

    #[test]
    fn hc_blocks_round_trip_smaller_than_plain() {
        let src = text();
        let mut plain = vec![0; block_bound(src.len())];
        let plain = compress_block(&src, &mut plain).unwrap();
        let mut last = usize::MAX;
        for level in [2, 9, 12] {
            let mut packed = vec![0; block_bound(src.len())];
            let n = compress_block_hc(&src, &mut packed, level).unwrap();
            let mut out = vec![0; src.len()];
            assert_eq!(decompress_block(&packed[..n], &mut out), Some(src.len()), "level {level}");
            assert_eq!(out, src);
            assert!(n < plain && n <= last, "level {level}: {n} against plain {plain}, previous level {last}");
            last = n;
        }
        for len in 0..40 {
            let src = &sample()[..len];
            let mut packed = vec![0; block_bound(len)];
            let n = compress_block_hc(src, &mut packed, 12).unwrap();
            let mut out = vec![0; len];
            assert_eq!(decompress_block(&packed[..n], &mut out), Some(len));
            assert_eq!(out, src);
        }
        assert_eq!(compress_block_hc(&src, &mut [0; 16], 9), None, "too small a destination fails");
    }

    #[test]
    fn hc_frames_round_trip_within_bound() {
        let mut x = 0x1234_5678u32;
        let noise: Vec<u8> = (0..70_000).map(|_| { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as u8 }).collect();
        for src in [text(), sample(), noise, Vec::new()] {
            let frame = compress_frame(&src, 12).unwrap();
            assert!(frame.len() <= frame_bound(src.len()));
            assert_eq!(frame_content_size(&frame), Some(src.len() as u64));
            let mut out = vec![0; src.len()];
            assert_eq!(decompress_frame(&frame, &mut out), Some(src.len()));
            assert_eq!(out, src);
        }
        assert!(compress_frame(&text(), 12).unwrap().len() < compress_frame(&text(), 1).unwrap().len());
    }

    #[test]
    fn incompressible_frame_fits_bound() {
        let mut x = 0x1234_5678u32;
        let noise: Vec<u8> = (0..150_000).map(|_| { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as u8 }).collect();
        assert!(compress_frame(&noise, 1).unwrap().len() <= frame_bound(noise.len()));
    }
}
