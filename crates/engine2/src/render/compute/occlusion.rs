//! `occlusion_cull_cs.shader` (`Sandbox.SceneRenderer`'s `OcclusionCullLayer`) on the CPU: it
//! fills the indirect args and culled instance ids every culled draw reads, so without it the
//! managed renderer's opaque pass draws nothing. Emits exactly what the shader's `MainCs` emits
//! for boxes the depth test can't resolve (`Coverage.Tested == 0`, never hidden): nothing is
//! occluded, so each instance draws in the early and final phases.
//!
//! Gotcha: the GPU shader appends with atomics, so its slot order varies run to run; this one
//! appends in instance order.

use super::Inputs;
use crate::render::device;

pub const SHADER: &str = "occlusion_cull_cs.shader";

const PHASE_EARLY: u32 = 1;
const PHASE_LATE: u32 = 2;
const PHASE_FINAL: u32 = 4;
const RUN_FRAGMENTS: u32 = 8;
const RUN_COMPACT: u32 = 16;
const STAT_VISIBLE: usize = 0;
const STAT_INSTANCES: usize = 1;
const STAT_FRAGMENTS: usize = 3;
const STAT_TRIANGLES: usize = 5;
const STAT_TRIANGLES_UNTESTED: usize = 8;
/// `IndirectDrawIndexedArguments`: index count, instance count, first index, base vertex, first instance.
const ARG_WORDS: usize = 5;

/// The cull's buffers as u32 words, and its constants.
pub(super) struct Cull {
    pub instances: Vec<u32>,
    pub runs: Vec<u32>,
    pub args: Vec<u32>,
    pub ids: Vec<u32>,
    pub stats: Vec<u32>,
    pub count: usize,
    /// 0: the early phase, before the prepass; 1: late and final.
    pub pass: u32,
    /// Args (not words) from one phase's args to the next's, and ids.
    pub stride: (usize, usize),
    pub reject_early: bool,
    pub has_previous: bool,
}

#[derive(Clone, Copy)]
struct Run {
    first_instance: usize,
    first_arg: usize,
    draws: usize,
    phases: u32,
}

impl Cull {
    fn run(&self, i: usize) -> Option<Run> {
        let w = self.runs.get(i * 4..i * 4 + 4)?;
        Some(Run { first_instance: w[0] as usize, first_arg: w[1] as usize, draws: w[2] as usize, phases: w[3] })
    }

    fn instance_count(&mut self, arg: usize) -> Option<&mut u32> {
        self.args.get_mut(arg * ARG_WORDS + 1)
    }

    fn emit_instance(&mut self, run: Run, phase: usize, id: u32) {
        let arg = run.first_arg + phase * self.stride.0;
        let Some(slot) = self.instance_count(arg).map(|n| std::mem::replace(n, *n + 1)) else { return };
        for d in 1..run.draws {
            if let Some(n) = self.instance_count(arg + d) {
                *n += 1;
            }
        }
        if let Some(out) = self.ids.get_mut(run.first_instance + phase * self.stride.1 + slot as usize) {
            *out = id;
        }
    }

    fn fragment_arg(run: Run, index: usize) -> usize {
        run.first_arg + index + if run.phases & RUN_COMPACT != 0 { run.draws } else { 0 }
    }

    fn emit_fragment(&mut self, run: Run, phase: usize, index: usize) {
        let base = phase * self.stride.0;
        if run.phases & RUN_COMPACT == 0 {
            if let Some(n) = self.instance_count(base + run.first_arg + index) {
                *n = 1;
            }
            return;
        }
        let Some(survivors) = self.args.get_mut((base + run.first_arg + 2 * run.draws) * ARG_WORDS) else { return };
        let slot = std::mem::replace(survivors, *survivors + 1) as usize;
        let from = (base + Self::fragment_arg(run, index)) * ARG_WORDS;
        let Some(mut args) = self.args.get(from..from + ARG_WORDS).map(<[u32]>::to_vec) else { return };
        args[1] = 1;
        let to = (base + run.first_arg + slot) * ARG_WORDS;
        if let Some(dst) = self.args.get_mut(to..to + ARG_WORDS) {
            dst.copy_from_slice(&args);
        }
    }

    fn emit(&mut self, run: Run, phase: usize, id: u32) {
        if run.phases & RUN_FRAGMENTS != 0 {
            self.emit_fragment(run, phase, id as usize);
        } else {
            self.emit_instance(run, phase, id);
        }
    }

    fn stat(&mut self, i: usize, add: u32) {
        if let Some(s) = self.stats.get_mut(i) {
            *s += add;
        }
    }

    /// `MainCs` for every instance, with every depth test untested.
    pub fn execute(&mut self) {
        for i in 0..self.count {
            let Some(inst) = self.instances.get(i * 8..i * 8 + 8).map(<[u32]>::to_vec) else { break };
            let Some(run) = self.run(inst[3] as usize) else { continue };
            let phases = run.phases & if self.pass == 0 { PHASE_EARLY } else { PHASE_LATE | PHASE_FINAL };
            if run.draws == 0 || phases == 0 {
                continue;
            }
            let (fragment, cullable, id) = (run.phases & RUN_FRAGMENTS != 0, f32::from_bits(inst[4]) >= 0.0, inst[7]);
            // The previous chain's test is untested too: never occluded.
            let early = !cullable || !self.has_previous || phases & (PHASE_EARLY | PHASE_LATE) == 0 || !self.reject_early;
            if self.pass == 0 {
                if early {
                    self.emit(run, 0, id);
                }
                continue;
            }
            let (missed_early, final_phase) = (!early && phases & PHASE_LATE != 0, phases & PHASE_FINAL != 0);
            if fragment && final_phase {
                let triangles = self.args.get(Self::fragment_arg(run, id as usize) * ARG_WORDS).map_or(0, |n| n / 3);
                self.stat(STAT_FRAGMENTS, 1);
                self.stat(STAT_TRIANGLES, triangles);
                self.stat(STAT_TRIANGLES_UNTESTED, triangles);
            }
            if missed_early {
                self.emit(run, 1, id);
            }
            if final_phase {
                self.emit(run, 2, id);
                if !fragment {
                    self.stat(STAT_VISIBLE, 1);
                }
            }
        }
    }
}

pub(crate) fn words(bytes: &[u8]) -> Vec<u32> {
    bytes.as_chunks::<4>().0.iter().map(|w| u32::from_le_bytes(*w)).collect()
}

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Read the cull's buffers back, run it, write args, ids and stats back.
pub(super) fn cull(inputs: &Inputs) {
    let Some(gpu) = device::gpu() else { return };
    let names = ["OcclusionInstances", "OcclusionRuns", "OcclusionArgs", "OcclusionCulledIds", "OcclusionStats"];
    let Some([instances, runs, args, ids, stats]) = names.map(|n| inputs.buffer(n)).into_iter().collect::<Option<Vec<_>>>().and_then(|v| <[_; 5]>::try_from(v).ok()) else { return };
    let read = |b: &render::Buffer| words(&b.read(gpu, 0, b.size()));
    let stats_words = read(&stats);
    let stride = inputs.vector("OcclusionPhaseStride").unwrap_or_default();
    let count = inputs.vector("OcclusionInstanceCount").map_or(stats_words.get(STAT_INSTANCES).copied().unwrap_or(0) as usize, |v| v.x as usize);
    let mut c = Cull {
        instances: read(&instances),
        runs: read(&runs),
        args: read(&args),
        ids: read(&ids),
        stats: stats_words,
        count,
        pass: inputs.vector("OcclusionPass").map_or(0, |v| v.x as u32),
        stride: (stride.x.max(0.0) as usize, stride.y.max(0.0) as usize),
        reject_early: inputs.vector("OcclusionRejectEarly").is_some_and(|v| v.x != 0.0),
        has_previous: inputs.vector("OcclusionPreviousViewport").is_some_and(|v| v.z != 0.0),
    };
    c.execute();
    args.write(gpu, 0, &bytes(&c.args));
    ids.write(gpu, 0, &bytes(&c.ids));
    stats.write(gpu, 0, &bytes(&c.stats));
}

#[cfg(test)]
#[path = "occlusion_tests.rs"]
mod tests;
