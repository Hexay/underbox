use super::*;
use abi::Guid;
use assets::compiled::{fixture, texture as vtex};
use std::sync::Arc;

/// The fixture's sheet through a compiled file, a texture, and the objects managed reads.
#[test]
fn compiled_sheet_sequences_reach_managed() {
    let t = vtex::read(&vtex::write(&fixture::sprite_sheet(), true)).unwrap();
    assert_eq!(t.sheet, fixture::sheet_sequences(), "the SHEET extra data round-trips");
    let decoded = crate::compiled::texture::decode(&t).unwrap();
    let r: texture::Ref = crate::resource::new_handle(Arc::new(texture::decoded("sheet", decoded, Guid::default())));
    assert_eq!(count(r), 2);

    let cycle = get(r, 0);
    assert!(get(r, 0) == cycle, "one object per sequence");
    assert_eq!(abi::rt::fields::get::<f32, { seq::GET_M_FL_TOTAL_TIME.index }>(cycle.as_raw()), 0.3);
    assert!(!abi::rt::fields::get::<bool, { seq::GET_M_B_CLAMP.index }>(cycle.as_raw()), "loops");
    let fr = frames(cycle);
    assert_eq!(fr.len(), 3);
    assert_eq!(abi::rt::fields::get::<f32, { frame::GET_M_FL_DISPLAY_TIME.index }>(fr[2].as_raw()), 0.1);
    let img = image(fr[1], 0);
    assert_eq!([img.uv1.vec.x, img.uv1.vec.y, img.uv1.vec.z, img.uv1.vec.w], [0.5, 0.0, 1.0, 0.5], "the green cell");
    assert_eq!(image(fr[1], 1).uv1.vec.z, 0.0, "past the last image reads as zero");

    let still = get(r, 1);
    assert_eq!(abi::rt::fields::get::<u32, { seq::GET_M_N_ID.index }>(still.as_raw()), 1);
    assert!(abi::rt::fields::get::<bool, { seq::GET_M_B_CLAMP.index }>(still.as_raw()));
    assert!(get(r, 2) == NativeRef::NULL && get(r, -1) == NativeRef::NULL);

    crate::resource::destroy_handle(r);
    assert!(objects::get::<_, Frames>(cycle).is_none() && objects::get::<_, Vec<[f32; 8]>>(fr[0]).is_none(), "sequences and frames go with their texture");
}
