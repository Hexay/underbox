//! PVS culling for managed's scene renderer, which never consults the PVS: before its occlusion
//! cull runs, the cullable `CullInstance`s whose box the camera's cluster can't see are dropped
//! from a compacted copy of `OcclusionInstances`, and the instance count shrinks to match. Every
//! culled draw (early, late and final phases) then skips them. The camera render in progress
//! names the eye and PVS (`scene::pvs::enter`).

use crate::render::scene::pvs;
use crate::render::{attributes, buffers, device};
use glam::Vec3;
use std::sync::Arc;

/// Words per `CullInstance`: centre, run, extents (x < 0: never culled), id.
const WORDS: usize = 8;
/// `OcclusionStats[STAT_INSTANCES]`, the count `D_GPU_COUNT` dispatches read.
const STAT_INSTANCES: u64 = 1;

/// The instances `eye` keeps: never-culled ones, and boxes `sees` lets through.
pub(super) fn keep(instances: &[u32], count: usize, sees: impl Fn(Vec3, Vec3) -> bool) -> Vec<u32> {
    let f = |w: u32| f32::from_bits(w);
    instances
        .chunks_exact(WORDS)
        .take(count)
        .filter(|i| {
            let (center, extents) = (Vec3::new(f(i[0]), f(i[1]), f(i[2])), Vec3::new(f(i[4]), f(i[5]), f(i[6])));
            extents.x < 0.0 || sees(center - extents, center + extents)
        })
        .flatten()
        .copied()
        .collect()
}

/// The attributes to dispatch the occlusion cull with: `attrs` itself without a PVS, else a
/// child of it with the filtered instances (free it with [`release`]).
pub(super) fn filter(attrs: attributes::Ref) -> attributes::Ref {
    let (Some((eye, pvs)), Some(gpu)) = (pvs::current(), device::gpu()) else { return attrs };
    let Some(instances) = attributes::buffer(attrs, "OcclusionInstances").and_then(|h| buffers::get(buffers::Handle::new(h))) else { return attrs };
    let stats = attributes::buffer(attrs, "OcclusionStats").and_then(|h| buffers::get(buffers::Handle::new(h)));
    let gpu_count = attributes::combo(attrs, "D_GPU_COUNT").is_some_and(|c| c != 0);
    let count = match (gpu_count, &stats) {
        (true, Some(s)) => super::words(&s.read(gpu, STAT_INSTANCES * 4, 4)).first().copied().unwrap_or(0) as usize,
        _ => attributes::shader_value(attrs, "OcclusionInstanceCount").map_or(0, |v| v.x.max(0.0) as usize),
    };
    let words = super::words(&instances.read(gpu, 0, instances.size()));
    let kept = keep(&words, count, |lo, hi| pvs::box_visible(eye, pvs, (lo, hi)));
    let n = kept.len() / WORDS;
    if n == count.min(words.len() / WORDS) {
        return attrs;
    }
    let bytes: Vec<u8> = kept.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mut padded = bytes;
    padded.resize(padded.len().max(WORDS * 4), 0);
    let handle = buffers::insert(Arc::new(render::Buffer::with_data(gpu, "pvs instances", render::BufferKind::Storage, &padded)));
    let child = attributes::create();
    attributes::set_parent(child, attrs);
    attributes::set_buffer(child, abi::StringToken::of("OcclusionInstances"), handle.value);
    attributes::set(child, "OcclusionInstanceCount", attributes::Value::Int(n as i32));
    if let (true, Some(s)) = (gpu_count, &stats) {
        s.write(gpu, STAT_INSTANCES * 4, &(n as u32).to_le_bytes());
    }
    child
}

/// Frees what [`filter`] made for this dispatch.
pub(super) fn release(made: attributes::Ref, attrs: attributes::Ref) {
    if made == attrs {
        return;
    }
    if let Some(h) = attributes::buffer(made, "OcclusionInstances") {
        buffers::destroy(buffers::Handle::new(h));
    }
    objects::destroy(made);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(center: [f32; 3], never: bool, id: u32) -> [u32; 8] {
        let e = if never { -1.0f32 } else { 1.0 };
        [center[0].to_bits(), center[1].to_bits(), center[2].to_bits(), 0, e.to_bits(), 1f32.to_bits(), 1f32.to_bits(), id]
    }

    /// Only boxes at x < 100 are in a visible cluster.
    #[test]
    fn hidden_cullable_instances_drop_out() {
        let all: Vec<u32> = [instance([0.0; 3], false, 1), instance([500.0, 0.0, 0.0], false, 2), instance([500.0, 0.0, 0.0], true, 3), instance([0.0; 3], false, 4)].concat();
        let kept = keep(&all, 4, |lo, _| lo.x < 100.0);
        let ids: Vec<u32> = kept.chunks(8).map(|i| i[7]).collect();
        assert_eq!(ids, [1, 3, 4], "the hidden cullable one goes; never-culled ones stay");
        assert_eq!(keep(&all, 2, |_, _| true).len(), 16, "only the first `count` are live");
    }

    /// `world::fixture::visibility`'s clusters as scene/tests.rs uses them: from the origin
    /// (cluster 1) a box around x = 308 (cluster 2) is hidden.
    #[test]
    fn occlusion_inputs_in_a_pvs_view_lose_hidden_instances() {
        let Some(gpu) = device::create() else { return };
        let vis = ::world::vis::Visibility { rows: vec![0b011, 0b010, 0b110, 0b100, 0b110], ..::world::fixture::visibility() };
        let map: crate::world::pvs::Ref = objects::create(crate::world::pvs::Pvs(Some(vis)));
        let world: crate::scene::world::Ref = objects::create(crate::scene::world::SceneWorld::default());
        *crate::scene::world::get(world).unwrap().pvs.lock().unwrap() = map.addr();
        let bytes: Vec<u8> = [instance([0.0; 3], false, 1), instance([308.0, 0.0, 0.0], false, 2)].concat().iter().flat_map(|w| w.to_le_bytes()).collect();
        let h = buffers::insert(Arc::new(render::Buffer::with_data(gpu, "instances", render::BufferKind::Storage, &bytes)));
        let attrs = attributes::create();
        attributes::set_buffer(attrs, abi::StringToken::of("OcclusionInstances"), h.value);
        attributes::set(attrs, "OcclusionInstanceCount", attributes::Value::Int(2));
        assert!(filter(attrs) == attrs, "no camera render, no PVS");
        {
            let _view = pvs::enter(Vec3::ZERO, &[world]);
            let made = filter(attrs);
            let kept = buffers::get(buffers::Handle::new(attributes::buffer(made, "OcclusionInstances").unwrap())).unwrap();
            assert_eq!(super::super::words(&kept.read(gpu, 0, 32))[7], 1, "the visible instance first");
            assert_eq!(attributes::shader_value(made, "OcclusionInstanceCount").unwrap().x, 1.0);
            assert!(attributes::buffer(made, "OcclusionRuns").is_none() && attributes::buffer(made, "OcclusionInstances") != Some(h.value));
            release(made, attrs);
        }
        assert!(pvs::current().is_none(), "the scope ended with the render");
        buffers::destroy(h);
        objects::destroy(attrs);
        objects::destroy(world);
        objects::destroy(map);
    }
}
