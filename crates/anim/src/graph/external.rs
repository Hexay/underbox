//! External graph slots (managed `GraphExternalSlots`): a `PoseNode::External` plays the graph
//! connected to its slot, as its own instance with parameters at their defaults, else its
//! fallback. A connected graph advances only while its node is evaluated.

use super::PoseId;
use super::eval::{Instance, Out};
use crate::pose::Pose;

pub struct External {
    pub(super) slot: String,
    instance: Box<Instance>,
    started: bool,
    /// Into the current loop, and left of it, as of the last evaluation.
    elapsed: f32,
    remaining: f32,
    active: bool,
}

impl External {
    pub(super) fn idle(&mut self) {
        self.active = false;
    }
}

/// `GetExternalGraphTiming`: the slot's last evaluated duration and normalised time, and whether
/// the last update evaluated it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub duration: f32,
    pub normalized: f32,
    pub active: bool,
}

impl Instance {
    pub fn has_graph_slot(&self, slot: &str) -> bool {
        self.definition().graph_slots.iter().any(|s| s == slot)
    }

    pub fn is_connected(&self, slot: &str) -> bool {
        self.externals.iter().any(|e| e.slot == slot)
    }

    /// Play `child` in `slot`; false if the graph has no such slot or it's occupied.
    pub fn connect(&mut self, slot: &str, child: Instance) -> bool {
        if !self.has_graph_slot(slot) || self.is_connected(slot) {
            return false;
        }
        let instance = Box::new(child);
        self.externals.push(External { slot: slot.to_owned(), instance, started: false, elapsed: 0.0, remaining: 0.0, active: false });
        true
    }

    /// Empty `slot`, so its node plays the fallback again; false if it was empty.
    pub fn disconnect(&mut self, slot: &str) -> bool {
        let before = self.externals.len();
        self.externals.retain(|e| e.slot != slot);
        self.externals.len() != before
    }

    pub fn external_timing(&self, slot: &str) -> Option<Timing> {
        let e = self.externals.iter().find(|e| e.slot == slot)?;
        let duration = e.elapsed + e.remaining;
        Some(Timing { duration, normalized: if duration > 0.0 { e.elapsed / duration } else { 0.0 }, active: e.active })
    }

    pub(super) fn external(&mut self, slot: &str, fallback: Option<PoseId>, dt: f32, bind: &Pose) -> Out {
        let Some(k) = self.externals.iter().position(|e| e.slot == slot) else {
            return match fallback {
                Some(f) => self.eval(f, dt, bind),
                None => Out::ended(bind.clone()),
            };
        };
        let e = &mut self.externals[k];
        let out = e.instance.advance(dt, bind);
        // A clip starts at 0 on its first evaluation; more left than before means it looped.
        let duration = e.elapsed + e.remaining;
        e.elapsed = if e.started { e.elapsed + dt } else { 0.0 };
        if e.started && out.remaining > e.remaining {
            e.elapsed = (e.elapsed - duration).max(0.0);
        }
        (e.started, e.remaining, e.active) = (true, out.remaining, true);
        let events = e.instance.take_events();
        self.events.extend(events);
        out
    }
}
