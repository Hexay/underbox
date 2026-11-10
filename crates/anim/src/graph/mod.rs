//! Animation graphs (AnimGraph2): a compiled [`Definition`] of value and pose nodes, and an
//! [`Instance`] that holds one model's parameters and node state and evaluates a pose each update.
//! Value nodes compute bools/floats/IDs from parameters; pose nodes sample clips and combine them.

mod eval;
pub mod recording;
mod value;

pub use eval::Instance;
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
