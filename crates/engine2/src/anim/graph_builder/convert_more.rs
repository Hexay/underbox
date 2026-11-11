//! Builder nodes beyond `convert`/`convert_pose`: eases, springs, random floats and const targets; blend2D,
//! the selectors, and the IK nodes (foot IK as a two-bone IK per foot).

use super::convert::Conv;
use super::snapshot::Node;
use anim::graph::{Easing, LookAt, PoseId, PoseNode, TwoBoneIk, ValueId, ValueNode};
use anim::params::{Target, Value};
use glam::{Quat, Vec3};

fn start(n: &Node) -> Option<f32> {
    n.bool("m_useStartValue").then(|| n.float("m_startValue"))
}

fn quat(c: &[f32]) -> Quat {
    match c {
        [x, y, z, w] => Quat::from_xyzw(*x, *y, *z, *w).normalize(),
        _ => Quat::IDENTITY,
    }
}

impl<'a> Conv<'a> {
    fn optional(&mut self, i: i64) -> Option<ValueId> {
        (i >= 0).then(|| self.value(i))
    }

    pub(super) fn value_more(&mut self, class: &str, n: &'a Node) -> Option<ValueNode> {
        let input = n.idx("m_inputValueNodeIdx");
        Some(match class {
            "CFloatEaseNode" => {
                let easing = Easing::ALL.get(n.setter("EasingOp") as usize).copied().unwrap_or(Easing::Linear);
                ValueNode::Ease { input: self.value(input), time: n.float("m_easeTime"), easing, start: start(n) }
            }
            "CFloatSpringNode" => ValueNode::Spring { input: self.value(input), hertz: n.float("m_hertz"), damping: n.float("m_dampingRatio"), start: start(n) },
            "CFloatRandomNode" => ValueNode::Random { min: n.float("m_min"), max: n.float("m_max") },
            "CConstTargetNode" => {
                let bone = n.bool("m_isBoneTarget").then(|| self.id_name(n.setter("BoneID")));
                let t = n.components("m_translation");
                let pos = Vec3::new(t.first().copied().unwrap_or(0.0), t.get(1).copied().unwrap_or(0.0), t.get(2).copied().unwrap_or(0.0));
                let xform = anim::Xform::new(pos, quat(&n.components("m_rotation")));
                ValueNode::Const(Value::Target(Target { set: n.bool("m_isSet"), bone, xform }))
            }
            _ => return None,
        })
    }

    pub(super) fn pose_more(&mut self, class: &str, n: &'a Node) -> Option<PoseId> {
        let options = |c: &mut Self| n.ints("OptionNodeIndices").iter().map(|&i| c.pose(i)).collect::<Vec<_>>();
        let node = match class {
            "CBlend2DNode" => {
                let (x, y) = (self.value(n.idx("m_inputParameterNodeIdx0")), self.value(n.idx("m_inputParameterNodeIdx1")));
                let points = n.ints("SourceNodeIndices").iter().zip(n.vec2("Values")).map(|(&src, &at)| (at, self.pose(src))).collect();
                let triangles = n.ints("Indices").as_chunks::<3>().0.iter().map(|t| t.map(|i| i as usize)).collect();
                PoseNode::Blend2D { x, y, points, triangles }
            }
            "CSelectorNode" | "CClipSelectorNode" => {
                let poses = options(self);
                let conds: Vec<ValueId> = n.ints("ConditionNodeIndices").iter().map(|&i| self.value(i)).collect();
                PoseNode::Selector { cases: conds.into_iter().zip(poses.iter().copied()).collect(), fallback: *poses.first()? }
            }
            "CParameterizedSelectorNode" | "CParameterizedClipSelectorNode" => {
                let weights = n.bool("m_hasWeightsSet").then(|| n.ints("OptionWeights").iter().map(|&w| w.clamp(0, 255) as u32).collect());
                PoseNode::Variation { seed: self.value(n.idx("m_parameterNodeIdx")), options: options(self), weights }
            }
            "CIDBasedSelectorNode" | "CIDBasedClipSelectorNode" => {
                let poses = options(self);
                let id = self.value(n.idx("m_parameterNodeIdx"));
                let names: Vec<String> = n.ints("OptionIDs").iter().map(|&h| self.id_name(h)).collect();
                let cases = names.into_iter().zip(poses.iter().copied()).map(|(name, p)| (self.def.add_value(ValueNode::IdMatches { id, ids: vec![name], negate: false }), p)).collect();
                let fallback = n.idx("m_fallbackNodeIdx");
                PoseNode::Selector { cases, fallback: if fallback >= 0 { self.pose(fallback) } else { *poses.first()? } }
            }
            "CTwoBoneIKNode" => {
                let source = self.pose(n.idx("m_childNodeIdx"));
                return Some(self.two_bone(n, source, "EffectorBoneID", "m_effectorTargetNodeIdx"));
            }
            "CFootIKNode" => {
                let source = self.pose(n.idx("m_childNodeIdx"));
                let left = self.two_bone(n, source, "LeftFootBoneID", "m_leftTargetNodeIdx");
                return Some(self.two_bone(n, left, "RightFootBoneID", "m_rightTargetNodeIdx"));
            }
            "CLookAtChainNode" => {
                let ids = n.ints("BoneIDs");
                let look = LookAt {
                    source: self.pose(n.idx("m_childNodeIdx")),
                    pitch: self.value(n.idx("m_pitchNodeIdx")),
                    yaw: self.value(n.idx("m_yawNodeIdx")),
                    weight: self.optional(n.idx("m_weightNodeIdx")),
                    reference: self.id_name(n.setter("ReferenceBoneID")),
                    reference_rotation: quat(&n.components("m_referenceRotation")),
                    bones: ids.iter().zip(n.floats("Weights")).map(|(&id, &w)| (self.id_name(id), w)).collect(),
                    yaw_limit: n.float("m_yawLimit"),
                    pitch_limit: n.float("m_pitchLimit"),
                };
                PoseNode::LookAt(Box::new(look))
            }
            _ => return None,
        };
        Some(self.def.add_pose(node))
    }

    /// A two-bone IK on `source`; an unset target leaves it as it is.
    fn two_bone(&mut self, n: &'a Node, source: PoseId, bone: &str, target: &str) -> PoseId {
        let target = n.idx(target);
        if target < 0 {
            return source;
        }
        let ik = TwoBoneIk {
            source,
            effector: self.id_name(n.setter(bone)),
            target: self.value(target),
            enabled: self.optional(n.idx("m_enableNodeIdx")),
            weight: self.optional(n.idx("m_weightNodeIdx")),
            world: n.bool("m_isTargetInWorldSpace"),
        };
        self.def.add_pose(PoseNode::TwoBoneIk(Box::new(ik)))
    }
}
