//! Sprite sheets: a compiled texture's SHEET sequences (`assets::compiled::texture::sheet`) as the
//! `SheetSequence_t`/`SheetSequenceFrame_t` objects `Texture.UpdateSheetInfo` reads through
//! `g_pRenderDevice.GetSequenceCount`/`GetSequence`. A sequence's fields (`m_nId`, `m_bClamp`,
//! ..., `m_flTotalTime`) live in the generated field store; the objects are made on first ask and
//! live as long as their texture.

use crate::math::vec4;
use crate::render::texture;
use abi::slots::{g_p_render_device as device, sheet_sequence_frame_t as frame, sheet_sequence_t as seq};
use abi::structs::SheetFrameImage_t;
use abi::{NativeRef, Registry, bind, class};
use assets::compiled::texture::sheet::Sequence;
use std::sync::OnceLock;

/// A texture's sequences, and the objects handed to managed for them.
#[derive(Default)]
pub struct Sheet {
    pub sequences: Vec<Sequence>,
    objects: OnceLock<Vec<NativeRef<class::SheetSequence_t>>>,
}

impl Sheet {
    pub fn new(sequences: Vec<Sequence>) -> Sheet {
        Sheet { sequences, objects: OnceLock::new() }
    }

    fn objects(&self) -> &[NativeRef<class::SheetSequence_t>] {
        self.objects.get_or_init(|| self.sequences.iter().map(sequence).collect())
    }
}

impl Drop for Sheet {
    fn drop(&mut self) {
        self.objects.get().into_iter().flatten().for_each(|s| {
            objects::destroy(*s);
        });
    }
}

struct Frames(Vec<NativeRef<class::SheetSequenceFrame_t>>);

impl Drop for Frames {
    fn drop(&mut self) {
        self.0.iter().for_each(|f| {
            objects::destroy(*f);
        });
    }
}

macro_rules! field {
    ($obj:expr, $getter:path, $ty:ty, $v:expr) => {
        abi::rt::fields::set::<$ty, { $getter.index }>($obj.as_raw(), $v)
    };
}

/// A sequence object for managed, its frames made with it.
fn sequence(s: &Sequence) -> NativeRef<class::SheetSequence_t> {
    let frames = s.frames.iter().map(|f| {
        let r = objects::create::<class::SheetSequenceFrame_t, _>(f.images.clone());
        field!(r, frame::GET_M_FL_DISPLAY_TIME, f32, f.display_time);
        r
    });
    let r = objects::create::<class::SheetSequence_t, _>(Frames(frames.collect()));
    field!(r, seq::GET_M_N_ID, u32, s.id);
    field!(r, seq::GET_M_B_CLAMP, bool, s.clamp);
    field!(r, seq::GET_M_B_ALPHA_CROP, bool, s.alpha_crop);
    field!(r, seq::GET_M_B_NO_COLOR, bool, s.no_color);
    field!(r, seq::GET_M_B_NO_ALPHA, bool, s.no_alpha);
    field!(r, seq::GET_M_FL_TOTAL_TIME, f32, s.total_time);
    r
}

fn frames(s: NativeRef<class::SheetSequence_t>) -> Vec<NativeRef<class::SheetSequenceFrame_t>> {
    objects::get::<_, Frames>(s).map(|f| f.0.clone()).unwrap_or_default()
}

fn image(f: NativeRef<class::SheetSequenceFrame_t>, i: i32) -> SheetFrameImage_t {
    let images = objects::get::<_, Vec<[f32; 8]>>(f);
    let Some(m) = images.as_ref().and_then(|v| v.get(usize::try_from(i).ok()?)) else { return SheetFrameImage_t::default() };
    SheetFrameImage_t { uv1: vec4(m[0], m[1], m[2], m[3]), uv2: vec4(m[4], m[5], m[6], m[7]) }
}

pub fn count(t: texture::Ref) -> i32 {
    texture::resolve(t).map_or(0, |t| t.sheet.sequences.len() as i32)
}

/// Sequence `i` of `t`, or null past the end.
pub fn get(t: texture::Ref, i: i32) -> NativeRef<class::SheetSequence_t> {
    let t = texture::resolve(t);
    let s = t.as_ref().and_then(|t| t.sheet.objects().get(usize::try_from(i).ok()?).copied());
    s.unwrap_or(NativeRef::NULL)
}

pub fn register(r: &mut Registry) {
    bind!(r, device::GET_SEQUENCE_COUNT, count);
    bind!(r, device::GET_SEQUENCE, get);
    bind!(r, seq::FRAME_COUNT, |s| frames(s).len() as i32);
    bind!(r, frame::IMAGE_COUNT, |f| objects::get::<_, Vec<[f32; 8]>>(f).map_or(0, |v| v.len() as i32));
    bind!(r, frame::GET_IMAGE, image);
}

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;
