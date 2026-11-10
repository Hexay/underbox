//! Graph instances: per-node runtime state, advanced and sampled in one recursive pass from the
//! root. Only active nodes advance; entering a state restarts its subtree.

use super::value::{StateCtx, Values};
use super::{Definition, PoseId, PoseNode, State};
use crate::clip::Clip;
use crate::params::Params;
use crate::pose::Pose;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
struct Machine {
    started: bool,
    current: usize,
    elapsed: f32,
    /// The current state's subtree reported finished last update.
    finished: bool,
    /// Blending out of `(state, progress, duration)`.
    from: Option<(usize, f32, f32)>,
}

#[derive(Debug, Clone, Copy, Default)]
enum NodeState {
    #[default]
    Idle,
    Clip { time: f32 },
    Machine(Machine),
}

struct Out {
    pose: Pose,
    finished: bool,
}

pub struct Instance {
    def: Arc<Definition>,
    pub params: Params,
    clips: Vec<Option<Arc<Clip>>>,
    nodes: Vec<NodeState>,
}

impl Instance {
    /// `clip` resolves each animation slot name to a clip (missing ones sample as no change).
    pub fn new(def: Arc<Definition>, clip: impl Fn(&str) -> Option<Arc<Clip>>) -> Self {
        let params = Params::new(def.params.clone().into());
        let clips = def.slots.iter().map(|s| clip(s)).collect();
        let nodes = vec![NodeState::Idle; def.poses.len()];
        Self { def, params, clips, nodes }
    }

    pub fn definition(&self) -> &Arc<Definition> {
        &self.def
    }

    /// The current state of state machine `node` (its index in the definition).
    pub fn current_state(&self, node: PoseId) -> Option<&str> {
        let (NodeState::Machine(m), Some(PoseNode::StateMachine { states, .. })) = (self.nodes.get(node)?, self.def.poses.get(node)) else {
            return None;
        };
        states.get(m.current).map(|s| s.name.as_str())
    }

    /// Forget every node's progress, as if just created (parameters are kept).
    pub fn restart(&mut self) {
        self.nodes.fill(NodeState::Idle);
    }

    /// Advance by `dt` seconds and return the pose. `bind` is the skeleton's bind pose.
    pub fn update(&mut self, dt: f32, bind: &Pose) -> Pose {
        match self.def.root {
            Some(root) => self.eval(root, dt, bind).pose,
            None => bind.clone(),
        }
    }

    fn values(&self, ctx: StateCtx) -> Values<'_> {
        Values { def: &self.def, params: &self.params, ctx }
    }

    fn eval(&mut self, id: PoseId, dt: f32, bind: &Pose) -> Out {
        let def = self.def.clone();
        let done = |pose| Out { pose, finished: false };
        match &def.poses[id] {
            PoseNode::Zero => done(Pose::zero(bind.0.len())),
            PoseNode::Reference => done(bind.clone()),
            PoseNode::Clip { slot, looping, speed } => self.clip(id, *slot, *looping, dt * speed, bind),
            PoseNode::SpeedScale { source, speed } => {
                let s = self.values(StateCtx::default()).float(*speed);
                self.eval(*source, dt * s, bind)
            }
            PoseNode::Blend { base, layer, weight } => {
                let w = self.values(StateCtx::default()).float(*weight).clamp(0.0, 1.0);
                let a = self.eval(*base, dt, bind);
                let b = self.eval(*layer, dt, bind);
                Out { pose: a.pose.blend(&b.pose, w), finished: a.finished }
            }
            PoseNode::Blend1D { param, points } => self.blend1d(*param, points, dt, bind),
            PoseNode::StateMachine { states, default } => self.machine(id, states, *default, dt, bind),
        }
    }

    fn clip(&mut self, id: PoseId, slot: usize, looping: bool, dt: f32, bind: &Pose) -> Out {
        let time = match self.nodes[id] {
            NodeState::Clip { time } => time + dt,
            _ => 0.0,
        };
        self.nodes[id] = NodeState::Clip { time };
        let Some(clip) = self.clips.get(slot).cloned().flatten() else { return Out { pose: bind.clone(), finished: true } };
        let d = clip.duration();
        let t = if looping && d > 0.0 { time.rem_euclid(d) } else { time.clamp(0.0, d) };
        let mut pose = bind.clone();
        clip.sample_frame(t * clip.fps, &mut pose);
        Out { pose, finished: !looping && time >= d }
    }

    fn blend1d(&mut self, param: usize, points: &[(f32, PoseId)], dt: f32, bind: &Pose) -> Out {
        let v = self.values(StateCtx::default()).float(param);
        let Some(&(_, first_pose)) = points.first() else { return Out { pose: bind.clone(), finished: true } };
        let upper = points.iter().position(|&(x, _)| x > v);
        let (a, b, t) = match upper {
            None => (points[points.len() - 1].1, None, 0.0),
            Some(0) => (first_pose, None, 0.0),
            Some(i) => {
                let ((x0, p0), (x1, p1)) = (points[i - 1], points[i]);
                (p0, Some(p1), (v - x0) / (x1 - x0))
            }
        };
        let lo = self.eval(a, dt, bind);
        match b {
            Some(b) => {
                let hi = self.eval(b, dt, bind);
                Out { pose: lo.pose.blend(&hi.pose, t), finished: if t < 0.5 { lo.finished } else { hi.finished } }
            }
            None => lo,
        }
    }

    fn machine(&mut self, id: PoseId, states: &[State], default: usize, dt: f32, bind: &Pose) -> Out {
        let mut m = match self.nodes[id] {
            NodeState::Machine(m) => m,
            _ => Machine::default(),
        };
        if !m.started || m.current >= states.len() {
            m = Machine { started: true, current: default.min(states.len().saturating_sub(1)), ..Machine::default() };
            if let Some(s) = states.get(m.current) {
                self.reset(s.pose);
            }
        }
        if states.is_empty() {
            return Out { pose: bind.clone(), finished: true };
        }
        let cur = self.eval(states[m.current].pose, dt, bind);
        m.elapsed += dt;
        m.finished = cur.finished;
        let mut pose = match m.from {
            Some((from, progress, duration)) => {
                let progress = progress + dt;
                let old = self.eval(states[from].pose, dt, bind);
                m.from = (progress < duration).then_some((from, progress, duration));
                old.pose.blend(&cur.pose, (progress / duration).min(1.0))
            }
            None => cur.pose,
        };
        // Transitions see the state after this update; the new state starts at weight 0.
        let values = self.values(StateCtx { elapsed: m.elapsed, completed: m.finished });
        let taken = states[m.current].transitions.iter().find(|t| t.to != m.current && t.to < states.len() && values.bool(t.condition));
        if let Some(t) = taken.cloned() {
            m = Machine { started: true, current: t.to, from: (t.duration > 0.0).then_some((m.current, 0.0, t.duration)), ..Machine::default() };
            self.reset(states[t.to].pose);
            let new = self.eval(states[t.to].pose, 0.0, bind);
            if m.from.is_none() {
                pose = new.pose;
            }
        }
        self.nodes[id] = NodeState::Machine(m);
        Out { pose, finished: m.finished }
    }

    /// Restart `id` and everything under it.
    fn reset(&mut self, id: PoseId) {
        let def = self.def.clone();
        self.nodes[id] = NodeState::Idle;
        match &def.poses[id] {
            PoseNode::SpeedScale { source, .. } => self.reset(*source),
            PoseNode::Blend { base, layer, .. } => {
                self.reset(*base);
                self.reset(*layer);
            }
            PoseNode::Blend1D { points, .. } => points.iter().for_each(|&(_, p)| self.reset(p)),
            PoseNode::StateMachine { .. } | PoseNode::Clip { .. } | PoseNode::Zero | PoseNode::Reference => {}
        }
    }
}
