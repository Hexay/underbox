//! The IK pose nodes on an instance: two-bone IK (with its enable blend, blend mode and
//! reference-pose twist) and look-at chains. The solvers are in `ik`.

use super::eval::{Instance, NodeState, Out};
use super::value::StateCtx;
use super::{LookAt, PoseId, TwoBoneIk, ValueId, ik};
use crate::params::Value;
use crate::pose::Pose;
use crate::xform::Xform;

impl Instance {
    /// The target's model-space transform, `None` when unset or unresolvable.
    fn target(&self, rig: &super::Rig, value: ValueId, world: bool, pose: &Pose) -> Option<Xform> {
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

    fn weight(&self, weight: Option<ValueId>) -> f32 {
        weight.map_or(1.0, |w| self.values(StateCtx::default()).float(w).clamp(0.0, 1.0))
    }

    /// Node `id`'s enable blend after `dt`: it starts where `enabled` is, then moves 0 ↔ 1 over
    /// `blend_time` seconds each time it flips.
    fn enable_blend(&mut self, id: PoseId, n: &TwoBoneIk, dt: f32) -> f32 {
        let on = n.enabled.is_none_or(|e| self.values(StateCtx::default()).bool(e));
        let goal = if on { 1.0 } else { 0.0 };
        let blend = match self.nodes[id] {
            NodeState::Ik(b) if n.blend_time > 0.0 => {
                let step = dt / n.blend_time;
                let next = if b < goal { (b + step).min(goal) } else { (b - step).max(goal) };
                // Summed steps miss the end by rounding; land on it.
                if (next - goal).abs() < 1e-4 { goal } else { next }
            }
            _ => goal,
        };
        self.nodes[id] = NodeState::Ik(blend);
        blend
    }

    pub(super) fn two_bone_ik(&mut self, id: PoseId, n: &TwoBoneIk, dt: f32, bind: &Pose) -> Out {
        let out = self.eval(n.source, dt, bind);
        let w = self.enable_blend(id, n, dt) * self.weight(n.weight);
        let Some(rig) = self.rig.clone().filter(|_| w > 0.0) else { return out };
        let (Some(effector), Some(target)) = (rig.skeleton.find(&n.effector), self.target(&rig, n.target, n.world, &out.pose)) else { return out };
        let s = &rig.skeleton;
        if out.pose.0.len() < s.len() {
            return out;
        }
        let goal = if n.blend_target { s.model_space(&out.pose.0)[effector].lerp(&target, w) } else { target };
        let mut solved = out.pose.clone();
        if !ik::two_bone(s, &mut solved, effector, &goal) {
            return out;
        }
        if n.twist > 0.0 && bind.0.len() >= s.len() {
            ik::twist_toward(s, &mut solved, effector, bind, n.twist.min(1.0));
        }
        let pose = if n.blend_target { solved } else { out.pose.blend(&solved, w) };
        Out { pose, finished: out.finished }
    }

    pub(super) fn look_at(&mut self, n: &LookAt, dt: f32, bind: &Pose) -> Out {
        let out = self.eval(n.source, dt, bind);
        let w = self.weight(n.weight);
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
