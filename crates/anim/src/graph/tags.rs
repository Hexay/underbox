//! Graph tags: the IDs a state's events raise (GraphBuilder `AddState`'s entry, exit and
//! fully-in-state events, and elapsed-time events), which managed reads as `IsTagActive` and
//! `DispatchTagEvents`. Each update recomputes the active set from the playing states:
//! - entry events while transitioning in, and on the update a state is entered;
//! - exit events while transitioning out, and on the update an instant transition leaves;
//! - fully-in-state events while the state plays with no transition running;
//! - elapsed events while time in state is at most (or at least) their time;
//! - remaining events likewise against the time left in the state's playing duration: its
//!   dominant clip's seconds to the end of the current loop, scaled by speed (Esoterica's
//!   `(1 - percentage through) * duration`); states without a clip have 0 left.

use super::eval::Instance;
use super::State;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateEvents {
    pub entry: Vec<String>,
    pub exit: Vec<String>,
    pub fully_in: Vec<String>,
    /// (id, seconds, at most): active while time in state is ≤ (else ≥) the seconds.
    pub elapsed: Vec<(String, f32, bool)>,
    /// (id, seconds, at most): as `elapsed`, against the seconds left in the state.
    pub remaining: Vec<(String, f32, bool)>,
}

/// What one machine's update did, for its tags.
pub(super) struct MachineStep {
    pub current: usize,
    pub elapsed: f32,
    pub remaining: f32,
    /// The state blending out, if a transition is running.
    pub blending_from: Option<usize>,
    /// `current` was entered this update.
    pub entered: bool,
    /// The state an instant transition left this update.
    pub left: Option<usize>,
}

impl Instance {
    pub(super) fn raise_state_tags(&mut self, states: &[State], step: &MachineStep) {
        let Some(cur) = states.get(step.current).map(|s| &s.events) else { return };
        let mut raise = |ids: &[String]| self.tags.extend(ids.iter().cloned());
        match step.blending_from.and_then(|f| states.get(f)) {
            Some(from) => {
                raise(&cur.entry);
                raise(&from.events.exit);
            }
            None if step.entered => raise(&cur.entry),
            None => raise(&cur.fully_in),
        }
        if let Some(left) = step.left.and_then(|l| states.get(l)) {
            raise(&left.events.exit);
        }
        let timed = |events: &[(String, f32, bool)], now: f32| {
            let hit = move |(_, t, at_most): &&(String, f32, bool)| if *at_most { now <= *t } else { now >= *t };
            events.iter().filter(hit).map(|(id, ..)| id.clone()).collect::<Vec<_>>()
        };
        self.tags.extend(timed(&cur.elapsed, step.elapsed));
        self.tags.extend(timed(&cur.remaining, step.remaining));
    }

    /// The tags the last update raised, deduplicated.
    pub fn active_tags(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in &self.tags {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
                out.push(t.clone());
            }
        }
        out
    }

    pub fn is_tag_active(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
    }
}

/// managed's `SceneModel.AnimTagStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum TagStatus {
    /// Started and ended between two dispatches.
    Fired = 0,
    Start = 1,
    End = 2,
}

/// Tag changes between dispatches: each update's active set is compared with the last one.
#[derive(Debug, Clone, Default)]
pub struct TagTracker {
    active: Vec<String>,
    log: Vec<(String, TagStatus)>,
}

/// Bounds the log for objects whose events managed never dispatches.
const MAX_LOG: usize = 256;

impl TagTracker {
    pub fn observe(&mut self, now: &[String]) {
        let has = |set: &[String], t: &str| set.iter().any(|s| s.eq_ignore_ascii_case(t));
        let ended = self.active.iter().filter(|t| !has(now, t)).map(|t| (t.clone(), TagStatus::End));
        let started = now.iter().filter(|t| !has(&self.active, t)).map(|t| (t.clone(), TagStatus::Start));
        let changes: Vec<_> = ended.chain(started).collect();
        self.log.extend(changes);
        self.active = now.to_vec();
        let over = self.log.len().saturating_sub(MAX_LOG);
        self.log.drain(..over);
    }

    /// The changes since the last call, a start followed directly by its end becoming `Fired`.
    pub fn drain(&mut self) -> Vec<(String, TagStatus)> {
        let mut out: Vec<(String, TagStatus)> = Vec::new();
        for (tag, status) in std::mem::take(&mut self.log) {
            let start = out.iter().rposition(|(t, _)| t.eq_ignore_ascii_case(&tag));
            match (status, start) {
                (TagStatus::End, Some(i)) if out[i].1 == TagStatus::Start => out[i].1 = TagStatus::Fired,
                _ => out.push((tag, status)),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Definition, PoseNode, Transition, ValueNode};
    use crate::params::{ParamDef, Value};
    use crate::pose::Pose;

    fn tags(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A (fully-in `a_full`, exit `a_exit`, `early` while ≤ 0.15 s in) → B (entry `b_in`) over 0.2 s.
    #[test]
    fn states_raise_entry_exit_fully_in_and_elapsed_tags() {
        let mut def = Definition::default();
        def.params.push(ParamDef::new("go", Value::Bool(false)));
        let go = def.add_value(ValueNode::Param(0));
        let pose = def.add_pose(PoseNode::Reference);
        let a = StateEvents { exit: tags(&["a_exit"]), fully_in: tags(&["a_full"]), elapsed: vec![("early".into(), 0.15, true)], ..Default::default() };
        let b = StateEvents { entry: tags(&["b_in"]), ..Default::default() };
        let states = vec![
            State { name: "A".into(), pose, transitions: vec![Transition { to: 1, condition: go, duration: 0.2 }], events: a },
            State { name: "B".into(), pose, transitions: vec![], events: b },
        ];
        def.root = Some(def.add_pose(PoseNode::StateMachine { states, default: 0 }));
        let mut g = Instance::new(def.into_shared(), |_| None);
        let step = |g: &mut Instance| {
            g.update(0.1, &Pose(Vec::new()));
            let mut t = g.active_tags();
            t.sort();
            t
        };
        assert_eq!(step(&mut g), tags(&["early"]), "entering A: its elapsed event, not yet fully in");
        assert_eq!(step(&mut g), tags(&["a_full"]), "0.2 s in: fully in, past `early`");
        g.params.set_by_name("go", Value::Bool(true));
        assert_eq!(step(&mut g), tags(&["a_exit", "b_in"]), "the transition starts");
        assert_eq!(step(&mut g), tags(&["a_exit", "b_in"]), "and runs");
        assert!(step(&mut g).is_empty(), "B fully in raises nothing");
        assert!(!g.is_tag_active("b_in"));
    }

    /// One state looping `walk` (1 s) at speed 2: 0.5 s per loop, so 0.5, 0.4, 0.3, 0.2, 0.1 s
    /// left over five 0.1 s updates (a clip starts at 0 on its first update).
    #[test]
    fn remaining_time_events_follow_the_states_clip() {
        let mut def = Definition::default();
        let slot = def.slot("walk");
        let pose = def.add_pose(PoseNode::Clip { slot, looping: true, speed: 2.0 });
        let events = StateEvents { remaining: vec![("ending".into(), 0.25, true), ("lots_left".into(), 0.35, false)], ..Default::default() };
        def.root = Some(def.add_pose(PoseNode::StateMachine { states: vec![State { name: "A".into(), pose, transitions: vec![], events }], default: 0 }));
        let mut g = Instance::new(def.into_shared(), crate::graph::tests::clips);
        let seen: Vec<Vec<String>> = (0..5)
            .map(|_| {
                g.update(0.1, &Pose::zero(1));
                g.active_tags()
            })
            .collect();
        assert_eq!(seen, [tags(&["lots_left"]), tags(&["lots_left"]), tags(&[]), tags(&["ending"]), tags(&["ending"])]);
    }

    #[test]
    fn tracker_reports_starts_ends_and_one_dispatch_spans_as_fired() {
        let mut t = TagTracker::default();
        t.observe(&tags(&["a"]));
        assert_eq!(t.drain(), [("a".into(), TagStatus::Start)]);
        t.observe(&tags(&["a", "b"]));
        t.observe(&tags(&["A"]));
        assert_eq!(t.drain(), [("b".into(), TagStatus::Fired)], "b came and went; a held (case-blind)");
        t.observe(&tags(&[]));
        assert_eq!(t.drain(), [("A".into(), TagStatus::End)], "named as last raised");
        assert!(t.drain().is_empty());
    }
}
