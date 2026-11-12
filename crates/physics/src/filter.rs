//! Per-shape filter data and the Rapier hooks that apply collision rules to each pair.

use crate::rules::{CollisionRules, Rule};
use crate::world::World;
use rapier3d::prelude::{ActiveHooks, ColliderHandle, PairFilterContext, PhysicsHooks, RigidBodyHandle, SolverFlags};
use rustc_hash::FxHashMap as HashMap;

/// managed's `CollisionFunctionMask`.
pub mod function {
    pub const SOLID: u8 = 1 << 0;
    pub const TOUCH: u8 = 1 << 1;
    pub const SELF_COLLISIONS: u8 = 1 << 2;
    pub const PERSISTS: u8 = 1 << 3;
    pub const DEFAULT: u8 = SOLID | TOUCH;
}

/// What a shape is beyond its geometry: tags and switches the filter and events read.
#[derive(Debug, Clone)]
pub struct ShapeMeta {
    pub tags: Vec<u32>,
    /// A tag kept in `tags` whatever else the shape is given or loses (native's `DefaultSolidTag`).
    pub base_tag: Option<u32>,
    pub functions: u8,
    /// kg per cubic inch.
    pub density: f32,
    pub massless: bool,
}

impl Default for ShapeMeta {
    fn default() -> Self {
        Self { tags: Vec::new(), base_tag: None, functions: function::DEFAULT, density: DEFAULT_DENSITY, massless: false }
    }
}

impl ShapeMeta {
    pub fn keep_base_tag(&mut self) {
        if let Some(t) = self.base_tag
            && !self.tags.contains(&t)
        {
            self.tags.push(t);
        }
    }
}

pub const CUBIC_INCHES_PER_CUBIC_METRE: f32 = 61_023.74;

/// s&box's `default.surface` density, 2000 kg/m³, per cubic inch.
pub const DEFAULT_DENSITY: f32 = 2000.0 / CUBIC_INCHES_PER_CUBIC_METRE;

/// What native's body `GetDensity` reports for a body without shapes, kg/m³ (probe `MASS`).
pub const SHAPELESS_BODY_DENSITY: f32 = 1100.0;

/// How a pair of shapes interacts after rules and switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Contact {
    Solid,
    Trigger,
    None,
}

pub(crate) struct Hooks<'a> {
    pub rules: &'a CollisionRules,
    pub meta: &'a HashMap<ColliderHandle, ShapeMeta>,
}

impl Hooks<'_> {
    /// `sensor` says either shape is a trigger.
    pub fn classify(&self, a: ColliderHandle, b: ColliderHandle, sensor: bool) -> Contact {
        let empty = ShapeMeta::default();
        let (ma, mb) = (self.meta.get(&a).unwrap_or(&empty), self.meta.get(&b).unwrap_or(&empty));
        match self.rules.between(&ma.tags, &mb.tags) {
            Rule::Ignore | Rule::Unset => Contact::None,
            _ if sensor => Contact::Trigger,
            Rule::Trigger => Contact::Trigger,
            Rule::Collide if (ma.functions & mb.functions) & function::SOLID == 0 => Contact::None,
            Rule::Collide => Contact::Solid,
        }
    }
}

impl PhysicsHooks for Hooks<'_> {
    fn filter_contact_pair(&self, cx: &PairFilterContext) -> Option<SolverFlags> {
        match self.classify(cx.collider1, cx.collider2, false) {
            Contact::Solid => Some(SolverFlags::default()),
            // Keep the manifold (so the pair counts as touching) but give it no response.
            Contact::Trigger => Some(SolverFlags::empty()),
            Contact::None => None,
        }
    }

    fn filter_intersection_pair(&self, cx: &PairFilterContext) -> bool {
        self.classify(cx.collider1, cx.collider2, true) != Contact::None
    }
}

impl World {
    /// Only shapes some pair could filter run the hooks: Rapier never recycles a hooked pair's
    /// contacts, and real engine2 keeps a contact's points and normal until they drift
    /// (docs/differential-joints.md, orbmatch). Rapier filters a pair if either shape has hooks.
    /// Jointed bodies keep them too: recycled contacts freeze their normals in world space and
    /// fight the soft joint locks, so a car on its wheels creeps instead of resting.
    pub(crate) fn refresh_hooks(&mut self, id: ColliderHandle) {
        let (Some(meta), Some(c)) = (self.meta.get(&id), self.inner.colliders.get(id)) else { return };
        let jointed = c.parent().is_some_and(|b| self.inner.impulse_joints.attached_joints(b).next().is_some());
        let clean = !jointed && meta.functions & function::SOLID != 0 && !self.rules.needs_filter(&meta.tags);
        let hooks = if clean { ActiveHooks::empty() } else { ActiveHooks::FILTER_CONTACT_PAIRS | ActiveHooks::FILTER_INTERSECTION_PAIR };
        if let Some(c) = self.inner.colliders.get_mut(id) {
            c.set_active_hooks(hooks);
        }
    }

    pub(crate) fn refresh_body_hooks(&mut self, body: RigidBodyHandle) {
        let shapes = self.inner.bodies.get(body).map(|rb| rb.colliders().to_vec()).unwrap_or_default();
        for id in shapes {
            self.refresh_hooks(id);
        }
    }
}
