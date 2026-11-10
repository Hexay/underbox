//! `FloatBitMap_t`: float images managed loads (cursors, TGA/PSD/TIF imports, `ImageLoader`) and
//! encodes into texture formats (`Bitmap.ToFormat`, VTexWriter). The image work is `assets::bitmap`.

use abi::slots::float_bit_map_t as fbm;
use abi::{NativeRef, RawPtr, Registry, bind, class};
use assets::bitmap::file::{self, Kind};
use assets::bitmap::{FloatBitmap, Gamma, format};
use std::ffi::{CStr, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type Ref = NativeRef<class::FloatBitMap_t>;

fn create(w: i32, h: i32) -> Ref {
    objects::create(Mutex::new(FloatBitmap::new(w.max(0) as usize, h.max(0) as usize, 1)))
}

fn with<R: Default>(r: Ref, f: impl FnOnce(&mut FloatBitmap) -> R) -> R {
    objects::get::<_, Mutex<FloatBitmap>>(r).map(|b| f(&mut b.lock().unwrap())).unwrap_or_default()
}

/// A path as given if it exists, else through the game's mounted search paths.
fn path(p: Option<&CStr>) -> PathBuf {
    let p = p.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    if Path::new(&p).exists() { PathBuf::from(p) } else { crate::fs::resolve(&p, "GAME").unwrap_or_else(|| PathBuf::from(p)) }
}

/// Replace `r`'s image with `loaded`; false (and unchanged) if that failed.
fn replace(r: Ref, loaded: Option<FloatBitmap>) -> bool {
    let (Some(b), Some(dst)) = (loaded, objects::get::<_, Mutex<FloatBitmap>>(r)) else { return false };
    *dst.lock().unwrap() = b;
    true
}

fn load_file(r: Ref, p: Option<&CStr>, kind: Option<Kind>, gamma: i64) -> bool {
    let p = path(p);
    let gamma = Gamma::from_native(gamma);
    let loaded = match kind {
        Some(kind) => std::fs::read(&p).ok().and_then(|bytes| file::load(&bytes, kind, gamma)),
        None => file::load_path(&p, gamma),
    };
    replace(r, loaded)
}

/// # Safety
/// `data` must point at `size` readable bytes for the call (managed pins the span).
unsafe fn bytes<'a>(data: RawPtr<c_void>, size: i32) -> &'a [u8] {
    unsafe { data.cast::<u8>().as_slice(size.max(0) as usize) }
}

fn load_memory(r: Ref, data: RawPtr<c_void>, size: i32, kind: Kind) -> bool {
    // SAFETY: managed passes a fixed buffer and its length.
    replace(r, file::load(unsafe { bytes(data, size) }, kind, Gamma::Linear))
}

fn write_file(r: Ref, p: Option<&CStr>, encode: impl FnOnce(&FloatBitmap) -> Option<Vec<u8>>) -> bool {
    let p = PathBuf::from(p.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
    with(r, |b| encode(b)).is_some_and(|data| std::fs::write(p, data).is_ok())
}

fn write_to_buffer(r: Ref, dst: RawPtr<c_void>, size: i32, format: i64, srgb: bool) -> bool {
    let Some(encoded) = with(r, |b| format::encode(b, format, srgb)) else { return false };
    if dst.is_null() || encoded.len() > size.max(0) as usize {
        return false;
    }
    // SAFETY: managed passes a fixed buffer of `size` bytes, checked above to fit.
    unsafe { dst.cast::<u8>().as_mut_slice(encoded.len()) }.copy_from_slice(&encoded);
    true
}

pub fn register(r: &mut Registry) {
    bind!(r, fbm::CREATE, || create(0, 0));
    bind!(r, fbm::CREATE_1, create);
    bind!(r, fbm::DELETE, |r| {
        objects::destroy(r);
    });
    bind!(r, fbm::INIT, |r, w: i32, h: i32, d: i32| with(r, |b| *b = FloatBitmap::new(w.max(0) as usize, h.max(0) as usize, d.max(1) as usize)));
    bind!(r, fbm::SHUTDOWN, |r| with(r, |b| *b = FloatBitmap::new(0, 0, 1)));

    bind!(r, fbm::LOAD_FROM_FILE, |r, p, gamma| load_file(r, p, None, gamma));
    bind!(r, fbm::LOAD_FROM_PFM, |r, p| load_file(r, p, Some(Kind::Pfm), 1));
    bind!(r, fbm::LOAD_FROM_EXR, |r, p| load_file(r, p, Some(Kind::Exr), 1));
    bind!(r, fbm::LOAD_FROM_PSD, |r, p, gamma| load_file(r, p, Some(Kind::Psd), gamma));
    bind!(r, fbm::LOAD_FROM_TIF, |r, p, gamma| load_file(r, p, Some(Kind::Tiff), gamma));
    bind!(r, fbm::LOAD_FROM_PNG, |r, p, gamma| load_file(r, p, Some(Kind::Png), gamma));
    bind!(r, fbm::LOAD_FROM_JPG, |r, p, gamma| load_file(r, p, Some(Kind::Jpeg), gamma));
    bind!(r, fbm::LOAD_FROM_IN_MEMORY_TGA, |r, data, size| load_memory(r, data, size, Kind::Tga));
    bind!(r, fbm::LOAD_FROM_IN_MEMORY_PSD, |r, data, size| load_memory(r, data, size, Kind::Psd));
    bind!(r, fbm::LOAD_FROM_IN_MEMORY_TIF, |r, data, size| load_memory(r, data, size, Kind::Tiff));
    bind!(r, fbm::LOAD_FROM_BUFFER, |r, data, size, fmt, gamma| {
        with(r, |b| {
            // SAFETY: managed passes the bitmap's pixels and their byte count.
            let src = unsafe { bytes(data, size) };
            if let Some(decoded) = format::decode(src, b.width, b.height, fmt, Gamma::from_native(gamma)) {
                *b = decoded;
            }
        })
    });

    bind!(r, fbm::WRITE_TGA_FILE, |r, p| write_file(r, p, |b| Some(file::write_tga(b))));
    bind!(r, fbm::WRITE_PFM, |r, p| write_file(r, p, |b| Some(file::write_pfm(b))));
    bind!(r, fbm::WRITE_EXR, |r, p, _compression| write_file(r, p, file::write_exr));
    bind!(r, fbm::WRITE_TO_BUFFER, |r, dst, size, fmt, _fast, srgb, _flags| write_to_buffer(r, dst, size, fmt, srgb));

    bind!(r, fbm::SET_CHANNEL, |r, c: i32, v| with(r, |b| b.set_channel(c.max(0) as usize, v)));
    bind!(r, fbm::ROTATE90_DEGREES_CW, |r| with(r, FloatBitmap::rotate_90_cw));
    bind!(r, fbm::ROTATE90_DEGREES_CCW, |r| with(r, FloatBitmap::rotate_90_ccw));
    bind!(r, fbm::ROTATE180_DEGREES, |r| with(r, FloatBitmap::rotate_180));
    bind!(r, fbm::MIRROR_HORIZONTALLY, |r| with(r, FloatBitmap::mirror_horizontally));
    bind!(r, fbm::MIRROR_VERTICALLY, |r| with(r, FloatBitmap::mirror_vertically));
    bind!(r, fbm::RESIZE2_D, |r, w: i32, h: i32, clamp| with(r, |b| b.resize_2d(w.max(0) as usize, h.max(0) as usize, clamp)));

    bind!(r, fbm::PIXEL, |r, x, y, z, c| with(r, |b| b.pixel(x, y, z, c)));
    bind!(r, fbm::PIXEL_WRAPPED, |r, x, y, z, c| with(r, |b| b.pixel_wrapped(x, y, z, c)));
    bind!(r, fbm::PIXEL_CLAMPED, |r, x, y, z, c| with(r, |b| b.pixel_clamped(x, y, z, c)));
    bind!(r, fbm::ALPHA, |r, x, y, z| with(r, |b| b.pixel(x, y, z, 3)));
    bind!(r, fbm::RGB_PIXEL_AS_VECTOR, |r, x, y, z| with(r, |b| crate::math::vec3(b.pixel(x, y, z, 0), b.pixel(x, y, z, 1), b.pixel(x, y, z, 2))));
    bind!(r, fbm::WIDTH, |r| with(r, |b| b.width as i32));
    bind!(r, fbm::HEIGHT, |r| with(r, |b| b.height as i32));
    bind!(r, fbm::DEPTH, |r| with(r, |b| b.depth as i32));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_like_bitmap_to_format() {
        let r = create(2, 1);
        let src = [255u8, 0, 0, 255, 0, 0, 255, 128];
        replace(r, format::decode(&src, 2, 1, 0, Gamma::Linear));
        let mut out = [0u8; 8];
        let ptr = RawPtr::from_raw(out.as_mut_ptr().cast());
        assert!(write_to_buffer(r, ptr, 8, 12, false), "BGRA8888");
        assert_eq!(out, [0, 0, 255, 255, 255, 0, 0, 128]);
        assert!(!write_to_buffer(r, ptr, 7, 12, false), "too small a buffer");
        assert_eq!(with(r, |b| (b.width, b.height)), (2, 1));
        assert!(objects::destroy(r));
        assert!(!replace(r, Some(FloatBitmap::new(1, 1, 1))), "dead handle");
    }
}
