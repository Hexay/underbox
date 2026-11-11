//! Animation graphs (AnimGraph2): a compiled [`Definition`] of value and pose nodes, and an
//! [`Instance`] that holds one model's parameters and node state and evaluates a pose each update.
//! Value nodes compute bools/floats/IDs from parameters; pose nodes sample clips and combine them.

pub mod blend2d;
mod eval;
mod eval_more;
pub mod ik;
pub mod recording;
mod smooth;
mod value;

pub use eval::Instance;
pub use eval_more::Rig;
pub use smooth::ease;
pub use recording::Recording;

use crate::params::{ParamDef, Value};
use std::sync::Arc;

pub type ValueId = usize;
pub type PoseId = usize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmp {
    Eq,
    NearEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MathOp {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    /// Remainder with the sign of `a` (C `fmod`); 0 when `b` is 0.
    Mod,
    // The rest are unary: `b` is ignored.
    Floor,
    Ceil,
    /// The integer part (toward zero).
    Trunc,
    /// `a - trunc(a)`.
    Fract,
    /// `1 - fract(a)`.
    InvFract,
}

/// A node input that is either another value node or a constant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Operand {
    Node(ValueId),
    Const(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValueNode {
    Param(usize),
    Const(Value),
    Not(ValueId),
    And(Vec<ValueId>),
    Or(Vec<ValueId>),
    FloatCompare { value: ValueId, cmp: Cmp, rhs: Operand, epsilon: f32 },
    /// True when the ID is (or, `negate`, isn't) one of `ids`.
    IdMatches { id: ValueId, ids: Vec<String>, negate: bool },
    FloatMath { a: ValueId, b: Operand, op: MathOp, abs: bool, negate: bool },
    FloatClamp { value: ValueId, min: f32, max: f32 },
    FloatRemap { value: ValueId, from: (f32, f32), to: (f32, f32) },
    /// `yes` when `cond` holds, else `no`.
    Select { cond: ValueId, yes: Operand, no: Operand },
    /// Seconds in the state whose transitions are being checked.
    TimeInState { cmp: Cmp, seconds: f32 },
    /// The state being left has finished playing.
    StateCompleted,
    /// Eases toward `input` over `time` seconds whenever it changes; starts at `start`, else
    /// at the input.
    Ease { input: ValueId, time: f32, easing: Easing, start: Option<f32> },
    /// A damped spring (`hertz`, damping ratio) tracking `input`.
    Spring { input: ValueId, hertz: f32, damping: f32, start: Option<f32> },
}

/// managed's `GraphBuilder.Easing`, in its order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    Linear,
    InQuad,
    OutQuad,
    InOutQuad,
    InCubic,
    OutCubic,
    InOutCubic,
    InQuart,
    OutQuart,
    InOutQuart,
    InQuint,
    OutQuint,
    InOutQuint,
    InSine,
    OutSine,
    InOutSine,
    InExpo,
    OutExpo,
    InOutExpo,
    InCirc,
    OutCirc,
    InOutCirc,
    /// Jumps straight to the target.
    None,
}

impl Easing {
    pub const ALL: [Easing; 23] = {
        use Easing::*;
        [
            Linear, InQuad, OutQuad, InOutQuad, InCubic, OutCubic, InOutCubic, InQuart, OutQuart, InOutQuart, InQuint, OutQuint, InOutQuint, InSine, OutSine, InOutSine,
            InExpo, OutExpo, InOutExpo, InCirc, OutCirc, InOutCirc, None,
        ]
    };

    /// By managed's enum name (as `.ag` files store it).
    pub fn named(name: &str) -> Option<Easing> {
        Self::ALL.into_iter().find(|e| format!("{e:?}") == name)
    }
}

/// Two-bone IK: the effector's parent and grandparent turn so it reaches `target`, and it takes
/// the target's rotation.
#[derive(Debug, Clone, PartialEq)]
pub struct TwoBoneIk {
    pub source: PoseId,
    pub effector: String,
    /// A target value (`Value::Target`).
    pub target: ValueId,
    pub enabled: Option<ValueId>,
    pub weight: Option<ValueId>,
    /// The target is in world space (else the character's).
    pub world: bool,
}

/// A look-at chain: `bones` (end first) share a yaw/pitch turn of the reference bone's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct LookAt {
    pub source: PoseId,
    /// Degrees; positive pitch looks down, positive yaw turns left (about +Z).
    pub pitch: ValueId,
    pub yaw: ValueId,
    pub weight: Option<ValueId>,
    pub reference: String,
    pub reference_rotation: glam::Quat,
    /// (bone, cumulative weight), end to root, weights descending.
    pub bones: Vec<(String, f32)>,
    pub yaw_limit: f32,
    pub pitch_limit: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub to: usize,
    pub condition: ValueId,
    pub duration: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub name: String,
    pub pose: PoseId,
    /// Checked in order; the first whose condition holds is taken.
    pub transitions: Vec<Transition>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PoseNode {
    Zero,
    Reference,
    Clip { slot: usize, looping: bool, speed: f32 },
    /// Children at ascending parameter values; the parameter picks two and blends.
    Blend1D { param: ValueId, points: Vec<(f32, PoseId)>, },
    StateMachine { states: Vec<State>, default: usize },
    SpeedScale { source: PoseId, speed: ValueId },
    /// `base` blended toward `layer` by `weight` (0..1).
    Blend { base: PoseId, layer: PoseId, weight: ValueId },
    /// Children at points on the (x, y) parameter plane, triangulated; the parameters pick a
    /// triangle (or the nearest point on one) and blend its corners.
    Blend2D { x: ValueId, y: ValueId, points: Vec<([f32; 2], PoseId)>, triangles: Vec<[usize; 3]> },
    /// Chosen when the node starts: the first case whose condition holds, else `fallback`.
    Selector { cases: Vec<(ValueId, PoseId)>, fallback: PoseId },
    /// Chosen when the node starts from a float: by weight (`seed`'s fraction picks), or as an
    /// option index when there are no weights.
    Variation { seed: ValueId, options: Vec<PoseId>, weights: Option<Vec<u32>> },
    TwoBoneIk(Box<TwoBoneIk>),
    LookAt(Box<LookAt>),
}

impl PoseNode {
    /// The pose nodes this one evaluates.
    pub fn inputs(&self) -> Vec<PoseId> {
        match self {
            PoseNode::Zero | PoseNode::Reference | PoseNode::Clip { .. } | PoseNode::StateMachine { .. } => Vec::new(),
            PoseNode::SpeedScale { source, .. } => vec![*source],
            PoseNode::Blend { base, layer, .. } => vec![*base, *layer],
            PoseNode::Blend1D { points, .. } => points.iter().map(|p| p.1).collect(),
            PoseNode::Blend2D { points, .. } => points.iter().map(|p| p.1).collect(),
            PoseNode::Selector { cases, fallback } => cases.iter().map(|c| c.1).chain([*fallback]).collect(),
            PoseNode::Variation { options, .. } => options.clone(),
            PoseNode::TwoBoneIk(ik) => vec![ik.source],
            PoseNode::LookAt(l) => vec![l.source],
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Definition {
    pub params: Vec<ParamDef>,
    pub values: Vec<ValueNode>,
    pub poses: Vec<PoseNode>,
    pub root: Option<PoseId>,
    /// Animation slots: the clip names pose nodes refer to by index.
    pub slots: Vec<String>,
    pub model: String,
    pub variation: String,
}

impl Definition {
    pub fn add_value(&mut self, v: ValueNode) -> ValueId {
        self.values.push(v);
        self.values.len() - 1
    }

    pub fn add_pose(&mut self, p: PoseNode) -> PoseId {
        self.poses.push(p);
        self.poses.len() - 1
    }

    /// The slot for clip `name`, added if new.
    pub fn slot(&mut self, name: &str) -> usize {
        self.slots.iter().position(|s| s == name).unwrap_or_else(|| {
            self.slots.push(name.to_owned());
            self.slots.len() - 1
        })
    }

    pub fn param(&self, name: &str) -> Option<usize> {
        self.params.iter().position(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn into_shared(self) -> Arc<Definition> {
        Arc::new(self)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_nodes;
