//! The instance's multi-input nodes: blend2D and the selectors. The IK nodes are in `ik_node`.

use super::eval::{Instance, NodeState, Out};
use super::value::StateCtx;
use super::{PoseId, ValueId, blend2d};
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
        let Some(&(top, _)) = weights.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else { return Out::ended(bind.clone()) };
        let (mut pose, mut sum, mut finished, mut remaining) = (None::<Pose>, 0.0, false, 0.0);
        for (i, w) in weights {
            let out = self.eval(points[i].1, dt, bind);
            sum += w;
            if i == top {
                (finished, remaining) = (out.finished, out.remaining);
            }
            pose = Some(match pose {
                None => out.pose,
                Some(acc) => acc.blend(&out.pose, w / sum),
            });
        }
        Out { pose: pose.unwrap_or_else(|| bind.clone()), finished, remaining }
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
}
