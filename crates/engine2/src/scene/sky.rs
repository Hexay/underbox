//! `CSceneSkyBoxObject`'s own settings. They live as generated fields keyed by their getter, so
//! they go with the object (`objects::destroy` forgets them). The renderer draws the sky from the
//! object's tint (render/scene/execute.rs); material, fog and lighting are kept for managed.

use super::object;
use crate::render::scene::field;
use abi::slots::c_scene_sky_box_object as sky;
use abi::structs::Vector3;
use abi::{NativeRef, Registry, bind, class};

type Sky = NativeRef<class::CSceneSkyBoxObject>;

fn tint(s: Sky) -> Vector3 {
    let t = object::with(s.cast(), |st| Some(st.tint)).unwrap_or_else(|| crate::math::vec4(1.0, 1.0, 1.0, 1.0)).vec;
    crate::math::vec3(t.x, t.y, t.z)
}

pub fn register(r: &mut Registry) {
    bind!(r, sky::GET_SKY_TINT, tint);
    bind!(r, sky::SET_MATERIAL, |s: Sky, m: NativeRef<class::IMaterial2>| field!(set NativeRef<class::IMaterial2>, sky::GET_MATERIAL, s.addr(), m));
    bind!(r, sky::GET_MATERIAL, |s: Sky| field!(get NativeRef<class::IMaterial2>, sky::GET_MATERIAL, s.addr()));
    bind!(r, sky::SET_FOG_TYPE, |s: Sky, t| field!(set i32, sky::GET_FOG_TYPE, s.addr(), t));
    bind!(r, sky::GET_FOG_TYPE, |s: Sky| field!(get i32, sky::GET_FOG_TYPE, s.addr()));
    bind!(r, sky::SET_ANGULAR_FOG_PARAMS, |s: Sky, min_start, min_end, max_start, max_end| {
        let a = s.addr();
        field!(set f32, sky::GET_FOG_MIN_START, a, min_start);
        field!(set f32, sky::GET_FOG_MIN_END, a, min_end);
        field!(set f32, sky::GET_FOG_MAX_START, a, max_start);
        field!(set f32, sky::GET_FOG_MAX_END, a, max_end);
    });
    bind!(r, sky::GET_FOG_MIN_START, |s: Sky| field!(get f32, sky::GET_FOG_MIN_START, s.addr()));
    bind!(r, sky::GET_FOG_MIN_END, |s: Sky| field!(get f32, sky::GET_FOG_MIN_END, s.addr()));
    bind!(r, sky::GET_FOG_MAX_START, |s: Sky| field!(get f32, sky::GET_FOG_MAX_START, s.addr()));
    bind!(r, sky::GET_FOG_MAX_END, |s: Sky| field!(get f32, sky::GET_FOG_MAX_END, s.addr()));
    // TODO: sky lighting feeds ambient light once the renderer lights with more than one direction.
    bind!(r, sky::SET_LIGHTING_CONSTANT_COLOR_HEMISPHERE, |_, _| {});
    bind!(r, sky::SET_LIGHTING_SAMPLES, |_, _, _, _| {});
}
