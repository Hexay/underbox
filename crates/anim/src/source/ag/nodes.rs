//! Per-class compilation of `.ag` nodes into graph value and pose nodes.

use super::{Compiler, Scope, f32_of, link, str_of};
use crate::graph::{Cmp, MathOp, Operand, PoseId, PoseNode, State, Transition, ValueId, ValueNode};
use crate::params::Value;
use serde_json::Value as Json;

/// Pose nodes we don't evaluate yet but can pass one input through (class, input field).
const PASS_THROUGH: &[(&str, &str)] = &[
    ("LayerBlendNode", "Base"),
    ("StateMachineLayerNode", "StateMachine"),
    ("LocalLayerNode", "Input"),
    ("RootMotionOverrideNode", "Input"),
    ("LookAtNode", "Input"),
    ("TwoBoneIKNode", "Input"),
];

/// Value nodes we don't evaluate yet but can pass one input through.
const VALUE_THROUGH: &[(&str, &str)] = &[("FloatSpringNode", "Value"), ("FloatEaseNode", "Value")];

fn cmp(name: &str) -> Option<Cmp> {
    Some(match name {
        "Equal" | "Equals" => Cmp::Eq,
        "NearEqual" => Cmp::NearEq,
        "NotEqual" => Cmp::Ne,
        "LessThan" => Cmp::Lt,
        "LessThanEqual" => Cmp::Le,
        "GreaterThan" => Cmp::Gt,
        "GreaterThanEqual" => Cmp::Ge,
        _ => return None,
    })
}

fn math(name: &str) -> Option<MathOp> {
    Some(match name {
        "Add" => MathOp::Add,
        "Subtract" => MathOp::Sub,
        "Multiply" => MathOp::Mul,
        "Divide" => MathOp::Div,
        "Min" => MathOp::Min,
        "Max" => MathOp::Max,
        _ => return None,
    })
}

impl Compiler {
    fn konst(&mut self, v: Value) -> ValueId {
        self.def.add_value(ValueNode::Const(v))
    }

    fn unsupported(&mut self, class: &str) {
        self.unsupported.insert(class.to_owned());
    }

    /// A value input, or `default` when it's unlinked.
    pub(super) fn value_input(&mut self, l: Option<&Json>, scope: &Scope, default: Value) -> ValueId {
        let Some((node, id)) = link(scope, l) else { return self.konst(default) };
        if let Some(&v) = self.values.get(&id) {
            return v;
        }
        let v = self.value_node(node, scope);
        self.values.insert(id, v);
        v
    }

    /// A linked input as an operand, else the constant `field` of `node`.
    fn operand(&mut self, node: &Json, input: &str, field: &str, scope: &Scope) -> Operand {
        match link(scope, node.get(input)) {
            Some(_) => Operand::Node(self.value_input(node.get(input), scope, Value::Float(0.0))),
            None => Operand::Const(f32_of(node, field, 0.0)),
        }
    }

    fn value_node(&mut self, n: &Json, scope: &Scope) -> ValueId {
        let class = str_of(n, "_class");
        let input = |c: &mut Self, field: &str| c.value_input(n.get(field), scope, Value::Float(0.0));
        let node = match class {
            c if c.ends_with("ParameterNode") => match self.def.param(str_of(n, "ParameterName")) {
                Some(p) => ValueNode::Param(p),
                None => {
                    self.unsupported(&format!("{c} ({})", str_of(n, "ParameterName")));
                    ValueNode::Const(Value::Float(0.0))
                }
            },
            "ConstFloatNode" => ValueNode::Const(Value::Float(f32_of(n, "Value", 0.0))),
            "ConstBoolNode" => ValueNode::Const(Value::Bool(n.get("Value").and_then(Json::as_bool).unwrap_or(false))),
            "NotNode" => ValueNode::Not(input(self, "Value")),
            "OrNode" => ValueNode::Or(vec![input(self, "A"), input(self, "B")]),
            "AndNode" => {
                let conds = n.get("Conditions").and_then(Json::as_array).cloned().unwrap_or_default();
                ValueNode::And(conds.iter().map(|c| self.value_input(c.get("Input"), scope, Value::Bool(true))).collect())
            }
            "FloatComparisonNode" => ValueNode::FloatCompare {
                value: input(self, "Value"),
                cmp: cmp(str_of(n, "Comparison")).unwrap_or(Cmp::Eq),
                rhs: self.operand(n, "Comparand", "ComparisonValue", scope),
                epsilon: f32_of(n, "Epsilon", 0.0),
            },
            "IDComparisonNode" => ValueNode::IdMatches {
                id: input(self, "Id"),
                ids: n.get("Ids").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(str::to_owned).collect()).unwrap_or_default(),
                negate: str_of(n, "Comparison") != "Matches",
            },
            "FloatMathNode" => ValueNode::FloatMath {
                a: input(self, "A"),
                b: self.operand(n, "B", "ValueB", scope),
                op: math(str_of(n, "Operator")).unwrap_or(MathOp::Add),
                abs: n.get("AbsoluteResult").and_then(Json::as_bool).unwrap_or(false),
                negate: n.get("NegateResult").and_then(Json::as_bool).unwrap_or(false),
            },
            "FloatClampNode" => ValueNode::FloatClamp { value: input(self, "Value"), min: f32_of(n, "Min", 0.0), max: f32_of(n, "Max", 1.0) },
            "FloatRemapNode" => ValueNode::FloatRemap {
                value: input(self, "Value"),
                from: (f32_of(n, "InputMin", 0.0), f32_of(n, "InputMax", 1.0)),
                to: (f32_of(n, "OutputMin", 0.0), f32_of(n, "OutputMax", 1.0)),
            },
            "FloatSwitchNode" => ValueNode::Select {
                cond: self.value_input(n.get("Condition"), scope, Value::Bool(false)),
                yes: self.operand(n, "TrueInput", "TrueValue", scope),
                no: self.operand(n, "FalseInput", "FalseValue", scope),
            },
            "TimeConditionNode" if matches!(str_of(n, "Type"), "TimeInState" | "ElapsedTime") => {
                ValueNode::TimeInState { cmp: cmp(str_of(n, "Operator")).unwrap_or(Cmp::Ge), seconds: f32_of(n, "Value", 0.0) }
            }
            "StateCompletedConditionNode" => ValueNode::StateCompleted,
            c => match VALUE_THROUGH.iter().find(|(k, _)| *k == c) {
                Some((_, field)) => {
                    self.unsupported(c);
                    return input(self, field);
                }
                None => {
                    self.unsupported(c);
                    ValueNode::Const(Value::Bool(false))
                }
            },
        };
        self.def.add_value(node)
    }

    /// A pose input; unlinked ones are the reference pose.
    pub(super) fn pose_input(&mut self, l: Option<&Json>, scope: &Scope) -> PoseId {
        let Some((node, id)) = link(scope, l) else { return self.def.add_pose(PoseNode::Reference) };
        if let Some(&p) = self.poses.get(&id) {
            return p;
        }
        let p = self.pose_node(node, scope);
        self.poses.insert(id, p);
        p
    }

    /// The result of a nested graph (`{ "nodes": [...] }`) seen from `parent`.
    fn graph_result(&mut self, graph: Option<&Json>, parent: &Scope) -> PoseId {
        let Some(graph) = graph.filter(|g| g.is_object()) else { return self.def.add_pose(PoseNode::Reference) };
        let scope = Scope::new(graph, Some(parent));
        let result = scope.result().and_then(|r| r.get("Pose"));
        self.pose_input(result, &scope)
    }

    fn pose_node(&mut self, n: &Json, scope: &Scope) -> PoseId {
        let class = str_of(n, "_class");
        let node = match class {
            "ZeroPoseNode" => PoseNode::Zero,
            "ReferencePoseNode" => PoseNode::Reference,
            "AnimationClipNode" | "AnimationPoseNode" => {
                let data = n.get("DefaultVariationData");
                let slot = self.def.slot(data.map(|d| str_of(d, "AnimationName")).unwrap_or_default());
                let speed = if class == "AnimationPoseNode" { 0.0 } else { data.map_or(1.0, |d| f32_of(d, "SpeedMultiplier", 1.0)) };
                PoseNode::Clip { slot, looping: n.get("Loop").and_then(Json::as_bool).unwrap_or(false), speed }
            }
            "SpeedScaleNode" => {
                let speed = self.value_input(n.get("Speed"), scope, Value::Float(f32_of(n, "DefaultSpeed", 1.0)));
                PoseNode::SpeedScale { source: self.pose_input(n.get("Source"), scope), speed }
            }
            "Blend1DNode" => {
                let param = self.value_input(n.get("Parameter"), scope, Value::Float(0.0));
                let points = n.get("Points").and_then(Json::as_array).cloned().unwrap_or_default();
                let mut points: Vec<_> = points.iter().map(|p| (f32_of(p, "Value", 0.0), self.graph_result(p.get("Graph"), scope))).collect();
                points.sort_by(|a, b| a.0.total_cmp(&b.0));
                PoseNode::Blend1D { param, points }
            }
            "PoseGroupNode" => return self.graph_result(n.get("Contents"), scope),
            "GroupInputNode" => return self.pose_input(n.get("Source"), scope),
            "StateMachineNode" => self.state_machine(n, scope),
            c => {
                self.unsupported(c);
                match PASS_THROUGH.iter().find(|(k, _)| *k == c) {
                    Some((_, field)) => return self.pose_input(n.get(*field), scope),
                    None => PoseNode::Reference,
                }
            }
        };
        self.def.add_pose(node)
    }

    fn state_machine(&mut self, n: &Json, scope: &Scope) -> PoseNode {
        let list = n.get("States").and_then(Json::as_array).cloned().unwrap_or_default();
        let index = |name: &str| list.iter().position(|s| str_of(s, "Name") == name);
        let mut states: Vec<State> =
            list.iter().map(|s| State { name: str_of(s, "Name").to_owned(), pose: self.graph_result(s.get("Graph"), scope), transitions: Vec::new() }).collect();
        for conduit in n.get("Conduits").and_then(Json::as_array).cloned().unwrap_or_default() {
            let (Some(from), Some(to)) = (index(str_of(&conduit, "From")), index(str_of(&conduit, "To"))) else { continue };
            let Some(graph) = conduit.get("Graph") else { continue };
            let inner = Scope::new(graph, Some(scope));
            for t in inner.list.iter().filter(|t| str_of(t, "_class") == "TransitionNode") {
                let condition = self.value_input(t.get("Condition"), &inner, Value::Bool(false));
                states[from].transitions.push(Transition { to, condition, duration: f32_of(t, "Duration", 0.0) });
            }
        }
        if let Some(global) = n.get("GlobalGraph") {
            let inner = Scope::new(global, Some(scope));
            for t in inner.list.iter().filter(|t| str_of(t, "_class") == "GlobalTransitionNode") {
                let Some(to) = index(str_of(t, "StateName")) else { continue };
                if link(&inner, t.get("Condition")).is_none() {
                    continue;
                }
                let condition = self.value_input(t.get("Condition"), &inner, Value::Bool(false));
                let duration = f32_of(t, "Duration", 0.0);
                states.iter_mut().enumerate().filter(|(i, _)| *i != to).for_each(|(_, s)| s.transitions.push(Transition { to, condition, duration }));
            }
        }
        let default = n.get("DefaultStateIndex").and_then(Json::as_u64).map(|i| i as usize).or_else(|| index(str_of(n, "DefaultState"))).unwrap_or(0);
        PoseNode::StateMachine { states, default }
    }
}
