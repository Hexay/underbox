//! Per-shape filter data and the Rapier hooks that apply collision rules to each pair.

use crate::rules::{CollisionRules, Rule};
use rapier3d::prelude::{ColliderHandle, PairFilterContext, PhysicsHooks, SolverFlags};
use std::collections::HashMap;

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

/// s&box's `default.surface` density, 2000 kg/m³, per cubic inch.
pub const DEFAULT_DENSITY: f32 = 2000.0 / 61_023.74;

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
