//! `CRenderPipelineSetup`, what native hands managed's `RenderPipeline.AddLayersToView`: the view,
//! its targets and what to render, all as generated fields, plus a few helper methods. And
//! `RenderPipelineTools`, the target queries and scratch targets managed's pipeline uses.

use super::{field, view};
use crate::render::{attributes, texture};
use abi::slots::{c_render_pipeline_setup as s, render_pipeline_tools as tools};
use abi::structs::{RenderViewport, Vector4};
use abi::{NativeRef, Registry, StrRet, StringToken, bind, class};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub type Ref = NativeRef<class::CRenderPipelineSetup>;

/// `ImageFormat` RGBA16161616F, our HDR target format.
const HDR_FORMAT: i64 = 24;
const BACK_BUFFER_FORMAT: i64 = 0;
/// `ClearFlags.Color | Depth | Stencil`.
pub const CLEAR_ALL: i32 = 0xFF | 0x100 | 0x200;

pub struct Desc {
    pub attributes: attributes::Ref,
    pub viewport: RenderViewport,
    pub clear_flags: i32,
    pub clear_color: Vector4,
    pub tools_view: bool,
}

/// A setup for `view`, its fields filled the way native fills them before calling managed.
pub fn create(v: view::Ref, d: &Desc) -> Ref {
    let (color, depth) = view::get(v).map(|x| (x.color, x.depth)).unwrap_or_default();
    let (w, h) = (d.viewport.m_n_width, d.viewport.m_n_height);
    let r: Ref = objects::create(());
    let a = r.addr();
    field!(set NativeRef<class::ISceneView>, s::GET_M_P_VIEW, a, v);
    field!(set NativeRef<class::CRenderAttributes>, s::GET_M_P_ATTRIBUTES, a, d.attributes);
    field!(set RenderViewport, s::GET_M_VIEWPORT, a, d.viewport);
    field!(set RenderViewport, s::GET_M_SKY_VIEWPORT, a, d.viewport);
    field!(set RenderViewport, s::GET_M_SCREEN_VIEWPORT, a, d.viewport);
    field!(set RenderViewport, s::GET_M_UPSCALE_VIEWPORT, a, d.viewport);
    let (color, depth) = (view::rt_handle(color), view::rt_handle(depth));
    field!(set view::RtHandle, s::GET_M_H_COLOR, a, color);
    field!(set view::RtHandle, s::GET_M_H_DEPTH, a, depth);
    field!(set view::RtHandle, s::GET_M_H_FINAL_COLOR, a, color);
    field!(set view::RtHandle, s::GET_M_H_FINAL_DEPTH, a, depth);
    field!(set i32, s::GET_M_N_WIDTH, a, w);
    field!(set i32, s::GET_M_N_HEIGHT, a, h);
    field!(set i32, s::GET_M_N_UPSCALE_RENDER_WIDTH, a, w);
    field!(set i32, s::GET_M_N_UPSCALE_RENDER_HEIGHT, a, h);
    field!(set i64, s::GET_M_N_DEPTH_FORMAT, a, 62);
    field!(set i32, s::GET_M_N_CLEAR_FLAGS, a, d.clear_flags);
    field!(set Vector4, s::GET_M_V_CLEAR_COLOR, a, d.clear_color);
    field!(set bool, s::GET_M_B_TOOLS_VIEW, a, d.tools_view);
    field!(set bool, s::GET_M_B_RENDER_SKY, a, true);
    field!(set bool, s::GET_M_B_RENDER_OPAQUE, a, true);
    field!(set bool, s::GET_M_B_RENDER_DYNAMIC_OBJECTS, a, true);
    field!(set bool, s::GET_M_B_RENDER_DECALS, a, true);
    field!(set bool, s::GET_M_B_RENDER_TRANSLUCENT, a, true);
    field!(set bool, s::GET_M_B_MANAGED_HOOKS, a, true);
    field!(set bool, s::GET_M_B_DRAW_OVERLAYS, a, true);
    r
}

pub fn view_of(r: Ref) -> view::Ref {
    field!(get NativeRef<class::ISceneView>, s::GET_M_P_VIEW, r.addr())
}

/// Whether the view draws objects tagged `tag`: render tags (when any) must include it, exclude
/// tags must not.
fn renders_tag(r: Ref, tag: StringToken) -> bool {
    view::get(view_of(r)).is_some_and(|v| (v.render_tags.is_empty() || v.render_tags.contains(&tag.0)) && !v.exclude_tags.contains(&tag.0))
}

type ScratchKey = (String, i32, i32, i64);
static SCRATCH: LazyLock<Mutex<HashMap<ScratchKey, texture::Ref>>> = LazyLock::new(Default::default);

/// A pooled target by name and shape, kept across frames as native's scratch pool does.
fn scratch_target(name: Option<&std::ffi::CStr>, w: i32, h: i32, format: i64) -> view::RtHandle {
    let name = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let key = (name.clone(), w.max(1), h.max(1), format);
    let t = *SCRATCH.lock().unwrap().entry(key).or_insert_with(|| texture::create_target(&name, w.max(1) as u32, h.max(1) as u32, format as i32));
    view::rt_handle(t)
}

fn target_info<R: Default>(h: view::RtHandle, f: impl FnOnce(&texture::Texture) -> R) -> R {
    texture::resolve(view::rt_texture(h)).map(|t| f(&t)).unwrap_or_default()
}

/// A layer measuring `color` for the camera's tonemap system, when it has one (auto exposure on).
fn add_auto_exposure(r: Ref, color: view::RtHandle, viewport: RenderViewport) {
    let attributes = field!(get NativeRef<class::CRenderAttributes>, s::GET_M_P_ATTRIBUTES, r.addr());
    let Some(tonemap) = attributes::pointer(attributes, StringToken::of("ITonemapSystem")) else { return };
    let v = view_of(r);
    let kind = super::layer::Kind::Exposure { tonemap, src: view::rt_texture(color) };
    view::add_layer(v, super::layer::create(v, "AutoExposure".into(), viewport, kind, 0, 0), Default::default());
}

pub fn register(r: &mut Registry) {
    bind!(r, s::ADD_VOLUMETRIC_FOG, |_| {});
    // False (no skybox world): managed then draws the 2D sky itself.
    bind!(r, s::ADD3_D_SKYBOX, super::skybox::add_for_setup);
    bind!(r, s::ADD_TEMPORAL_UPSCALE, |_| {});
    bind!(r, s::ADD_AUTO_EXPOSURE, add_auto_exposure);
    // Layers already run in the order managed adds them.
    bind!(r, s::SORT_BY_RENDER_ORDER, |_, _| {});
    bind!(r, s::RENDERS_TAG, renders_tag);
    bind!(r, s::GET_SHADING_MODE, |_| StringToken::of("Forward").0);

    bind!(r, tools::GET_TARGET_FORMAT, |_, h| target_info(h, |t| i64::from(t.image_format())));
    bind!(r, tools::GET_TARGET_WIDTH, |_, h| target_info(h, |t| t.gpu.as_ref().map_or(0, |g| g.desc.width as i32)));
    bind!(r, tools::GET_TARGET_HEIGHT, |_, h| target_info(h, |t| t.gpu.as_ref().map_or(0, |g| g.desc.height as i32)));
    bind!(r, tools::TARGET_SUPPORTS_SRGB_WRITE, |_, _| false);
    bind!(r, tools::SCREEN_TEXT, |_, _, _, _, _, _, _, _| {});
    bind!(r, tools::COLLECT_LOADED_SHADER_NAMES, || 0);
    bind!(r, tools::GET_LOADED_SHADER_NAME, |_| StrRet::from(None::<String>));
    bind!(r, tools::GET_SHADER_ID_COLOR, |_| crate::math::vec4(1.0, 1.0, 1.0, 1.0));
    bind!(r, tools::ADD_TEXTURE_DEBUG_LAYER, |_, _, _, _, _| {});
    bind!(r, tools::FIND_OR_CREATE_SCRATCH_TARGET, |_, name, w, h, format, _msaa, _uav| scratch_target(name, w, h, format));
    bind!(r, tools::IS_FLOAT_FORMAT, |f| matches!(f, 24 | 27 | 29 | 53 | 59 | 96));
    bind!(r, tools::GET_HDR_FORMAT, || HDR_FORMAT);
    bind!(r, tools::GET_BACK_BUFFER_FORMAT, || BACK_BUFFER_FORMAT);
}
