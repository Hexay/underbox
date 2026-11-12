//! `CDecalSceneObject`'s texture getters. The fields stay generic (`bind_fields`; render/lighting/
//! decals.rs reads them), but a getter hands out a strong handle as native's do: managed's
//! `Texture.FromNative` throws on null and destroys the handle it's given once it has a wrapper,
//! so an unset texture is an empty handle and a set one a copy.

use crate::render::texture::{self, Texture};
use crate::resource;
use abi::slots::c_decal_scene_object as d;
use abi::{Registry, bind};
use std::ffi::c_void;

fn handed_out<const KEY: usize>(this: *mut c_void) -> texture::Ref {
    let t = abi::rt::fields::get::<texture::Ref, KEY>(this);
    resource::handed_out::<_, Texture>((!t.is_null()).then_some(t))
}

pub fn register(r: &mut Registry) {
    bind!(r, d::GET_M_H_COLOR, handed_out::<{ d::GET_M_H_COLOR.index }>);
    bind!(r, d::GET_M_H_NORMAL, handed_out::<{ d::GET_M_H_NORMAL.index }>);
    bind!(r, d::GET_M_H_RMO, handed_out::<{ d::GET_M_H_RMO.index }>);
    bind!(r, d::GET_M_H_HEIGHT, handed_out::<{ d::GET_M_H_HEIGHT.index }>);
    bind!(r, d::GET_M_H_EMISSION, handed_out::<{ d::GET_M_H_EMISSION.index }>);
}
