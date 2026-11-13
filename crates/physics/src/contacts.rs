//! Touch and trigger tracking: which shape pairs touch after each step, and the begin/persist/end
//! events managed's `ProcessIntersections` drains.

use crate::filter::{Contact, Hooks, ShapeMeta, function};
use rapier3d::math::Vector;
use rapier3d::pipeline::PhysicsWorld;
use rapier3d::prelude::{ColliderHandle, ContactPair};
use rustc_hash::FxHashMap;

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

/// A shape's place in creation order, then its handle for shapes created before tracking.
type Rank = (u64, u32, u32);

/// A touching pair, `a` created before `b`.
#[derive(Clone, Copy)]
struct Touch {
    key: (Rank, Rank),
    a: ColliderHandle,
    b: ColliderHandle,
    kind: Contact,
}

#[derive(Default)]
pub(crate) struct Contacts {
    /// Sorted by `key`: shape creation order, which pairs and events follow as native's do
    /// (orbmatch's tie-break).
    touching: Vec<Touch>,
    spare: Vec<Touch>,
    created: FxHashMap<ColliderHandle, u64>,
    next: u64,
    pub events: Vec<ContactEvent>,
    /// Solid pairs that started touching in the last step.
    pub began: Vec<(ColliderHandle, ColliderHandle)>,
}

/// Gotcha: a recycled manifold (`persist.rs`) keeps the distance of the step it was made in, so a
/// body that lands from far enough away never reads as touching; measure the pair again instead.
/// Overlapping pairs skip the contact query's EPA. Not `distance` for the rest: for box pairs the
/// contact query disagrees with GJK's distance both ways near the threshold (0.24 apart reads as
/// touching; one pair 0.004 apart doesn't).
fn touches(world: &PhysicsWorld, pair: &ContactPair) -> bool {
    if !pair.has_any_active_contact() {
        return false;
    }
    let (Some(a), Some(b)) = (world.colliders.get(pair.collider1), world.colliders.get(pair.collider2)) else { return false };
    let (pa, pb) = (a.position(), b.position());
    if rapier3d::parry::query::intersection_test(pa, a.shape(), pb, b.shape()).is_ok_and(|i| i.intersecting) {
        return true;
    }
    match rapier3d::parry::query::contact(pa, a.shape(), pb, b.shape(), TOUCH_DISTANCE) {
        Ok(found) => found.is_some(),
        Err(_) => pair.find_deepest_contact().is_some_and(|(_, c)| c.dist <= TOUCH_DISTANCE),
    }
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

fn find(touches: &[Touch], key: (Rank, Rank)) -> Option<&Touch> {
    touches.binary_search_by_key(&key, |t| t.key).ok().map(|i| &touches[i])
}

/// Each of `from`'s touches with its match in `other`; both sorted by key without repeats.
fn merged<'a>(from: &'a [Touch], other: &'a [Touch]) -> impl Iterator<Item = (&'a Touch, Option<&'a Touch>)> {
    let mut j = 0;
    from.iter().map(move |t| {
        while other.get(j).is_some_and(|o| o.key < t.key) {
            j += 1;
        }
        (t, other.get(j).filter(|o| o.key == t.key))
    })
}

impl Contacts {
    /// Diff this step's touching pairs against the last step's, queueing events.
    pub fn update(&mut self, world: &PhysicsWorld, hooks: &Hooks) {
        let mut now = std::mem::take(&mut self.spare);
        now.clear();
        for pair in world.narrow_phase.contact_pairs().filter(|p| touches(world, p)) {
            let kind = hooks.classify(pair.collider1, pair.collider2, false);
            if kind != Contact::None {
                now.push(self.touch(pair.collider1, pair.collider2, kind));
            }
        }
        for (a, b, _) in world.narrow_phase.intersection_pairs().filter(|p| p.2) {
            now.push(self.touch(a, b, Contact::Trigger));
        }
        // Stable sort, and a repeated pair keeps its last entry (a trigger over a solid contact).
        now.sort_by_key(|t| t.key);
        now.dedup_by(|later, kept| {
            let same = later.key == kept.key;
            if same {
                *kept = *later;
            }
            same
        });
        let wants = |a, b, bit| [a, b].iter().any(|c| hooks.meta.get(c).is_some_and(|m: &ShapeMeta| m.functions & bit != 0));
        self.began.clear();
        for (t, was) in merged(&now, &self.touching) {
            let was = was.map(|w| w.kind);
            if t.kind == Contact::Solid && was.is_none() {
                self.began.push((t.a, t.b));
            }
            let kind_event = match (t.kind, was) {
                (Contact::Trigger, None) => Some(EventKind::TriggerBegin),
                (Contact::Solid, None) if wants(t.a, t.b, function::TOUCH) => Some(EventKind::TouchBegin),
                (Contact::Solid, Some(_)) if wants(t.a, t.b, function::PERSISTS) => Some(EventKind::TouchPersists),
                _ => None,
            };
            if let Some(k) = kind_event {
                self.events.push(event(world, k, t.a, t.b));
            }
        }
        for (t, _) in merged(&self.touching, &now).filter(|(_, kept)| kept.is_none()) {
            match t.kind {
                Contact::Trigger => self.events.push(event(world, EventKind::TriggerEnd, t.a, t.b)),
                Contact::Solid if wants(t.a, t.b, function::TOUCH) => self.events.push(event(world, EventKind::TouchEnd, t.a, t.b)),
                _ => {}
            }
        }
        self.spare = std::mem::replace(&mut self.touching, now);
    }

    /// Whether `a` touches `b` (solidly, or as a trigger when `triggers`).
    pub fn touching(&self, a: ColliderHandle, b: ColliderHandle, triggers: bool) -> bool {
        let want = if triggers { Contact::Trigger } else { Contact::Solid };
        find(&self.touching, self.touch(a, b, want).key).is_some_and(|t| t.kind == want)
    }

    pub fn created(&mut self, shape: ColliderHandle) {
        self.created.insert(shape, self.next);
        self.next += 1;
    }

    /// Forget pairs involving a removed shape without reporting them.
    pub fn forget(&mut self, shape: ColliderHandle) {
        self.touching.retain(|t| t.a != shape && t.b != shape);
        self.created.remove(&shape);
    }

    fn rank(&self, c: ColliderHandle) -> Rank {
        let (index, generation) = c.into_raw_parts();
        (self.created.get(&c).copied().unwrap_or(u64::MAX), index, generation)
    }

    fn touch(&self, a: ColliderHandle, b: ColliderHandle, kind: Contact) -> Touch {
        let (ra, rb) = (self.rank(a), self.rank(b));
        if ra <= rb { Touch { key: (ra, rb), a, b, kind } } else { Touch { key: (rb, ra), a: b, b: a, kind } }
    }
}
