//! AV1 through rav1d's dav1d-compatible API: the FFI wrapper, so the only `unsafe` in this crate.
//! 8-bit only (the crate is built with `bitdepth_8`).

use super::Frame;
use crate::yuv::{self, Matrix, Planes};
use rav1d::Dav1dResult;
use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::{DAV1D_MC_BT709, DAV1D_MC_UNKNOWN,DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib as dav1d;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

pub struct Decoder {
    ctx: Option<Dav1dContext>,
}

const AGAIN: i32 = -libc::EAGAIN;

fn check(r: Dav1dResult, what: &str) -> Result<(), String> {
    if r.0 == 0 { Ok(()) } else { Err(format!("rav1d {what}: error {}", r.0)) }
}

impl Decoder {
    pub fn new() -> Result<Self, String> {
        let mut s = MaybeUninit::<Dav1dSettings>::uninit();
        // SAFETY: default_settings writes a whole Dav1dSettings to the pointer.
        let mut s = unsafe {
            dav1d::dav1d_default_settings(NonNull::new_unchecked(s.as_mut_ptr()));
            s.assume_init()
        };
        s.n_threads = 2;
        s.max_frame_delay = 1;
        let mut ctx = None;
        // SAFETY: both pointers are live locals; open fills `ctx` on success.
        check(unsafe { dav1d::dav1d_open(NonNull::new(&mut ctx), NonNull::new(&mut s)) }, "open")?;
        Ok(Self { ctx })
    }

    /// Send one temporal unit and collect what's ready.
    pub fn decode(&mut self, unit: &[u8], pts: f64, out: &mut Vec<Frame>) -> Result<(), String> {
        let mut data = Dav1dData::default();
        // SAFETY: data_create allocates `unit.len()` bytes owned by `data` and returns them.
        let buf = unsafe { dav1d::dav1d_data_create(NonNull::new(&mut data), unit.len()) };
        if buf.is_null() {
            return Err("rav1d: out of memory".into());
        }
        // SAFETY: `buf` holds `unit.len()` writable bytes.
        unsafe { std::ptr::copy_nonoverlapping(unit.as_ptr(), buf, unit.len()) };
        data.m.timestamp = (pts * 1e6).round() as i64;
        let mut result = Ok(());
        for _ in 0..64 {
            // SAFETY: `ctx` is open and `data` is a dav1d-owned buffer it may consume.
            let r = unsafe { dav1d::dav1d_send_data(self.ctx, NonNull::new(&mut data)) };
            if r.0 != 0 && r.0 != AGAIN {
                result = check(r, "send_data");
                break;
            }
            if let Err(e) = self.drain(out) {
                result = Err(e);
                break;
            }
            if data.sz == 0 {
                break;
            }
        }
        // SAFETY: releases whatever `send_data` didn't consume (a no-op once it's empty).
        unsafe { dav1d::dav1d_data_unref(NonNull::new(&mut data)) };
        result
    }

    /// Every picture the decoder has ready.
    pub fn drain(&mut self, out: &mut Vec<Frame>) -> Result<(), String> {
        loop {
            let mut pic = Dav1dPicture::default();
            // SAFETY: `ctx` is open; on success `pic` holds a reference we release below.
            let r = unsafe { dav1d::dav1d_get_picture(self.ctx, NonNull::new(&mut pic)) };
            if r.0 == AGAIN {
                return Ok(());
            }
            check(r, "get_picture")?;
            let frame = convert(&pic);
            // SAFETY: `pic` came from get_picture and is released once.
            unsafe { dav1d::dav1d_picture_unref(NonNull::new(&mut pic)) };
            out.extend(frame);
        }
    }

    /// Drop everything in flight (a seek).
    pub fn reset(&mut self) {
        if let Some(ctx) = self.ctx {
            // SAFETY: `ctx` is open.
            unsafe { dav1d::dav1d_flush(ctx) };
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: closes the context once and sets it to None.
        unsafe { dav1d::dav1d_close(NonNull::new(&mut self.ctx)) };
    }
}

/// An 8-bit picture as RGBA; `None` for other depths or a picture without planes.
fn convert(pic: &Dav1dPicture) -> Option<Frame> {
    let (w, h) = (pic.p.w.max(0) as usize, pic.p.h.max(0) as usize);
    if pic.p.bpc != 8 || w == 0 || h == 0 {
        return None;
    }
    let layout = pic.p.layout;
    let (ss_x, ss_y) = (layout == DAV1D_PIXEL_LAYOUT_I420 || layout == DAV1D_PIXEL_LAYOUT_I422, layout == DAV1D_PIXEL_LAYOUT_I420);
    let (y_stride, uv_stride) = (pic.stride[0].unsigned_abs(), pic.stride[1].unsigned_abs());
    let chroma_rows = if ss_y { h.div_ceil(2) } else { h };
    let plane = |i: usize, len: usize| -> Option<&[u8]> {
        // SAFETY: dav1d's planes hold `stride * rows` bytes and live as long as `pic`'s reference.
        pic.data[i].map(|p| unsafe { std::slice::from_raw_parts(p.as_ptr().cast::<u8>(), len) })
    };
    let y = plane(0, y_stride * (h - 1) + w)?;
    let (u, v) = if layout == DAV1D_PIXEL_LAYOUT_I400 {
        (&[][..], &[][..])
    } else {
        let len = uv_stride * (chroma_rows - 1) + if ss_x { w.div_ceil(2) } else { w };
        (plane(1, len)?, plane(2, len)?)
    };
    // SAFETY: the sequence header lives as long as the picture's reference.
    let seq = pic.seq_hdr.map(|s| unsafe { s.as_ref() });
    let matrix = match seq.map(|s| s.mtrx) {
        Some(DAV1D_MC_BT709) => Matrix::Bt709,
        None | Some(DAV1D_MC_UNKNOWN) => Matrix::guess(h),
        Some(_) => Matrix::Bt601,
    };
    let full_range = seq.is_some_and(|s| s.color_range != 0);
    let planes = Planes { y, u, v, y_stride, uv_stride, width: w, height: h, ss_x, ss_y };
    let pts = pic.m.timestamp as f64 / 1e6;
    Some(Frame { pts, width: w as u32, height: h as u32, rgba: yuv::to_rgba(&planes, matrix, full_range) })
}
