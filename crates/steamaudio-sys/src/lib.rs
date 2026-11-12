//! Steam Audio's binaural effect with its default HRTF (CIPIC subject 124), the effect real engine2
//! links statically behind `CBinauralEffect`. Pinned at ValveSoftware/steam-audio v4.8.1; see
//! docs/audio.md.

use std::ffi::c_int;
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock};

#[repr(C)]
struct RawHrtf {
    _p: [u8; 0],
}
#[repr(C)]
struct RawBinaural {
    _p: [u8; 0],
}

unsafe extern "C" {
    fn ubx_sa_hrtf_create(rate: c_int, frame: c_int) -> *mut RawHrtf;
    fn ubx_sa_binaural_create(hrtf: *const RawHrtf, rate: c_int, frame: c_int) -> *mut RawBinaural;
    fn ubx_sa_binaural_destroy(effect: *mut RawBinaural);
    fn ubx_sa_binaural_apply(
        effect: *mut RawBinaural,
        hrtf: *const RawHrtf,
        direction: *const f32,
        bilinear: c_int,
        spatial_blend: f32,
        in_channels: c_int,
        input: *const *mut f32,
        frame: c_int,
        output: *const *mut f32,
    ) -> c_int;
}

/// The sample rate and block size every effect runs at (s&box's mixer: 44100 Hz, 512 samples).
pub const RATE: usize = 44100;
pub const FRAME: usize = 512;

struct Hrtf(NonNull<RawHrtf>);
// SAFETY: the database is only touched under `HRTF`'s lock.
unsafe impl Send for Hrtf {}

/// One database for the process. Lookups write its scratch buffers, so applies serialize on it.
static HRTF: OnceLock<Option<Mutex<Hrtf>>> = OnceLock::new();

fn hrtf() -> Option<&'static Mutex<Hrtf>> {
    HRTF.get_or_init(|| {
        // SAFETY: plain constructor; null on failure.
        NonNull::new(unsafe { ubx_sa_hrtf_create(RATE as c_int, FRAME as c_int) }).map(|p| Mutex::new(Hrtf(p)))
    })
    .as_ref()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    Nearest,
    Bilinear,
}

/// One voice's binaural effect: its overlap-add state carries the HRIR tail across blocks.
pub struct Binaural(NonNull<RawBinaural>);
// SAFETY: the effect has no thread affinity; `&mut self` serializes its use.
unsafe impl Send for Binaural {}

impl Binaural {
    /// None if the HRTF failed to load.
    pub fn new() -> Option<Self> {
        let db = hrtf()?.lock().unwrap();
        // SAFETY: `db` is a live database; the effect only reads its size here.
        NonNull::new(unsafe { ubx_sa_binaural_create(db.0.as_ptr(), RATE as c_int, FRAME as c_int) }).map(Self)
    }

    /// Convolves one block (1 or 2 channels) with the HRTF for `direction`, in Steam Audio's
    /// listener space (+x right, +y up, -z ahead), into `out` (left, right).
    pub fn apply(&mut self, direction: [f32; 3], interpolation: Interpolation, spatial_blend: f32, input: &mut [[f32; FRAME]], out: &mut [[f32; FRAME]; 2]) -> bool {
        if !(1..=2).contains(&input.len()) {
            return false;
        }
        let Some(db) = hrtf() else { return false };
        let db = db.lock().unwrap();
        let ins: Vec<*mut f32> = input.iter_mut().map(|c| c.as_mut_ptr()).collect();
        let outs = out.each_mut().map(|c| c.as_mut_ptr());
        // SAFETY: every channel pointer covers FRAME floats and outlives the call; the database is locked.
        unsafe {
            ubx_sa_binaural_apply(
                self.0.as_ptr(),
                db.0.as_ptr(),
                direction.as_ptr(),
                (interpolation == Interpolation::Bilinear) as c_int,
                spatial_blend,
                ins.len() as c_int,
                ins.as_ptr(),
                FRAME as c_int,
                outs.as_ptr(),
            ) != 0
        }
    }
}

impl Drop for Binaural {
    fn drop(&mut self) {
        // SAFETY: created by ubx_sa_binaural_create, dropped once.
        unsafe { ubx_sa_binaural_destroy(self.0.as_ptr()) }
    }
}

#[cfg(test)]
mod tests;
