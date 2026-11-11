//! The instance's multi-input and effect nodes: blend2D, selectors and the IK nodes.

use super::eval::{Instance, NodeState, Out};
use super::value::StateCtx;
use super::{LookAt, PoseId, TwoBoneIk, ValueId, blend2d, ik};
use crate::params::Value;
use crate::pose::Pose;
use crate::skeleton::Skeleton;
use crate::xform::Xform;
use std::sync::Arc;

/// What the IK nodes need beyond the pose: the bones, and where the character is in the world
/// (for world-space targets).
#[derive(Debug, Clone)]
pub struct Rig {
    pub skeleton: Arc<Skeleton>,
    pub world: Xform,
}

impl Instance {
    pub(super) fn blend2d(&mut self, axes: (ValueId, ValueId), points: &[([f32; 2], PoseId)], triangles: &[[usize; 3]], dt: f32, bind: &Pose) -> Out {
        let v = self.values(StateCtx::default());
        let at = [v.float(axes.0), v.float(axes.1)];
        let coords: Vec<[f32; 2]> = points.iter().map(|p| p.0).collect();
        let mut weights = blend2d::weights(at, &coords, triangles);
        weights.retain(|w| w.1 > 1e-6);
        let Some(&(top, _)) = weights.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else { return Out { pose: bind.clone(), finished: true } };
        let (mut pose, mut sum, mut finished) = (None::<Pose>, 0.0, false);
        for (i, w) in weights {
            let out = self.eval(points[i].1, dt, bind);
            sum += w;
            finished |= i == top && out.finished;
            pose = Some(match pose {
                None => out.pose,
                Some(acc) => acc.blend(&out.pose, w / sum),
            });
        }
        Out { pose: pose.unwrap_or_else(|| bind.clone()), finished }
    }

    /// The option `id` chose when it started, choosing it now if it hasn't.
    pub(super) fn chosen(&mut self, id: PoseId, choose: impl FnOnce(&Self) -> PoseId) -> PoseId {
        if let NodeState::Chosen(p) = self.nodes[id] {
            return p;
        }
        let p = choose(self);
        self.nodes[id] = NodeState::Chosen(p);
        p
    }

    pub(super) fn pick_case(&self, cases: &[(ValueId, PoseId)], fallback: PoseId) -> PoseId {
        let v = self.values(StateCtx::default());
        cases.iter().find(|c| v.bool(c.0)).map_or(fallback, |c| c.1)
    }

    /// The seed's fractional part walks the weights; without weights the seed is an index.
    pub(super) fn pick_variation(&self, seed: ValueId, options: &[PoseId], weights: Option<&[u32]>) -> PoseId {
        let s = self.values(StateCtx::default()).float(seed);
        let last = options.len().saturating_sub(1);
        let k = match weights {
            Some(w) if w.iter().sum::<u32>() > 0 => {
                let total: u32 = w.iter().take(options.len()).sum();
                let mut pick = s.rem_euclid(1.0) * total as f32;
                w.iter().take(options.len()).position(|&x| {
                    pick -= x as f32;
                    x > 0 && pick < 0.0
                })
            }
            _ => Some(s.max(0.0) as usize),
        };
        options[k.unwrap_or(last).min(last)]
    }

    /// The target's model-space transform, `None` when unset or unresolvable.
    fn target(&self, rig: &Rig, value: ValueId, world: bool, pose: &Pose) -> Option<Xform> {
        let Value::Target(t) = self.values(StateCtx::default()).get(value) else { return None };
        if !t.set {
            return None;
        }
        match t.bone.as_deref().filter(|b| !b.is_empty()) {
            Some(bone) => Some(rig.skeleton.model_space(&pose.0)[rig.skeleton.find(bone)?]),
            None if world => Some(rig.world.inverse().mul(&t.xform)),
            None => Some(t.xform),
        }
    }

    fn weight(&self, enabled: Option<ValueId>, weight: Option<ValueId>) -> f32 {
        let v = self.values(StateCtx::default());
        if enabled.is_some_and(|e| !v.bool(e)) {
            return 0.0;
        }
        weight.map_or(1.0, |w| v.float(w).clamp(0.0, 1.0))
    }

    pub(super) fn two_bone_ik(&mut self, n: &TwoBoneIk, dt: f32, bind: &Pose) -> Out {
        let out = self.eval(n.source, dt, bind);
        let w = self.weight(n.enabled, n.weight);
        let Some(rig) = self.rig.clone().filter(|_| w > 0.0) else { return out };
        let (Some(effector), Some(target)) = (rig.skeleton.find(&n.effector), self.target(&rig, n.target, n.world, &out.pose)) else { return out };
        let mut solved = out.pose.clone();
        if !ik::two_bone(&rig.skeleton, &mut solved, effector, &target) {
            return out;
        }
        Out { pose: out.pose.blend(&solved, w), finished: out.finished }
    }

    pub(super) fn look_at(&mut self, n: &LookAt, dt: f32, bind: &Pose) -> Out {
        let out = self.eval(n.source, dt, bind);
        let w = self.weight(None, n.weight);
        let Some(rig) = self.rig.clone().filter(|_| w > 0.0) else { return out };
        let v = self.values(StateCtx::default());
        let yaw = v.float(n.yaw).clamp(-n.yaw_limit.abs(), n.yaw_limit.abs()) * w;
        let pitch = v.float(n.pitch).clamp(-n.pitch_limit.abs(), n.pitch_limit.abs()) * w;
        let s = &rig.skeleton;
        let frame = s.find(&n.reference).map_or(glam::Quat::IDENTITY, |r| s.model_space(&out.pose.0)[r].rot) * n.reference_rotation;
        let bones: Vec<(usize, f32)> = n.bones.iter().filter_map(|(b, w)| Some((s.find(b)?, *w))).collect();
        let mut pose = out.pose;
        if pose.0.len() >= s.len() {
            ik::look_at(s, &mut pose, frame, yaw, pitch, &bones);
        }
        Out { pose, finished: out.finished }
    }
}
