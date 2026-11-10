//! `ISceneView`: one camera's render of the worlds into a pair of targets, as an ordered list of
//! layers. Views live for one `CCameraRenderer` render (camera.rs makes and frees them).
//! Render targets (`HSceneViewRenderTarget`) are texture handles by address; managed's
//! `NoTarget` is -1.

use super::{field, layer};
use crate::frustum::camera::Camera;
use crate::render::{attributes, swap_chain, texture};
use crate::scene::world;
use abi::slots::i_scene_view as v;
use abi::structs::RenderViewport;
use abi::{NativeRef, PtrHandle, RawPtr, Registry, bind, class, handle};
use glam::Mat4;
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::ISceneView>;
pub type RtHandle = PtrHandle<handle::HSceneViewRenderTarget>;

pub fn rt_handle(t: texture::Ref) -> RtHandle {
    if t.is_null() { RtHandle::new(usize::MAX) } else { RtHandle::new(t.addr()) }
}

pub fn rt_texture(h: RtHandle) -> texture::Ref {
    if h.value == 0 || h.value == usize::MAX { texture::Ref::default() } else { texture::Ref::from_raw(h.value as *mut _) }
}

#[derive(Default)]
pub struct ViewState {
    pub layers: Vec<layer::Ref>,
    pub parent: Ref,
    pub priority: i32,
    pub required: i64,
    pub excluded: i64,
    pub worlds: Vec<world::Ref>,
}

pub struct SceneView {
    pub camera: Camera,
    pub view_proj: Mat4,
    pub frustum: NativeRef<class::CFrustum>,
    pub attributes: attributes::Ref,
    pub viewport: RenderViewport,
    pub swap_chain: swap_chain::Handle,
    pub color: texture::Ref,
    pub depth: texture::Ref,
    pub render_tags: Vec<u32>,
    pub exclude_tags: Vec<u32>,
    pub post_process: bool,
    pub state: Mutex<ViewState>,
}

/// What a camera render decides about its view before the layers are added.
pub struct Desc {
    pub camera: Camera,
    pub proj: Option<Mat4>,
    pub viewport: RenderViewport,
    pub swap_chain: swap_chain::Handle,
    pub color: texture::Ref,
    pub depth: texture::Ref,
    pub worlds: Vec<world::Ref>,
    pub render_tags: Vec<u32>,
    pub exclude_tags: Vec<u32>,
    pub post_process: bool,
    pub unique_id: i32,
    pub managed_camera_id: i32,
}

pub fn create(d: Desc) -> Ref {
    let view_proj = d.proj.unwrap_or_else(|| d.camera.proj()) * d.camera.view();
    let frustum = objects::create(Mutex::new(d.camera));
    let state = ViewState { worlds: d.worlds, ..Default::default() };
    let view = SceneView {
        camera: d.camera,
        view_proj,
        frustum,
        attributes: attributes::create(),
        viewport: d.viewport,
        swap_chain: d.swap_chain,
        color: d.color,
        depth: d.depth,
        render_tags: d.render_tags,
        exclude_tags: d.exclude_tags,
        post_process: d.post_process,
        state: Mutex::new(state),
    };
    let r: Ref = objects::create(view);
    field!(set i32, v::GET_M_VIEW_UNIQUE_ID, r.addr(), d.unique_id);
    field!(set i32, v::GET_M_MANAGED_CAMERA_ID, r.addr(), d.managed_camera_id);
    r
}

pub fn get(r: Ref) -> Option<Arc<SceneView>> {
    objects::get::<_, SceneView>(r)
}

/// Free the view, its layers, frustum and attributes.
pub fn destroy(r: Ref) {
    if let Some(view) = get(r) {
        let layers = std::mem::take(&mut view.state.lock().unwrap().layers);
        layers.into_iter().for_each(layer::destroy);
        objects::destroy(view.frustum);
        objects::destroy(view.attributes);
    }
    objects::destroy(r);
}

pub fn with<R: Default>(r: Ref, f: impl FnOnce(&SceneView, &mut ViewState) -> R) -> R {
    get(r).map(|view| f(&view, &mut view.state.lock().unwrap())).unwrap_or_default()
}

/// Insert `l` before `before` (or at the end), returning it.
pub fn add_layer(r: Ref, l: layer::Ref, before: layer::Ref) -> layer::Ref {
    with(r, |_, s| match s.layers.iter().position(|x| *x == before).filter(|_| !before.is_null()) {
        Some(i) => s.layers.insert(i, l),
        None => s.layers.push(l),
    });
    l
}

fn new_layer(r: Ref, name: Option<&std::ffi::CStr>, viewport: RenderViewport, kind: layer::Kind, before: layer::Ref) -> layer::Ref {
    let name = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (required, excluded) = with(r, |_, s| (s.required, s.excluded));
    add_layer(r, layer::create(r, name, viewport, kind, required, excluded), before)
}

pub fn register(r: &mut Registry) {
    bind!(r, v::GET_MAIN_VIEWPORT, |r| get(r).map(|v| v.viewport).unwrap_or_default());
    bind!(r, v::GET_SWAP_CHAIN, |r| RawPtr::from_raw(get(r).map_or(0, |v| v.swap_chain.value) as *mut _));
    bind!(r, v::GET_RENDER_TARGET_ARRAY_INDEX, |_| -1);
    bind!(r, v::ADD_DEPENDENT_VIEW, |_, _| {});
    bind!(r, v::GET_RENDER_ATTRIBUTES_PTR, |r| get(r).map(|v| v.attributes).unwrap_or_default());
    bind!(r, v::ADD_RENDER_LAYER, |r, name, vp, mode, before| new_layer(r, name, vp, layer::Kind::Render(mode), before));
    bind!(r, v::ADD_CLEAR_LAYER, |r, name, vp, flags, before| new_layer(r, name, vp, layer::Kind::Clear(flags), before));
    bind!(r, v::ADD_LABEL_LAYER, |r, name| new_layer(r, name, RenderViewport::default(), layer::Kind::Label, layer::Ref::default()));
    bind!(r, v::ADD_MANAGED_PROCEDURAL_LAYER, |r, name, vp, callback, before, _delete| {
        new_layer(r, name, vp, layer::Kind::Procedural(callback.as_ptr() as usize), before)
    });
    bind!(r, v::ADD_BLIT_TEXTURE_TO_TARGET_LAYER, |r, name, src, src_vp, dst_vp, dst, _color_space| {
        let l = new_layer(r, name, dst_vp, layer::Kind::Blit { src: rt_texture(src), src_viewport: src_vp }, layer::Ref::default());
        layer::set_output(l, rt_texture(dst), texture::Ref::default());
        l
    });
    // Occlusion culling is a GPU feedback loop we don't run; managed sees it never requested.
    bind!(r, v::SET_OCCLUSION_CULL, |_, _, _, _, _, _, _, _, _| {});
    bind!(r, v::GET_OCCLUSION_CULL_REQUESTED, |_| 0);
    bind!(r, v::SET_DEFAULT_LAYER_OBJECT_REQUIRED_FLAGS, |r, f| with(r, |_, s| s.required = f));
    bind!(r, v::SET_DEFAULT_LAYER_OBJECT_EXCLUDED_FLAGS, |r, f| with(r, |_, s| s.excluded = f));
    bind!(r, v::GET_DEFAULT_LAYER_OBJECT_REQUIRED_FLAGS, |r| with(r, |_, s| s.required));
    bind!(r, v::GET_DEFAULT_LAYER_OBJECT_EXCLUDED_FLAGS, |r| with(r, |_, s| s.excluded));
    bind!(r, v::ADD_WORLD_TO_RENDER_LIST, |r, w| with(r, |_, s| s.worlds.push(w)));
    bind!(r, v::GET_CAMERA_POSITION, |r| get(r).map(|v| crate::math::from_v3(v.camera.origin)).unwrap_or_default());
    bind!(r, v::FIND_OR_CREATE_RENDER_TARGET, |_, _, t, _flags| rt_handle(t));
    bind!(r, v::SET_PARENT, |r, p| with(r, |_, s| s.parent = p));
    bind!(r, v::GET_PARENT, |r| with(r, |_, s| s.parent));
    bind!(r, v::GET_PRIORITY, |r| with(r, |_, s| s.priority));
    bind!(r, v::SET_PRIORITY, |r, p| with(r, |_, s| s.priority = p));
    bind!(r, v::SET_ISSUE_SORT_INDEX, |_, _| {});
    bind!(r, v::GET_FRUSTUM, |r| get(r).map(|v| v.frustum).unwrap_or_default());
    bind!(r, v::GET_POST_PROCESS_ENABLED, |r| get(r).is_some_and(|v| v.post_process));
    bind!(r, v::GET_TOOLS_VIS_MODE, |_| 0);
}
