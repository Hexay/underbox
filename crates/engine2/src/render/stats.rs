//! Render and scene statistics managed polls for its perf overlay. `GetPerFrameStats` holds the
//! last presented frame's views, draw calls and triangles; its other fields stay 0.

use abi::rt::fields;
use abi::slots::{g_p_render_device as device, g_p_scene_system as scene, scene_system_per_frame_stats_t as f};
use abi::{NativeRef, Registry, StrRet, bind, class};
use render::wgpu::PrimitiveTopology;
use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

struct PerFrameStats;

/// Views, draw calls and triangles since the last present.
static COUNTING: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];

fn per_frame_stats() -> NativeRef<class::SceneSystemPerFrameStats_t> {
    static STATS: OnceLock<usize> = OnceLock::new();
    let addr = *STATS.get_or_init(|| objects::create::<class::SceneSystemPerFrameStats_t, _>(PerFrameStats).addr());
    NativeRef::from_raw(addr as *mut _)
}

pub fn view_rendered() {
    COUNTING[0].fetch_add(1, Ordering::Relaxed);
}

/// Count a pass's draws. Indirect (GPU-culled) draws count their whole range.
pub fn drew(draws: &[super::draw::Draw]) {
    let tris: u32 = draws.iter().map(|d| triangles(d.state.topology, d.range.len() as u32) * d.instances).sum();
    COUNTING[1].fetch_add(draws.len() as u32, Ordering::Relaxed);
    COUNTING[2].fetch_add(tris, Ordering::Relaxed);
}

fn triangles(topology: PrimitiveTopology, vertices: u32) -> u32 {
    match topology {
        PrimitiveTopology::TriangleList => vertices / 3,
        PrimitiveTopology::TriangleStrip => vertices.saturating_sub(2),
        _ => 0,
    }
}

/// Publish the counts since the last call as the frame's stats, and start the next frame's.
pub fn end_frame() {
    let counts = COUNTING.each_ref().map(|c| c.swap(0, Ordering::Relaxed));
    publish(per_frame_stats().as_raw().cast(), counts);
}

fn publish(stats: *mut c_void, [views, draws, tris]: [u32; 3]) {
    fields::set::<i32, { f::GET_M_N_NUM_VIEWS_RENDERED.index }>(stats, views as i32);
    fields::set::<u32, { f::GET_M_N_DRAW_CALLS.index }>(stats, draws);
    fields::set::<u32, { f::GET_M_N_TRIANGLES_RENDERED.index }>(stats, tris);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangles_by_topology() {
        assert_eq!(triangles(PrimitiveTopology::TriangleList, 36), 12);
        assert_eq!(triangles(PrimitiveTopology::TriangleStrip, 4), 2);
        assert_eq!(triangles(PrimitiveTopology::LineList, 36), 0);
    }

    #[test]
    fn published_counts_read_through_the_field_slots() {
        let mut object = 0u64;
        let p = (&raw mut object).cast::<c_void>();
        publish(p, [2, 5, 120]);
        assert_eq!(fields::get::<i32, { f::GET_M_N_NUM_VIEWS_RENDERED.index }>(p), 2);
        assert_eq!(fields::get::<u32, { f::GET_M_N_DRAW_CALLS.index }>(p), 5);
        assert_eq!(fields::get::<u32, { f::GET_M_N_TRIANGLES_RENDERED.index }>(p), 120);
    }
}
