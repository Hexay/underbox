//! Native's GPU occlusion cull (`CSceneViewOcclusionCull`). Managed hands the view its buffers and
//! dispatches occlusion_cull_cs between layers (NativeOcclusionCull.cs). Before the layers run we
//! record what each culled layer draws: a run per draw with that layer's single phase, a cull
//! instance per drawn instance, and arguments with no instances. Those draws then go indirect,
//! from the arguments and surviving ids the cull writes. A draw past the buffers' capacity, or one
//! whose instance ids aren't its instance indices, stays direct.

use super::collect::Collected;
use super::layer;
use super::view::SceneView;
use crate::render::buffers::{self, Handle};
use crate::render::device;
use crate::render::draw::{Draw, Indirect, vertex_cached};
use glam::Vec3;
use std::collections::HashMap;
use std::sync::Arc;

/// `SceneObjectFlags.NoOcclusionCulling`.
pub const NO_OCCLUSION_CULLING: i64 = 0x400_0000_0000;

/// The shader's `PHASE_LATE` and `PHASE_FINAL` (the forward pass).
const LATE: u32 = 2;
const FINAL: u32 = 4;

/// What managed passed `SetOcclusionCull`.
#[derive(Clone, Copy)]
pub struct Buffers {
    pub instances: Handle,
    pub runs: Handle,
    pub args: Handle,
    pub ids: Handle,
    pub stats: Handle,
    pub dispatch: Handle,
    pub capacity: u32,
}

/// `LayerFlags` OcclusionCulledEarly, OcclusionCulledLate and OcclusionCulled as the shader's
/// phase bits.
fn phase(layer_flags: i64) -> Option<u32> {
    match layer_flags {
        f if f & 0x400_0000_0000 != 0 => Some(1),
        f if f & 0x800_0000_0000 != 0 => Some(LATE),
        f if f & 0x200_0000_0000 != 0 => Some(FINAL),
        _ => None,
    }
}

/// The culled layers' draws, collected and recorded for the cull before any layer runs, by layer
/// address; empty when managed set no cull on the view.
pub fn prepare(view: &SceneView, layers: &[layer::Ref]) -> HashMap<usize, Collected> {
    let Some(b) = view.state.lock().unwrap().occlusion else { return HashMap::new() };
    let (Some(args), Some(ids)) = (buffers::get(b.args), buffers::get(b.ids)) else { return HashMap::new() };
    let mut record = Record::default();
    let mut out = HashMap::new();
    for &l in layers {
        let Some(phase) = phase(layer::layer_flags(l)) else { continue };
        let Some(mut collected) = super::execute::collect_layer(view, l) else { continue };
        collected.0.iter_mut().for_each(|d| record.add(d, phase, (&args, &ids), b.capacity));
        // The late prepass only redraws what the early cull missed: the early layers drew the rest.
        if phase == LATE {
            collected.0.retain(|d| d.cull.indirect.is_some());
        }
        out.insert(l.addr(), collected);
    }
    record.upload(&b);
    view.state.lock().unwrap().occlusion_requested = i32::try_from(record.requested).unwrap_or(i32::MAX);
    out
}

/// The cull's inputs as the shader reads them, in 32-bit words.
#[derive(Default)]
struct Record {
    /// `CullInstance`: center, run, extents (x below zero never culls), id.
    instances: Vec<u32>,
    /// `CullRun`: first instance, first argument, draw count, phases.
    runs: Vec<u32>,
    /// `DrawIndexedIndirectArgs`.
    args: Vec<u32>,
    count: u32,
    final_count: u32,
    requested: u32,
}

impl Record {
    fn add(&mut self, d: &mut Draw, phase: u32, (args, ids): (&Arc<render::Buffer>, &Arc<render::Buffer>), capacity: u32) {
        let indexed_ids = d.shaded.as_ref().is_none_or(|s| s.context.instance_ids.is_none());
        if d.indices.is_none() || d.instances == 0 || vertex_cached(d) || !indexed_ids {
            return;
        }
        self.requested += d.instances;
        let (first, run, first_arg) = (self.count, self.runs.len() as u32 / 4, self.args.len() as u32 / 5);
        if first + d.instances > capacity || run >= capacity || first_arg >= capacity {
            return;
        }
        let bounds = (d.cull.bounds.len() == d.instances as usize).then_some(&d.cull.bounds);
        for i in 0..d.instances {
            let (center, extents) = bounds.map_or((Vec3::ZERO, Vec3::NEG_X), |b| {
                let [lo, hi] = b[i as usize];
                ((lo + hi) * 0.5, (hi - lo) * 0.5)
            });
            let [cx, cy, cz, ex, ey, ez] = [center.x, center.y, center.z, extents.x, extents.y, extents.z].map(f32::to_bits);
            self.instances.extend([cx, cy, cz, run, ex, ey, ez, i]);
        }
        self.count += d.instances;
        if phase == FINAL {
            self.final_count += d.instances;
        }
        self.runs.extend([first, first_arg, 1, phase]);
        self.args.extend([d.range.len() as u32, 0, d.range.start, d.base_vertex as u32, 0]);
        d.cull.indirect = Some(Indirect { args: args.clone(), args_offset: u64::from(first_arg) * 20, ids: ids.clone(), ids_offset: u64::from(first) * 4 });
    }

    /// Writes the cull's inputs, its instance count where `D_GPU_COUNT` reads it, and its dispatch.
    fn upload(&self, b: &Buffers) {
        let Some(gpu) = device::gpu() else { return };
        let write = |h: Handle, words: &[u32]| {
            let Some(buffer) = buffers::get(h).filter(|_| !words.is_empty()) else { return };
            let n = words.len().min(buffer.size() as usize / 4);
            let bytes: Vec<u8> = words[..n].iter().flat_map(|w| w.to_le_bytes()).collect();
            buffer.write(gpu, 0, &bytes);
        };
        write(b.instances, &self.instances);
        write(b.runs, &self.runs);
        write(b.args, &self.args);
        // CSceneViewOcclusionCull::Stat_t: visible, instances, final-phase instances, then counters.
        let mut stats = [0; 9];
        (stats[1], stats[2]) = (self.count, self.final_count);
        write(b.stats, &stats);
        write(b.dispatch, &[self.count.div_ceil(64), 1, 1]);
    }
}
