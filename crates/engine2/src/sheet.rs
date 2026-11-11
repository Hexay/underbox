//! Sprite sheets (`Sheet_t`): `SheetSequence_t` and `SheetSequenceFrame_t`, as `Texture.UpdateSheetInfo`
//! reads them. A sequence's fields (`m_nId`, `m_bClamp`, ..., `m_flTotalTime`) live in the
//! generated field store, set when it's made.
//!
//! TODO: no texture has a sheet yet. `g_pRenderDevice.GetSequenceCount`/`GetSequence` (render
//! lane) answer none; they'd hand out [`sequence`] for a vtex's SHEET extra data, whose layout
//! (ValveResourceFormat `Texture.GetSpriteSheetData`) is unchecked against a shipped file.

use abi::slots::{sheet_sequence_frame_t as frame, sheet_sequence_t as seq};
use crate::math::vec4;
use abi::structs::SheetFrameImage_t;
use abi::{NativeRef, Registry, bind, class};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub display_time: f32,
    /// Per image: cropped UV min/max, then uncropped UV min/max.
    pub images: Vec<[f32; 8]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sequence {
    pub id: u32,
    pub clamp: bool,
    pub alpha_crop: bool,
    pub no_color: bool,
    pub no_alpha: bool,
    pub total_time: f32,
    pub frames: Vec<Frame>,
}

struct Live {
    frames: Vec<NativeRef<class::SheetSequenceFrame_t>>,
}

impl Drop for Live {
    fn drop(&mut self) {
        self.frames.iter().for_each(|f| {
            objects::destroy(*f);
        });
    }
}

macro_rules! field {
    ($obj:expr, $getter:path, $ty:ty, $v:expr) => {
        abi::rt::fields::set::<$ty, { $getter.index }>($obj.as_raw(), $v)
    };
}

/// A sequence object for managed, its frames made with it; free it with [`destroy`].
pub fn sequence(s: &Sequence) -> NativeRef<class::SheetSequence_t> {
    let frames = s.frames.iter().map(|f| {
        let r = objects::create::<class::SheetSequenceFrame_t, _>(f.images.clone());
        field!(r, frame::GET_M_FL_DISPLAY_TIME, f32, f.display_time);
        r
    });
    let r = objects::create::<class::SheetSequence_t, _>(Live { frames: frames.collect() });
    field!(r, seq::GET_M_N_ID, u32, s.id);
    field!(r, seq::GET_M_B_CLAMP, bool, s.clamp);
    field!(r, seq::GET_M_B_ALPHA_CROP, bool, s.alpha_crop);
    field!(r, seq::GET_M_B_NO_COLOR, bool, s.no_color);
    field!(r, seq::GET_M_B_NO_ALPHA, bool, s.no_alpha);
    field!(r, seq::GET_M_FL_TOTAL_TIME, f32, s.total_time);
    r
}

pub fn destroy(r: NativeRef<class::SheetSequence_t>) {
    objects::destroy(r);
}

/// The frame objects of a sequence, in order.
pub fn frames(r: NativeRef<class::SheetSequence_t>) -> Vec<NativeRef<class::SheetSequenceFrame_t>> {
    objects::get::<_, Live>(r).map(|l| l.frames.clone()).unwrap_or_default()
}

fn image(f: NativeRef<class::SheetSequenceFrame_t>, i: i32) -> SheetFrameImage_t {
    let images = objects::get::<_, Vec<[f32; 8]>>(f);
    let Some(m) = images.as_ref().and_then(|v| v.get(usize::try_from(i).ok()?)) else { return SheetFrameImage_t::default() };
    SheetFrameImage_t { uv1: vec4(m[0], m[1], m[2], m[3]), uv2: vec4(m[4], m[5], m[6], m[7]) }
}

pub fn register(r: &mut Registry) {
    bind!(r, seq::FRAME_COUNT, |s| frames(s).len() as i32);
    bind!(r, frame::IMAGE_COUNT, |f| objects::get::<_, Vec<[f32; 8]>>(f).map_or(0, |v| v.len() as i32));
    bind!(r, frame::GET_IMAGE, image);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_carry_frames_images_and_fields() {
        let s = Sequence {
            id: 7,
            clamp: true,
            total_time: 1.5,
            frames: vec![
                Frame { display_time: 0.5, images: vec![[0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.5, 0.5]] },
                Frame { display_time: 1.0, images: vec![] },
            ],
            ..Sequence::default()
        };
        let r = sequence(&s);
        let fr = frames(r);
        assert_eq!(fr.len(), 2);
        assert_eq!(abi::rt::fields::get::<u32, { seq::GET_M_N_ID.index }>(r.as_raw()), 7);
        assert!(abi::rt::fields::get::<bool, { seq::GET_M_B_CLAMP.index }>(r.as_raw()));
        assert_eq!(abi::rt::fields::get::<f32, { frame::GET_M_FL_DISPLAY_TIME.index }>(fr[1].as_raw()), 1.0);
        let img = image(fr[0], 0);
        assert_eq!((img.uv1.vec.z, img.uv2.vec.w), (0.5, 0.5));
        assert_eq!(image(fr[0], 1).uv1.vec.z, 0.0, "past the end reads as zero");
        destroy(r);
        assert!(objects::get::<_, Vec<[f32; 8]>>(fr[0]).is_none(), "frames go with their sequence");
    }
}
