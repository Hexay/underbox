//! Value-node evaluation. Values are pure functions of parameters and the state context, so
//! they're recomputed on demand rather than cached.

use super::smooth::Smooth;
use super::{Cmp, Definition, MathOp, Operand, ValueId, ValueNode};
use crate::params::{Params, Value};

/// What transition conditions see about the state they'd leave.
#[derive(Debug, Clone, Copy, Default)]
pub struct StateCtx {
    pub elapsed: f32,
    pub completed: bool,
}

pub fn compare(a: f32, cmp: Cmp, b: f32, epsilon: f32) -> bool {
    match cmp {
        Cmp::Eq => a == b,
        Cmp::NearEq => (a - b).abs() <= epsilon.max(1e-5),
        Cmp::Ne => a != b,
        Cmp::Lt => a < b,
        Cmp::Le => a <= b,
        Cmp::Gt => a > b,
        Cmp::Ge => a >= b,
    }
}

pub(super) fn math(a: f32, b: f32, op: MathOp) -> f32 {
    match op {
        MathOp::Add => a + b,
        MathOp::Sub => a - b,
        MathOp::Mul => a * b,
        MathOp::Div if b == 0.0 => 0.0,
        MathOp::Div => a / b,
        MathOp::Min => a.min(b),
        MathOp::Max => a.max(b),
        MathOp::Mod if b == 0.0 => 0.0,
        MathOp::Mod => a % b,
        MathOp::Floor => a.floor(),
        MathOp::Ceil => a.ceil(),
        MathOp::Trunc => a.trunc(),
        MathOp::Fract => a - a.trunc(),
        MathOp::InvFract => 1.0 - (a - a.trunc()),
    }
}

#[derive(Clone, Copy)]
pub struct Values<'a> {
    pub def: &'a Definition,
    pub params: &'a Params,
    pub ctx: StateCtx,
    /// Ease/spring state per value node (see `smooth`); empty reads their input.
    pub(super) smooth: &'a [Smooth],
}

impl Values<'_> {
    pub fn get(&self, id: ValueId) -> Value {
        let Some(node) = self.def.values.get(id) else { return Value::Bool(false) };
        match node {
            ValueNode::Param(i) => self.params.get(*i).cloned().unwrap_or(Value::Bool(false)),
            ValueNode::Const(v) => v.clone(),
            ValueNode::Not(v) => Value::Bool(!self.bool(*v)),
            ValueNode::And(vs) => Value::Bool(vs.iter().all(|v| self.bool(*v))),
            ValueNode::Or(vs) => Value::Bool(vs.iter().any(|v| self.bool(*v))),
            ValueNode::FloatCompare { value, cmp, rhs, epsilon } => Value::Bool(compare(self.float(*value), *cmp, self.operand(*rhs), *epsilon)),
            ValueNode::IdMatches { id, ids, negate } => {
                let v = match self.get(*id) {
                    Value::Id(s) => s,
                    _ => String::new(),
                };
                Value::Bool(ids.iter().any(|i| i.eq_ignore_ascii_case(&v)) != *negate)
            }
            ValueNode::FloatMath { a, b, op, abs, negate } => {
                let r = math(self.float(*a), self.operand(*b), *op);
                let r = if *abs { r.abs() } else { r };
                Value::Float(if *negate { -r } else { r })
            }
            ValueNode::FloatClamp { value, min, max } => Value::Float(self.float(*value).clamp(min.min(*max), max.max(*min))),
            ValueNode::FloatRemap { value, from, to } => {
                let span = from.1 - from.0;
                let t = if span == 0.0 { 0.0 } else { ((self.float(*value) - from.0) / span).clamp(0.0, 1.0) };
                Value::Float(to.0 + (to.1 - to.0) * t)
            }
            ValueNode::Select { cond, yes, no } => Value::Float(self.operand(if self.bool(*cond) { *yes } else { *no })),
            ValueNode::TimeInState { cmp, seconds } => Value::Bool(compare(self.ctx.elapsed, *cmp, *seconds, 0.0)),
            ValueNode::StateCompleted => Value::Bool(self.ctx.completed),
            ValueNode::Ease { input, .. } | ValueNode::Spring { input, .. } => match self.smooth.get(id) {
                Some(s) => Value::Float(s.value),
                None => Value::Float(self.float(*input)),
            },
        }
    }

    pub fn bool(&self, id: ValueId) -> bool {
        self.get(id).as_bool()
    }

    pub fn float(&self, id: ValueId) -> f32 {
        self.get(id).as_float()
    }

    fn operand(&self, o: Operand) -> f32 {
        match o {
            Operand::Node(id) => self.float(id),
            Operand::Const(c) => c,
        }
    }
}
