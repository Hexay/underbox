//! Touch and trigger tracking: which shape pairs touch after each step, and the begin/persist/end
//! events managed's `ProcessIntersections` drains.

use crate::filter::{Contact, Hooks, ShapeMeta, function};
use rapier3d::math::Vector;
use rapier3d::pipeline::PhysicsWorld;
use rapier3d::prelude::{ColliderHandle, ContactPair};
use std::collections::HashMap;

/// managed's `IntersectionEventType_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    TouchBegin = 0,
    TouchEnd = 1,
    TouchPersists = 2,
    Hit = 3,
    TriggerBegin = 4,
    TriggerEnd = 5,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactEvent {
    pub kind: EventKind,
    pub a: ColliderHandle,
    pub b: ColliderHandle,
    pub point: Vector,
    /// From `a` towards `b`.
    pub normal: Vector,
    /// `b`'s velocity relative to `a`'s at `point`.
    pub speed: Vector,
    pub normal_speed: f32,
    pub impulse: f32,
}

/// A contact closer than this (inches) touches; Rapier also keeps speculative contacts further out.
const TOUCH_DISTANCE: f32 = 0.1;

#[derive(Default)]
pub(crate) struct Contacts {
    touching: HashMap<(ColliderHandle, ColliderHandle), Contact>,
    pub events: Vec<ContactEvent>,
}

fn touches(pair: &ContactPair) -> bool {
    pair.has_any_active_contact() && pair.find_deepest_contact().is_some_and(|(_, c)| c.dist <= TOUCH_DISTANCE)
}

fn event(world: &PhysicsWorld, kind: EventKind, a: ColliderHandle, b: ColliderHandle) -> ContactEvent {
    let mut e = ContactEvent { kind, a, b, point: Vector::ZERO, normal: Vector::ZERO, speed: Vector::ZERO, normal_speed: 0.0, impulse: 0.0 };
    let (Some(ca), Some(cb)) = (world.colliders.get(a), world.colliders.get(b)) else { return e };
    let pair = world.narrow_phase.contact_pair(a, b);
    let deepest = pair.and_then(|p| p.find_deepest_contact());
    e.point = match deepest {
        Some((m, c)) => {
            let flip = pair.is_some_and(|p| p.collider1 != a);
            e.normal = if flip { -m.data.normal } else { m.data.normal };
            if flip { cb.position() * c.local_p2 } else { ca.position() * c.local_p1 }
        }
        None => (ca.compute_aabb().center() + cb.compute_aabb().center()) * 0.5,
    };
    e.impulse = pair.map_or(0.0, |p| p.total_impulse_magnitude());
    let velocity = |c: &rapier3d::prelude::Collider| c.parent().and_then(|b| world.bodies.get(b)).map_or(Vector::ZERO, |b| b.velocity_at_point(e.point));
    e.speed = velocity(cb) - velocity(ca);
    e.normal_speed = e.speed.dot(e.normal);
    e
}

impl Contacts {
    /// Diff this step's touching pairs against the last step's, queueing events.
    pub fn update(&mut self, world: &PhysicsWorld, hooks: &Hooks) {
        let mut now = HashMap::with_capacity(self.touching.len());
        for pair in world.narrow_phase.contact_pairs().filter(|p| touches(p)) {
            let kind = hooks.classify(pair.collider1, pair.collider2, false);
            if kind != Contact::None {
                now.insert(ordered(pair.collider1, pair.collider2), kind);
            }
        }
        for (a, b, _) in world.narrow_phase.intersection_pairs().filter(|p| p.2) {
            now.insert(ordered(a, b), Contact::Trigger);
        }
        let wants = |a, b, bit| [a, b].iter().any(|c| hooks.meta.get(c).is_some_and(|m: &ShapeMeta| m.functions & bit != 0));
        for (&(a, b), &kind) in &now {
            let was = self.touching.get(&(a, b)).copied();
            let kind_event = match (kind, was) {
                (Contact::Trigger, None) => Some(EventKind::TriggerBegin),
                (Contact::Solid, None) if wants(a, b, function::TOUCH) => Some(EventKind::TouchBegin),
                (Contact::Solid, Some(_)) if wants(a, b, function::PERSISTS) => Some(EventKind::TouchPersists),
                _ => None,
            };
            if let Some(k) = kind_event {
                self.events.push(event(world, k, a, b));
            }
        }
        for (&(a, b), &kind) in &self.touching {
            if now.contains_key(&(a, b)) {
                continue;
            }
            match kind {
                Contact::Trigger => self.events.push(event(world, EventKind::TriggerEnd, a, b)),
                Contact::Solid if wants(a, b, function::TOUCH) => self.events.push(event(world, EventKind::TouchEnd, a, b)),
                _ => {}
            }
        }
        self.touching = now;
    }

    /// Whether `a` touches `b` (solidly, or as a trigger when `triggers`).
    pub fn touching(&self, a: ColliderHandle, b: ColliderHandle, triggers: bool) -> bool {
        let want = if triggers { Contact::Trigger } else { Contact::Solid };
        self.touching.get(&ordered(a, b)) == Some(&want)
    }

    /// Forget pairs involving a removed shape without reporting them.
    pub fn forget(&mut self, shape: ColliderHandle) {
        self.touching.retain(|&(a, b), _| a != shape && b != shape);
    }
}

fn ordered(a: ColliderHandle, b: ColliderHandle) -> (ColliderHandle, ColliderHandle) {
    if a.into_raw_parts() <= b.into_raw_parts() { (a, b) } else { (b, a) }
}
