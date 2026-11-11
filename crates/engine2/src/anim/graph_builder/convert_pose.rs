//! Pose nodes of a builder snapshot (see `convert` for values).
//!
//! Real: clip, zero and reference pose, blend1D (from its blend ranges), state machine (states,
//! transitions with their transition node's duration, default state), speed scale, layer blend
//! (overlay layers as weighted blends), passthrough, and `convert_more`'s. Stand-ins: other
//! effect nodes with a child (warps, scale, root-motion override, duration scale...) pass their
//! child through; other multi-input nodes play their first option; the rest is the reference pose.

use super::convert::Conv;
use super::snapshot::Node;
use anim::graph::{PoseId, PoseNode, State, StateEvents, Transition, ValueNode};
use anim::params::Value;

impl<'a> Conv<'a> {
    /// The pose node at native index `i`; missing, unset or cyclic inputs are the reference pose.
    pub fn pose(&mut self, i: i64) -> PoseId {
        let Some(n) = self.node(i) else { return self.def.add_pose(PoseNode::Reference) };
        let i = i as usize;
        if let Some(&p) = self.poses.get(&i) {
            return p;
        }
        if !self.visiting.insert(i) {
            return self.def.add_pose(PoseNode::Reference);
        }
        let p = self.pose_node(n);
        self.visiting.remove(&i);
        self.poses.insert(i, p);
        p
    }

    fn pose_node(&mut self, n: &'a Node) -> PoseId {
        let class = n.class.strip_suffix("_Definition").unwrap_or(&n.class);
        let child = n.idx("m_childNodeIdx");
        let node = match class {
            "CZeroPoseNode" => PoseNode::Zero,
            "CReferencePoseNode" => PoseNode::Reference,
            "CClipNode" => {
                let speed = n.field("m_speedMultiplier").and_then(|v| v.as_f64()).map_or(1.0, |s| s as f32);
                let slot = usize::try_from(n.idx("m_dataSlotIdx")).unwrap_or(usize::MAX);
                PoseNode::Clip { slot, looping: n.bool("m_allowLooping"), speed }
            }
            "CBlend1DNode" => self.blend1d(n),
            "CStateMachineNode" => self.state_machine(n),
            "CStateNode" | "CPassthroughNode" => return self.state_child(child),
            "CSpeedScaleNode" => PoseNode::SpeedScale { source: self.pose(child), speed: self.value(n.idx("m_inputValueNodeIdx")) },
            "CLayerBlendNode" => return self.layer_blend(n),
            c => return self.pose_more(c, n).unwrap_or_else(|| self.stand_in_pose(n)),
        };
        self.def.add_pose(node)
    }

    /// A state's (or passthrough's) child; an off state (-1) holds the reference pose.
    fn state_child(&mut self, child: i64) -> PoseId {
        if child < 0 { self.def.add_pose(PoseNode::Reference) } else { self.pose(child) }
    }

    fn blend1d(&mut self, n: &'a Node) -> PoseNode {
        let param = self.value(n.idx("m_parameterValueNodeIdx"));
        let sources = n.ints("SourceNodeIndices");
        let mut points: Vec<(f32, i64)> = Vec::new();
        for r in n.items("BlendRanges") {
            for (input, x) in [(r.idx("m_inputIdx0"), r.float("m_paramRangeStart")), (r.idx("m_inputIdx1"), r.float("m_paramRangeEnd"))] {
                if let Some(&src) = usize::try_from(input).ok().and_then(|k| sources.get(k)) {
                    points.push((x, src));
                }
            }
        }
        if points.is_empty() {
            points = sources.iter().enumerate().map(|(k, &src)| (k as f32, src)).collect();
        }
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        points.dedup();
        let points = points.into_iter().map(|(x, src)| (x, self.pose(src))).collect();
        PoseNode::Blend1D { param, points }
    }

    fn state_machine(&mut self, n: &'a Node) -> PoseNode {
        let mut states = Vec::new();
        for (k, s) in n.items("StateDefinition").iter().enumerate() {
            let state = self.node(s.idx("m_stateNodeIdx"));
            let events = state.filter(|st| st.class == "CStateNode_Definition").map(|st| self.state_events(st)).unwrap_or_default();
            let pose = match state {
                Some(st) if st.class == "CStateNode_Definition" => self.state_child(st.idx("m_childNodeIdx")),
                _ => self.pose(s.idx("m_stateNodeIdx")),
            };
            if s.idx("m_entryConditionNodeIdx") >= 0 {
                self.stand_in(n, "entry conditions ignored");
            }
            let transitions = s
                .items("TransitionDefinition")
                .iter()
                .filter_map(|t| {
                    let to = usize::try_from(t.idx("m_targetStateIdx")).ok()?;
                    let condition = self.value(t.idx("m_conditionNodeIdx"));
                    let duration = self.node(t.idx("m_transitionNodeIdx")).map_or(0.0, |tn| tn.float("m_duration").max(0.0));
                    Some(Transition { to, condition, duration })
                })
                .collect();
            states.push(State { name: format!("state{k}"), pose, transitions, events });
        }
        let default = usize::try_from(n.idx("m_defaultStateIndex")).unwrap_or(0);
        PoseNode::StateMachine { states, default }
    }

    /// `AddState`'s event IDs; timed events keep `ComparisonOperator` (0: ≤, 1: ≥).
    fn state_events(&self, st: &Node) -> StateEvents {
        let ids = |k: &str| st.ints(k).iter().map(|&id| self.id_name(id)).collect();
        let elapsed = st.items("TimedElapsedEvents").iter().map(|e| (self.id_name(e.setter("ID")), e.float("m_timeValue"), e.setter("ComparisonOperator") == 0)).collect();
        StateEvents { entry: ids("EntryEvents"), exit: ids("ExitEvents"), fully_in: ids("FullyInStateEvents"), elapsed }
    }

    /// Base, then each overlay layer blended on by its weight (unset: full weight).
    fn layer_blend(&mut self, n: &'a Node) -> PoseId {
        let mut acc = self.pose(n.idx("m_baseNodeIdx"));
        for l in n.items("LayerDefinitions") {
            if l.setter("BlendMode") != 0 || l.idx("m_boneMaskValueNodeIdx") >= 0 {
                self.stand_in(n, "only full-body overlay layers");
            }
            let layer = self.pose(l.idx("m_inputNodeIdx"));
            let w = l.idx("m_weightValueNodeIdx");
            let weight = if w >= 0 { self.value(w) } else { self.def.add_value(ValueNode::Const(Value::Float(1.0))) };
            acc = self.def.add_pose(PoseNode::Blend { base: acc, layer, weight });
        }
        acc
    }

    fn stand_in_pose(&mut self, n: &'a Node) -> PoseId {
        self.stand_in(n, "");
        let first = ["OptionNodeIndices", "SourceNodeIndices"].iter().find_map(|k| n.ints(k).first().copied());
        let input = first.or_else(|| ["m_childNodeIdx", "m_clipReferenceNodeIdx", "m_fallbackNodeIdx"].iter().map(|f| n.idx(f)).find(|&i| i >= 0));
        match input {
            Some(i) => self.pose(i),
            None => self.def.add_pose(PoseNode::Reference),
        }
    }
}
