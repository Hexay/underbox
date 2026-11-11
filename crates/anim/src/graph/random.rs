//! `RandomFloat` nodes: "a uniform random value each time this node is initialized, holding it
//! until shutdown" (GraphBuilder.Values.cs). A value node is initialized with the pose node that
//! reads it, so starting (or restarting) a pose node draws fresh values for the random nodes its
//! own inputs reach; until then they hold. Each instance has its own seeded stream, distinct
//! between instances, and `restart` rewinds it so a recording replays the same draws.

use super::eval::Instance;
use super::{Operand, PoseId, PoseNode, ValueId, ValueNode};
use std::sync::atomic::{AtomicU64, Ordering};

/// SplitMix64: a full-period stream from any seed.
fn mix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A seed no other instance in this process got.
pub(super) fn next_seed() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0x5EED);
    let mut s = COUNTER.fetch_add(1, Ordering::Relaxed);
    mix(&mut s)
}

/// Per-instance draws: the stream and each value node's held sample.
#[derive(Debug, Clone)]
pub(super) struct Draws {
    seed: u64,
    state: u64,
    pub held: Vec<f32>,
}

impl Draws {
    pub fn new(seed: u64, values: &[ValueNode]) -> Self {
        let mut d = Self { seed, state: seed, held: vec![0.0; values.len()] };
        d.rewind(values);
        d
    }

    /// Back to the seed's start, drawing every random node afresh.
    pub fn rewind(&mut self, values: &[ValueNode]) {
        self.state = self.seed;
        for id in 0..values.len() {
            self.draw(values, id);
        }
    }

    fn draw(&mut self, values: &[ValueNode], id: ValueId) {
        if let Some(ValueNode::Random { min, max }) = values.get(id) {
            let unit = (mix(&mut self.state) >> 40) as f32 / ((1u64 << 24) - 1) as f32;
            self.held[id] = min + (max - min) * unit;
        }
    }
}

fn operand(o: &Operand) -> Option<ValueId> {
    match o {
        Operand::Node(id) => Some(*id),
        Operand::Const(_) => None,
    }
}

/// The value nodes `v` reads directly.
fn value_inputs(v: &ValueNode) -> Vec<ValueId> {
    match v {
        ValueNode::Not(a) => vec![*a],
        ValueNode::And(vs) | ValueNode::Or(vs) => vs.clone(),
        ValueNode::FloatCompare { value, rhs, .. } => [Some(*value), operand(rhs)].into_iter().flatten().collect(),
        ValueNode::IdMatches { id, .. } => vec![*id],
        ValueNode::FloatMath { a, b, .. } => [Some(*a), operand(b)].into_iter().flatten().collect(),
        ValueNode::FloatClamp { value, .. } | ValueNode::FloatRemap { value, .. } => vec![*value],
        ValueNode::Select { cond, yes, no } => [Some(*cond), operand(yes), operand(no)].into_iter().flatten().collect(),
        ValueNode::Ease { input, .. } | ValueNode::Spring { input, .. } => vec![*input],
        _ => Vec::new(),
    }
}

/// The value nodes pose node `p` reads directly (a state machine's are its transition conditions).
fn pose_values(p: &PoseNode) -> Vec<ValueId> {
    match p {
        PoseNode::SpeedScale { speed, .. } => vec![*speed],
        PoseNode::Blend { weight, .. } => vec![*weight],
        PoseNode::Blend1D { param, .. } => vec![*param],
        PoseNode::Blend2D { x, y, .. } => vec![*x, *y],
        PoseNode::StateMachine { states, .. } => states.iter().flat_map(|s| s.transitions.iter().map(|t| t.condition)).collect(),
        PoseNode::Selector { cases, .. } => cases.iter().map(|c| c.0).collect(),
        PoseNode::Variation { seed, .. } => vec![*seed],
        PoseNode::TwoBoneIk(ik) => [Some(ik.target), ik.enabled, ik.weight].into_iter().flatten().collect(),
        PoseNode::LookAt(l) => [Some(l.pitch), Some(l.yaw), l.weight].into_iter().flatten().collect(),
        _ => Vec::new(),
    }
}

impl Instance {
    /// Draw again for every random node pose node `p` reaches through its value inputs.
    pub(super) fn redraw_for(&mut self, p: PoseId) {
        let def = self.definition().clone();
        let Some(node) = def.poses.get(p) else { return };
        let mut pending = pose_values(node);
        let mut seen = vec![false; def.values.len()];
        while let Some(id) = pending.pop() {
            if id >= seen.len() || std::mem::replace(&mut seen[id], true) {
                continue;
            }
            self.draws.draw(&def.values, id);
            pending.extend(value_inputs(&def.values[id]));
        }
    }
}
