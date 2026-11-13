//! `g_pSceneSystem`'s per-frame render slots. Views run eagerly inside each camera render
//! (camera.rs), so a frame's batch (`BeginRenderingViews` .. `FinishRenderingViews`) only tracks
//! that it is open and does the frame-end housekeeping; waiting is a queue flush.

use super::shadow_view;
use crate::render::{device, draw};
use abi::slots::g_p_scene_system as s;
use abi::{NativeRef, RawPtr, Registry, StrRet, bind};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

static RENDERING: AtomicBool = AtomicBool::new(false);
static RENDER_TIME: AtomicU32 = AtomicU32::new(0);
static PROFILER_MODE: AtomicU64 = AtomicU64::new(0);
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// The time managed set for this frame (`SetNextRenderTime`), in seconds.
pub fn render_time() -> f32 {
    f32::from_bits(RENDER_TIME.load(Ordering::Relaxed))
}

fn finish() {
    RENDERING.store(false, Ordering::Release);
    draw::gpu_mesh::prune();
    draw::morph::prune();
}

fn wait() -> bool {
    if let Some(gpu) = device::gpu() {
        gpu.wait_idle();
    }
    true
}

/// Opaque ids for culling boxes and fog volumes (render/fog_volume.rs).
fn next_id() -> u32 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn register(r: &mut Registry) {
    bind!(r, s::BEGIN_RENDERING_VIEWS, |_stats| {
        crate::render::submits::begin_frame();
        crate::render::skin::begin_frame();
        RENDERING.store(true, Ordering::Release);
    });
    bind!(r, s::FINISH_RENDERING_VIEWS, finish);
    bind!(r, s::WAIT_FOR_RENDERING_TO_COMPLETE, wait);
    bind!(r, s::IS_RENDERING_BUSY, || false);
    bind!(r, s::IS_RENDERING_VIEWS, || RENDERING.load(Ordering::Acquire));
    bind!(r, s::FRAME_UPDATE, || {});
    bind!(r, s::BEGIN_RENDERING_DYNAMIC_VIEW, |_| {});
    // A static-cache view hands its cached statics in; ours draws them again instead of copying.
    bind!(r, s::ADD_SHADOW_VIEW, |_, parent, frustum, viewport, target, slice, required, excluded, bias, slope, cached: NativeRef<_>| {
        let excluded = if cached.is_null() { excluded } else { 0 };
        shadow_view::render(&shadow_view::ShadowView { parent, frustum, viewport, target, slice: slice.max(0) as u32, required, excluded, bias: (bias, slope) });
        NativeRef::default()
    });
    bind!(r, s::ADD_SHADOW_VIEW_1, |_, parent, frustum, viewport, target, slice, required, excluded, bias, slope| {
        shadow_view::render(&shadow_view::ShadowView { parent, frustum, viewport, target, slice: slice.max(0) as u32, required, excluded, bias: (bias, slope) });
        NativeRef::default()
    });
    bind!(r, s::GET_WELL_KNOWN_TEXTURE, super::well_known::texture);
    bind!(r, s::GET_WELL_KNOWN_MATERIAL_HANDLE, |_| NativeRef::default());
    bind!(r, s::SETUP_PER_OBJECT_LIGHTING, |_, _, _| {});
    // TODO: apply culling boxes (SceneCullingBox hides objects inside or outside them).
    bind!(r, s::ADD_CULLING_BOX, |_, _, _, _, _| next_id());
    bind!(r, s::REMOVE_CULLING_BOX, |_, _| {});
    bind!(r, s::ADD_VOLUMETRIC_FOG_VOLUME, |w, v| {
        let id = next_id();
        crate::render::fog_volume::add(w.addr(), id, v);
        id
    });
    bind!(r, s::REMOVE_VOLUMETRIC_FOG_VOLUME, |w, id| crate::render::fog_volume::remove(w.addr(), id));
    bind!(r, s::DOWNSAMPLE_TEXTURE, |_, _, _| {});
    bind!(r, s::BIND_TRANSFORM_SLOT, |_, _, _| {});
    bind!(r, s::SET_PIPELINE_TEXTURE_INDEX, |_, _| {});
    bind!(r, s::UPLOAD_PIPELINE_TEXTURE_INDICES, |_| {});
    // Ray tracing isn't supported (IsRayTracingSupported is false), so managed makes none.
    bind!(r, s::CREATE_RAY_TRACE_WORLD, |_, _| NativeRef::default());
    bind!(r, s::DESTROY_RAY_TRACE_WORLD, |_| {});
    bind!(r, s::SET_GPU_PROFILER_MODE, |m| PROFILER_MODE.store(m as u64, Ordering::Relaxed));
    bind!(r, s::GET_GPU_PROFILER_MODE, || PROFILER_MODE.load(Ordering::Relaxed) as i64);
    // No GPU timestamps are collected: every snapshot is empty.
    bind!(r, s::REFRESH_GPU_TIMESTAMP_SNAPSHOT, || {});
    bind!(r, s::GET_GPU_TIMESTAMP_COUNT, || 0);
    bind!(r, s::GET_GPU_TIMESTAMP_RENDERER, || 0);
    bind!(r, s::GET_GPU_TIMESTAMP_FRAME_ID, || 0);
    bind!(r, s::GET_GPU_TIMESTAMP_DURATION, |_| 0.0);
    bind!(r, s::GET_GPU_TIMESTAMP_NAME, |_| StrRet::from(None::<String>));
    bind!(r, s::GET_GPU_TIMESTAMP_PARENT, |_| -1);
    bind!(r, s::GET_GPU_TIMESTAMP_STABLE_ID, |_| 0);
    bind!(r, s::GET_GPU_TIMESTAMP_MEASURED, |_| false);
    bind!(r, s::GET_GPU_TIMESTAMP_UNPARENTED, |_| false);
    bind!(r, s::GET_GPU_TIMESTAMP_EVENT_COUNT, || 0);
    bind!(r, s::GET_GPU_TIMESTAMP_EVENT, |_, _| StrRet::from(None::<String>));
    bind!(r, s::BEGIN_MANAGED_PERF_MARKER, |_, _, _, _| RawPtr::NULL);
    bind!(r, s::END_MANAGED_PERF_MARKER, |_, _| {});
    bind!(r, s::SET_NEXT_RENDER_TIME, |t: f32| RENDER_TIME.store(t.to_bits(), Ordering::Relaxed));
    bind!(r, s::GET_CURRENT_RENDER_TIME, render_time);
}
