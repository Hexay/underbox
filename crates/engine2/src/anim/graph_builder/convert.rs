//! Builder snapshot → `anim::graph::Definition`: parameters and value nodes (poses are in
//! `convert_pose`). Native node indices are one space; the definition keeps values and poses
//! apart, so each converted node is memoized per index in its own map.
//!
//! Real: const bool/float/ID/vector, control and virtual parameters, cached values (as their
//! source), not/and/or, float comparison and range comparison, ID comparison, float math, clamp,
//! remap, switch, float selector and ID→float (as nested selects), time-in-state/elapsed time
//! conditions, state completed. Everything else is a neutral `false`/0/empty-ID stand-in, listed
//! in `unsupported`.

use super::snapshot::{Node, Snapshot};
use anim::graph::{Cmp, Definition, MathOp, Operand, PoseId, ValueId, ValueNode};
use anim::params::{ParamDef, Target, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

pub struct Converted {
    pub def: Definition,
    /// Node classes compiled to a stand-in (or converted without some of their behavior).
    pub unsupported: Vec<String>,
}

pub(super) struct Conv<'a> {
    pub s: &'a Snapshot,
    pub def: Definition,
    pub values: HashMap<usize, ValueId>,
    pub poses: HashMap<usize, PoseId>,
    /// Pose nodes being converted, to cut reference cycles.
    pub visiting: HashSet<usize>,
    params: HashMap<usize, usize>,
    /// A constant `false`: what an unset or unsupported value input reads.
    neutral: ValueId,
    pub unsupported: BTreeSet<String>,
}

pub fn convert(s: &Snapshot) -> Converted {
    let mut def = Definition { model: s.model.clone(), variation: s.variation.clone(), slots: s.meta.animations.clone(), ..Definition::default() };
    let neutral = def.add_value(ValueNode::Const(Value::Bool(false)));
    let mut c = Conv {
        s,
        def,
        neutral,
        values: HashMap::new(),
        poses: HashMap::new(),
        visiting: HashSet::new(),
        params: HashMap::new(),
        unsupported: BTreeSet::new(),
    };
    c.params();
    let root = c.node(s.root).is_some().then(|| c.pose(s.root as i64));
    c.def.root = root;
    Converted { def: c.def, unsupported: c.unsupported.into_iter().collect() }
}

fn short(class: &str) -> &str {
    class.strip_suffix("_Definition").unwrap_or(class)
}

impl<'a> Conv<'a> {
    pub fn node(&self, i: impl TryInto<usize>) -> Option<&'a Node> {
        self.s.nodes.get(i.try_into().ok()?)
    }

    pub fn stand_in(&mut self, n: &Node, why: &str) {
        self.unsupported.insert(if why.is_empty() { short(&n.class).to_owned() } else { format!("{} ({why})", short(&n.class)) });
    }

    /// The name an interned ID hash came from (hex if the builder never interned it).
    pub fn id_name(&self, id: i64) -> String {
        let id = id as u32;
        self.s.meta.ids.get(&id).cloned().unwrap_or_else(|| format!("{id:08x}"))
    }

    /// Control-parameter nodes in node order pair with `meta.control_params`; descriptions by name.
    fn params(&mut self) {
        let control = self.s.nodes.iter().enumerate().filter(|(_, n)| n.class.starts_with("CControlParameter"));
        for (ordinal, (index, n)) in control.enumerate() {
            let name = self.s.meta.control_params.get(ordinal).map_or_else(|| format!("param{ordinal}"), |&id| self.id_name(id as i64));
            let default = match n.class.as_str() {
                "CControlParameterBoolNode_Definition" => Value::Bool(n.bool("m_defaultValue")),
                "CControlParameterFloatNode_Definition" => Value::Float(n.float("m_defaultValue")),
                "CControlParameterIDNode_Definition" => {
                    let id = n.setters.get("DefaultValue").map(|&id| self.id_name(id)).unwrap_or_default();
                    Value::Id(id)
                }
                "CControlParameterVectorNode_Definition" => {
                    let v: Vec<f32> = n.field("m_defaultValue").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                    Value::Vector(glam::Vec3::new(v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0)))
                }
                _ => Value::Target(Target::default()),
            };
            let mut p = ParamDef::new(&name, default);
            if let Some(d) = self.s.meta.params.iter().find(|d| d.name.eq_ignore_ascii_case(&name)) {
                (p.min, p.max, p.options) = (d.min, d.max, d.options.clone());
            }
            self.params.insert(index, self.def.params.len());
            self.def.params.push(p);
        }
    }

    fn konst(&mut self, v: Value) -> ValueId {
        self.def.add_value(ValueNode::Const(v))
    }

    /// The value node at native index `i`; a missing or unset (-1) input reads as `false`/0.
    pub fn value(&mut self, i: i64) -> ValueId {
        let Some(n) = self.node(i) else { return self.neutral };
        let i = i as usize;
        if let Some(&v) = self.values.get(&i) {
            return v;
        }
        // Mark it first so a malformed cycle reads the neutral value instead of recursing forever.
        self.values.insert(i, self.neutral);
        let v = self.value_node(i, n).unwrap_or_else(|| {
            self.stand_in(n, "");
            self.neutral
        });
        self.values.insert(i, v);
        v
    }

    /// A float input that's either node `i` or, when unset, the constant `c`.
    pub fn operand(&mut self, i: i64, c: f32) -> Operand {
        if i >= 0 { Operand::Node(self.value(i)) } else { Operand::Const(c) }
    }

    fn values_of(&mut self, list: &[i64]) -> Vec<ValueId> {
        list.iter().map(|&i| self.value(i)).collect()
    }

    /// `cases` checked in order, each giving its constant; `default` when none holds.
    fn select_chain(&mut self, cases: Vec<(ValueId, f32)>, default: f32) -> ValueId {
        let mut acc = Operand::Const(default);
        for (cond, yes) in cases.into_iter().rev() {
            acc = Operand::Node(self.def.add_value(ValueNode::Select { cond, yes: Operand::Const(yes), no: acc }));
        }
        match acc {
            Operand::Node(v) => v,
            Operand::Const(c) => self.konst(Value::Float(c)),
        }
    }

    fn value_node(&mut self, i: usize, n: &'a Node) -> Option<ValueId> {
        let class = n.class.strip_suffix("_Definition").unwrap_or(&n.class);
        let input = n.idx("m_inputValueNodeIdx");
        let node = match class {
            "CConstBoolNode" => ValueNode::Const(Value::Bool(n.bool("m_value"))),
            "CConstFloatNode" => ValueNode::Const(Value::Float(n.float("m_value"))),
            "CConstIDNode" => ValueNode::Const(Value::Id(self.id_name(n.setter("Value")))),
            c if c.starts_with("CControlParameter") => ValueNode::Param(*self.params.get(&i)?),
            c if c.starts_with("CVirtualParameter") => return Some(self.value(n.idx("m_childNodeIdx"))),
            c if c.starts_with("CCached") => return Some(self.value(n.idx("m_sourceNodeIdx"))),
            "CNotNode" => ValueNode::Not(self.value(input)),
            "CAndNode" => ValueNode::And(self.values_of(n.ints("ConditionNodeIndices"))),
            "COrNode" => ValueNode::Or(self.values_of(n.ints("ConditionNodeIndices"))),
            "CFloatComparisonNode" => {
                let cmp = [Cmp::Ge, Cmp::Le, Cmp::NearEq, Cmp::Gt, Cmp::Lt].get(n.setter("Comparison") as usize).copied()?;
                let value = self.value(input);
                let rhs = self.operand(n.idx("m_comparandValueNodeIdx"), n.float("m_comparisonValue"));
                ValueNode::FloatCompare { value, cmp, rhs, epsilon: n.float("m_epsilon") }
            }
            "CFloatRangeComparisonNode" => {
                let value = self.value(input);
                let (lo, hi) = if n.bool("m_isInclusiveCheck") { (Cmp::Ge, Cmp::Le) } else { (Cmp::Gt, Cmp::Lt) };
                let a = self.def.add_value(ValueNode::FloatCompare { value, cmp: lo, rhs: Operand::Const(n.float("m_rangeMin")), epsilon: 0.0 });
                let b = self.def.add_value(ValueNode::FloatCompare { value, cmp: hi, rhs: Operand::Const(n.float("m_rangeMax")), epsilon: 0.0 });
                ValueNode::And(vec![a, b])
            }
            "CIDComparisonNode" => {
                let ids = n.ints("ComparisionIDs").iter().map(|&id| self.id_name(id)).collect();
                ValueNode::IdMatches { id: self.value(input), ids, negate: n.setter("Comparison") == 1 }
            }
            "CFloatMathNode" => self.float_math(n)?,
            "CFloatClampNode" => ValueNode::FloatClamp { value: self.value(input), min: n.float("m_clampMin"), max: n.float("m_clampMax") },
            "CFloatRemapNode" => ValueNode::FloatRemap {
                value: self.value(input),
                from: (n.float("m_inputRangeMin"), n.float("m_inputRangeMax")),
                to: (n.float("m_outputRangeMin"), n.float("m_outputRangeMax")),
            },
            "CFloatSwitchNode" => ValueNode::Select {
                cond: self.value(n.idx("m_switchValueNodeIdx")),
                yes: self.operand(n.idx("m_trueValueNodeIdx"), n.float("m_trueValue")),
                no: self.operand(n.idx("m_falseValueNodeIdx"), n.float("m_falseValue")),
            },
            "CFloatSelectorNode" => {
                let conds = self.values_of(n.ints("ConditionNodeIndices"));
                return Some(self.select_chain(conds.into_iter().zip(n.floats("Values").iter().copied()).collect(), n.float("m_defaultValue")));
            }
            "CIDToFloatNode" => {
                let id = self.value(input);
                let cases = n.ints("IDs").iter().zip(n.floats("Values")).map(|(&h, &v)| (self.id_name(h), v)).collect::<Vec<_>>();
                let cases = cases.into_iter().map(|(name, v)| (self.def.add_value(ValueNode::IdMatches { id, ids: vec![name], negate: false }), v)).collect();
                return Some(self.select_chain(cases, n.float("m_defaultValue")));
            }
            "CTimeConditionNode" => self.time_condition(n)?,
            "CStateCompletedConditionNode" => ValueNode::StateCompleted,
            _ => return None,
        };
        Some(self.def.add_value(node))
    }

    fn float_math(&mut self, n: &'a Node) -> Option<ValueNode> {
        const OPS: [MathOp; 12] = [
            MathOp::Add,
            MathOp::Sub,
            MathOp::Mul,
            MathOp::Div,
            MathOp::Mod,
            MathOp::Add,
            MathOp::Add,
            MathOp::Floor,
            MathOp::Ceil,
            MathOp::Trunc,
            MathOp::Fract,
            MathOp::InvFract,
        ];
        let which = n.setter("Operator");
        let op = *OPS.get(usize::try_from(which).ok()?)?;
        let a = self.value(n.idx("m_inputValueNodeIdxA"));
        // Abs and Negate are Add-zero with the result flag set.
        let unary_flag = matches!(which, 5 | 6);
        let b = if unary_flag { Operand::Const(0.0) } else { self.operand(n.idx("m_inputValueNodeIdxB"), n.float("m_valueB")) };
        let abs = n.bool("m_returnAbsoluteResult") || which == 5;
        let negate = n.bool("m_returnNegatedResult") || which == 6;
        Some(ValueNode::FloatMath { a, b, op, abs, negate })
    }

    fn time_condition(&mut self, n: &'a Node) -> Option<ValueNode> {
        // TimeComparisonType: PercentageThroughState, PercentageThroughSyncEvent, ElapsedTime, TimeInState.
        if !matches!(n.setter("Type"), 2 | 3) {
            return None;
        }
        if n.idx("m_inputValueNodeIdx") >= 0 {
            self.stand_in(n, "input value ignored");
        }
        let cmp = [Cmp::Lt, Cmp::Le, Cmp::Gt, Cmp::Ge].get(n.setter("Operator") as usize).copied()?;
        Some(ValueNode::TimeInState { cmp, seconds: n.float("m_comparand") })
    }
}
