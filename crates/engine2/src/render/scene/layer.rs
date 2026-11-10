//! `ISceneLayer`: one step of a view, which clears, draws matching scene objects, calls a managed
//! procedural callback or blits. `m_nLayerFlags`, `LayerEnum`, `m_viewport` and `m_nClearFlags`
//! are generated fields managed writes directly; the rest lives here.

use super::{field, view};
use crate::render::{attributes, texture};
use abi::slots::i_scene_layer as l;
use abi::structs::{RenderViewport, Vector4};
use abi::{NativeRef, Registry, StrRet, StringToken, bind, class};
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::ISceneLayer>;

/// `SceneLayerType` values the executor tells apart.
pub mod layer_type {
    pub const TRANSLUCENT: i64 = 1;
    pub const DEPTH_PREPASS: i64 = 7;
    pub const OPAQUE: i64 = 8;
}

/// `LayerFlags` bits the executor reads.
pub mod flags {
    pub const LIGHT_BINNER_SETUP: i64 = 0x1_0000;
    pub const IGNORE_LAYER_MATCH_ID: i64 = 0x400_0000;
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// Draws objects in this shader mode.
    Render(StringToken),
    /// Clears with `ClearFlags`.
    Clear(i32),
    Label,
    /// A managed `void(ManagedRenderSetup_t)` function pointer.
    Procedural(usize),
    Blit { src: texture::Ref, src_viewport: RenderViewport },
}

#[derive(Default)]
pub struct LayerState {
    pub color: texture::Ref,
    pub depth: texture::Ref,
    pub clear_color: Vector4,
    pub required: i64,
    pub excluded: i64,
    pub match_id: u32,
    pub shader_mode: Option<StringToken>,
    /// Whether `SetOutput` ran; layers without it draw into the view's own targets.
    pub output_set: bool,
}

pub struct SceneLayer {
    pub name: String,
    pub kind: Kind,
    pub view: view::Ref,
    pub attributes: attributes::Ref,
    pub state: Mutex<LayerState>,
}

pub fn create(view: view::Ref, name: String, viewport: RenderViewport, kind: Kind, required: i64, excluded: i64) -> Ref {
    let state = LayerState { required, excluded, ..Default::default() };
    let r: Ref = objects::create(SceneLayer { name, kind, view, attributes: attributes::create(), state: Mutex::new(state) });
    field!(set RenderViewport, l::GET_M_VIEWPORT, r.addr(), viewport);
    if let Kind::Clear(flags) = kind {
        field!(set i32, l::GET_M_N_CLEAR_FLAGS, r.addr(), flags);
    }
    r
}

pub fn destroy(r: Ref) {
    if let Some(layer) = get(r) {
        objects::destroy(layer.attributes);
    }
    objects::destroy(r);
}

pub fn get(r: Ref) -> Option<Arc<SceneLayer>> {
    objects::get::<_, SceneLayer>(r)
}

pub fn with<R: Default>(r: Ref, f: impl FnOnce(&mut LayerState) -> R) -> R {
    get(r).map(|layer| f(&mut layer.state.lock().unwrap())).unwrap_or_default()
}

pub fn set_output(r: Ref, color: texture::Ref, depth: texture::Ref) {
    with(r, |s| (s.color, s.depth, s.output_set) = (color, depth, true));
}

pub fn layer_flags(r: Ref) -> i64 {
    field!(get i64, l::GET_M_N_LAYER_FLAGS, r.addr())
}

pub fn layer_type(r: Ref) -> i64 {
    field!(get i64, l::GET_LAYER_ENUM, r.addr())
}

pub fn viewport(r: Ref) -> RenderViewport {
    field!(get RenderViewport, l::GET_M_VIEWPORT, r.addr())
}

pub fn clear_flags(r: Ref) -> i32 {
    field!(get i32, l::GET_M_N_CLEAR_FLAGS, r.addr())
}

pub fn register(r: &mut Registry) {
    bind!(r, l::SET_OBJECT_MATCH_ID, |r, id| with(r, |s| s.match_id = id.0));
    bind!(r, l::ADD_OBJECT_FLAGS_REQUIRED_MASK, |r, f| with(r, |s| s.required |= f));
    bind!(r, l::ADD_OBJECT_FLAGS_EXCLUDED_MASK, |r, f| with(r, |s| s.excluded |= f));
    bind!(r, l::REMOVE_OBJECT_FLAGS_REQUIRED_MASK, |r, f| with(r, |s| s.required &= !f));
    bind!(r, l::REMOVE_OBJECT_FLAGS_EXCLUDED_MASK, |r, f| with(r, |s| s.excluded &= !f));
    bind!(r, l::GET_OBJECT_FLAGS_REQUIRED_MASK, |r| with(r, |s| s.required));
    bind!(r, l::GET_OBJECT_FLAGS_EXCLUDED_MASK, |r| with(r, |s| s.excluded));
    bind!(r, l::GET_DEBUG_NAME, |r| StrRet::from(get(r).map(|l| l.name.clone())));
    // Layers run in order on one queue (execute.rs), so semaphores have nothing to order.
    bind!(r, l::ADD_SEMAPHORE_DEPENDENCY, |_, _| {});
    bind!(r, l::SET_SEMAPHORE, |_, _| {});
    bind!(r, l::GET_RENDER_ATTRIBUTES_PTR, |r| get(r).map(|l| l.attributes).unwrap_or_default());
    // TODO: per-layer texture attributes (SetAttr) feed shader bindings once combos translate.
    bind!(r, l::SET_ATTR, |_, _, _, _, _| {});
    bind!(r, l::SET_LAYER_NO_CULL, |_, _| {});
    bind!(r, l::SET_BOUNDING_VOLUME_SIZE_CULL_THRESHOLD_IN_PERCENT, |_, _| {});
    bind!(r, l::DISABLE_REFLECTIONS_AND_REFRACTIONS_FOR_OBJECTS_IN_THIS_LAYER, |_, _| {});
    bind!(r, l::SET_FADE_TARGET_LAYER, |_, _| {});
    bind!(r, l::SET_TONEMAP_OVERRIDE_SCALE_VALUE, |_, _| {});
    bind!(r, l::SET_SHADER_MODE, |r, mode| with(r, |s| s.shader_mode = Some(mode)));
    bind!(r, l::SET_WIREFRAME_OVERRIDE, |_| {});
    bind!(r, l::SET_CLEAR_COLOR, |r, c, _target| with(r, |s| s.clear_color = c));
    bind!(r, l::GET_TEXTURE_VALUE, |_, _, default| default);
    bind!(r, l::GET_TEXTURE_VALUE_1, |_, _| texture::Ref::default());
    bind!(r, l::GET_COLOR_TARGET, |r| with(r, |s| s.color));
    bind!(r, l::GET_DEPTH_TARGET, |r| with(r, |s| s.depth));
    bind!(r, l::SET_OUTPUT, |r, c, d| set_output(r, view::rt_texture(c), view::rt_texture(d)));
    bind!(r, l::SET_OUTPUT_1, |r, c, d, _color_space| set_output(r, view::rt_texture(c), view::rt_texture(d)));
    bind!(r, l::SET_OVERLAY_STENCIL, |_, _| {});
}
