//! Render and scene statistics managed polls for its perf overlay. The empty device draws
//! nothing, so the counts are genuinely zero.

use abi::slots::{g_p_render_device as device, g_p_scene_system as scene};
use abi::{NativeRef, Registry, StrRet, bind, class};
use std::sync::OnceLock;

/// `SceneSystemPerFrameStats_t`'s fields are read through their own get/set slots; unbound ones
/// answer 0. TODO: back them with real counts once the scene system draws.
struct PerFrameStats;

fn per_frame_stats() -> NativeRef<class::SceneSystemPerFrameStats_t> {
    static STATS: OnceLock<usize> = OnceLock::new();
    let addr = *STATS.get_or_init(|| objects::create::<class::SceneSystemPerFrameStats_t, _>(PerFrameStats).addr());
    NativeRef::from_raw(addr as *mut _)
}

pub fn register(r: &mut Registry) {
    bind!(r, scene::GET_PER_FRAME_STATS, per_frame_stats);
    bind!(r, scene::GET_NUM_UNBATCHABLE_MATERIALS, || 0);
    bind!(r, device::GET_GPU_FRAME_TIME_MS, |_swap_chain, ms, frame| {
        (*ms, *frame) = (0.0, 0);
        false
    });
    bind!(r, device::GET_GPU_STATS_SUMMARY, || StrRet::from(c""));
}
