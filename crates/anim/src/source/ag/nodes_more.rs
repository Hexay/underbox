//! `.ag` springs, eases, bone targets, blend2D, selectors and the IK nodes, read as the editor's
//! node `Compile` methods hand them to managed's GraphBuilder.

use super::{Compiler, Scope, f32_of, link, str_of, vec3_str};
use crate::graph::{Easing, LookAt, PoseNode, TwoBoneIk, ValueId, ValueNode, blend2d};
use crate::params::{Target, Value};
use serde_json::Value as Json;

fn list(n: &Json, key: &str) -> Vec<Json> {
    n.get(key).and_then(Json::as_array).cloned().unwrap_or_default()
}

fn start(n: &Json) -> Option<f32> {
    n.get("UseStartValue").and_then(Json::as_bool).unwrap_or(false).then(|| f32_of(n, "StartValue", 0.0))
}

impl Compiler {
    fn optional(&mut self, n: &Json, field: &str, scope: &Scope, default: Value) -> Option<ValueId> {
        link(scope, n.get(field)).is_some().then(|| self.value_input(n.get(field), scope, default))
    }

    pub(super) fn value_more(&mut self, class: &str, n: &Json, scope: &Scope) -> Option<ValueNode> {
        let input = |c: &mut Self| c.value_input(n.get("Value"), scope, Value::Float(0.0));
        Some(match class {
            "FloatSpringNode" => ValueNode::Spring { input: input(self), hertz: f32_of(n, "Hertz", 1.0), damping: f32_of(n, "DampingRatio", 1.0), start: start(n) },
            "FloatEaseNode" => {
                let easing = Easing::named(str_of(n, "Easing")).unwrap_or(Easing::Linear);
                ValueNode::Ease { input: input(self), time: f32_of(n, "EaseTime", 0.0), easing, start: start(n) }
            }
            "RandomFloatNode" => ValueNode::Random { min: f32_of(n, "Min", 0.0), max: f32_of(n, "Max", 1.0) },
            "ConstBoneTargetNode" => ValueNode::Const(Value::Target(Target { set: true, bone: Some(str_of(n, "Bone").to_owned()), ..Target::default() })),
            _ => return None,
        })
    }

    pub(super) fn pose_more(&mut self, class: &str, n: &Json, scope: &Scope) -> Option<PoseNode> {
        let options = |c: &mut Self| list(n, "Options").iter().map(|o| c.pose_input(o.get("Pose"), scope)).collect::<Vec<_>>();
        Some(match class {
            "Blend2DNode" => {
                let x = self.value_input(n.get("X"), scope, Value::Float(0.0));
                let y = self.value_input(n.get("Y"), scope, Value::Float(0.0));
                let points: Vec<([f32; 2], usize)> = list(n, "Points")
                    .iter()
                    .map(|p| {
                        let at = vec3_str(str_of(p, "Position"));
                        ([at.x, at.y], self.graph_result(p.get("Graph"), scope))
                    })
                    .collect();
                let triangles = blend2d::triangulate(&points.iter().map(|p| p.0).collect::<Vec<_>>());
                PoseNode::Blend2D { x, y, points, triangles }
            }
            "SelectorNode" | "AnimationClipSelectorNode" => {
                let poses = options(self);
                let graph = n.get("ConditionGraph").filter(|g| g.is_object());
                let inner = graph.map(|g| Scope::new(g, Some(scope)));
                let conds = inner.as_ref().and_then(|s| s.of_class("SelectorConditionNode")).map(|c| list(c, "Conditions")).unwrap_or_default();
                let s = inner.as_ref().unwrap_or(scope);
                let cases = poses.iter().enumerate().map(|(i, &p)| (self.value_input(conds.get(i), s, Value::Bool(false)), p)).collect();
                PoseNode::Selector { cases, fallback: poses.first().copied()? }
            }
            "ParameterizedSelectorNode" | "ParameterizedAnimationClipSelectorNode" => {
                let options = options(self);
                let seed = self.value_input(n.get("Parameter"), scope, Value::Float(0.0));
                let w: Vec<u32> = n.pointer("/DefaultVariationData/OptionWeights").and_then(Json::as_array).map(|a| a.iter().map(|w| w.as_u64().unwrap_or(0).min(255) as u32).collect()).unwrap_or_default();
                let mut distinct = w.clone();
                distinct.dedup();
                PoseNode::Variation { seed, options, weights: (distinct.len() > 1).then_some(w) }
            }
            "LookAtNode" => {
                let a = vec3_str(str_of(n, "ReferenceAngles"));
                let look = LookAt {
                    source: self.pose_input(n.get("Input"), scope),
                    pitch: self.value_input(n.get("Pitch"), scope, Value::Float(0.0)),
                    yaw: self.value_input(n.get("Yaw"), scope, Value::Float(0.0)),
                    weight: self.optional(n, "Weight", scope, Value::Float(1.0)),
                    reference: str_of(n, "ReferenceBone").to_owned(),
                    reference_rotation: crate::source::angles(a),
                    bones: list(n, "Bones").iter().map(|b| (str_of(b, "Bone").to_owned(), f32_of(b, "Weight", 0.0))).collect(),
                    yaw_limit: f32_of(n, "YawLimit", 140.0),
                    pitch_limit: f32_of(n, "PitchLimit", 140.0),
                };
                PoseNode::LookAt(Box::new(look))
            }
            "TwoBoneIKNode" => {
                let ik = TwoBoneIk {
                    source: self.pose_input(n.get("Input"), scope),
                    effector: n.pointer("/DefaultVariationData/EffectorBone").and_then(Json::as_str).unwrap_or_default().to_owned(),
                    target: self.value_input(n.get("EffectorTarget"), scope, Value::Target(Target::default())),
                    enabled: self.optional(n, "Enable", scope, Value::Bool(true)),
                    weight: self.optional(n, "Weight", scope, Value::Float(1.0)),
                    world: n.get("IsTargetInWorldSpace").and_then(Json::as_bool).unwrap_or(false),
                };
                PoseNode::TwoBoneIk(Box::new(ik))
            }
            _ => return None,
        })
    }
}
