use crate::contacts::{ContactEvent, Contacts};
use crate::filter::{Hooks, ShapeMeta};
use crate::joint::Joints;
use crate::mass::BodyMass;
use crate::rules::CollisionRules;
use rapier3d::math::{Pose, Vector};
use rapier3d::pipeline::PhysicsWorld;
use rapier3d::prelude::{ColliderHandle, RigidBody, RigidBodyBuilder, RigidBodyHandle, RigidBodyType};
use std::collections::{HashMap, HashSet};

/// Rapier's tolerances assume metres; s&box measures in inches.
pub const INCHES_PER_METRE: f32 = 39.37;

/// Native's world gravity until managed sets one (inches/s²).
pub const DEFAULT_GRAVITY: Vector = Vector::new(0.0, 0.0, -800.0);

const MIN_SUBSTEPS: u32 = 4;

/// managed's `PhysicsBodyType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyType {
    Static = 0,
    Keyframed = 1,
    Dynamic = 2,
}

impl BodyType {
    pub fn from_raw(raw: i64) -> Option<Self> {
        [Self::Static, Self::Keyframed, Self::Dynamic].into_iter().find(|t| *t as i64 == raw)
    }

    fn rapier(self) -> RigidBodyType {
        match self {
            Self::Static => RigidBodyType::Fixed,
            Self::Keyframed => RigidBodyType::KinematicPositionBased,
            Self::Dynamic => RigidBodyType::Dynamic,
        }
    }

    pub fn of(body: &RigidBody) -> Self {
        match body.body_type() {
            RigidBodyType::Fixed => Self::Static,
            RigidBodyType::Dynamic => Self::Dynamic,
            _ => Self::Keyframed,
        }
    }
}

/// A moving body after a step, for managed's `OnActive`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Activity {
    pub body: RigidBodyHandle,
    pub user_data: u128,
    pub fell_asleep: bool,
}

pub struct World {
    pub(crate) inner: PhysicsWorld,
    sleeping: bool,
    max_speed: f32,
    pub(crate) rules: CollisionRules,
    pub(crate) meta: HashMap<ColliderHandle, ShapeMeta>,
    pub(crate) mass: HashMap<RigidBodyHandle, BodyMass>,
    pub(crate) contacts: Contacts,
    pub(crate) joints: Joints,
    awake: HashSet<RigidBodyHandle>,
    activity: Vec<Activity>,
}

impl Default for World {
    fn default() -> Self {
        let mut inner = PhysicsWorld::new();
        inner.integration_parameters.length_unit = INCHES_PER_METRE;
        // Fitted to real engine2's box landings (docs/differential.md, divergence 2): at 50 Hz a
        // falling prop moves ~11 in a tick, past Rapier's default 0.8 in speculative margin.
        inner.integration_parameters.normalized_prediction_distance = 0.4;
        inner.integration_parameters.num_internal_stabilization_iterations = 3;
        inner.gravity = DEFAULT_GRAVITY;
        Self {
            inner,
            sleeping: true,
            max_speed: f32::MAX,
            rules: CollisionRules::default(),
            meta: HashMap::new(),
            mass: HashMap::new(),
            contacts: Contacts::default(),
            joints: Joints::default(),
            awake: HashSet::new(),
            activity: Vec::new(),
        }
    }
}

impl World {
    pub(crate) fn raw(&self) -> &PhysicsWorld {
        &self.inner
    }

    pub(crate) fn raw_mut(&mut self) -> &mut PhysicsWorld {
        &mut self.inner
    }

    pub fn gravity(&self) -> Vector {
        self.inner.gravity
    }

    pub fn set_gravity(&mut self, gravity: Vector) {
        self.inner.gravity = gravity;
    }

    pub fn sleeping_enabled(&self) -> bool {
        self.sleeping
    }

    /// Applies to bodies added from now on, as native's world does.
    pub fn set_sleeping_enabled(&mut self, enabled: bool) {
        self.sleeping = enabled;
    }

    /// Bodies are clamped to this speed (inches/s) after every substep.
    pub fn set_max_speed(&mut self, speed: f32) {
        self.max_speed = if speed > 0.0 { speed } else { f32::MAX };
    }

    pub fn set_collision_rules(&mut self, rules: CollisionRules) {
        self.rules = rules;
    }

    /// Advance `dt` seconds, integrating `max(substeps, 4)` times as native does (measured in
    /// docs/differential.md, divergence 3): one collision pass, one Rapier solver iteration per
    /// integration. Then collect contacts and activity.
    pub fn step(&mut self, dt: f32, substeps: u32) {
        self.inner.integration_parameters.dt = dt;
        self.inner.integration_parameters.num_solver_iterations = substeps.max(MIN_SUBSTEPS) as usize;
        self.drive_joints();
        let hooks = Hooks { rules: &self.rules, meta: &self.meta };
        self.inner.step_with_events(&hooks, &());
        self.clamp_speeds();
        self.measure_joints(1);
        let hooks = Hooks { rules: &self.rules, meta: &self.meta };
        self.contacts.update(&self.inner, &hooks);
        self.collect_activity();
    }

    fn clamp_speeds(&mut self) {
        if self.max_speed == f32::MAX {
            return;
        }
        let max = self.max_speed;
        for (_, rb) in self.inner.rigid_bodies_mut() {
            let v = rb.linvel();
            if v.length_squared() > max * max {
                rb.set_linvel(v.normalize() * max, false);
            }
        }
    }

    fn collect_activity(&mut self) {
        let mut awake = HashSet::with_capacity(self.awake.len());
        for (h, rb) in self.inner.rigid_bodies().filter(|(_, rb)| rb.is_dynamic() && rb.is_enabled()) {
            let was = self.awake.contains(&h);
            if !rb.is_sleeping() {
                awake.insert(h);
                self.activity.push(Activity { body: h, user_data: rb.user_data, fell_asleep: false });
            } else if was {
                self.activity.push(Activity { body: h, user_data: rb.user_data, fell_asleep: true });
            }
        }
        self.awake = awake;
    }

    /// Bodies that moved or fell asleep since the last call.
    pub fn take_activity(&mut self) -> Vec<Activity> {
        std::mem::take(&mut self.activity)
    }

    /// Touch and trigger events since the last call.
    pub fn take_contact_events(&mut self) -> Vec<ContactEvent> {
        std::mem::take(&mut self.contacts.events)
    }

    /// Whether two shapes touch (solidly, or as a trigger when `triggers`), as of the last step.
    pub fn is_touching(&self, a: ColliderHandle, b: ColliderHandle, triggers: bool) -> bool {
        self.contacts.touching(a, b, triggers)
    }

    pub fn add_body(&mut self, kind: BodyType, pose: Pose) -> RigidBodyHandle {
        let body = RigidBodyBuilder::new(kind.rapier()).pose(pose).can_sleep(self.sleeping);
        let id = self.inner.insert_body(body);
        self.mass.insert(id, BodyMass::default());
        id
    }

    /// Remove a body with its shapes and joints.
    pub fn remove_body(&mut self, id: RigidBodyHandle) {
        if let Some(rb) = self.inner.bodies.get(id) {
            for c in rb.colliders().to_vec() {
                self.meta.remove(&c);
                self.contacts.forget(c);
            }
        }
        self.inner.remove_body_with_colliders(id, true);
        self.mass.remove(&id);
        self.awake.remove(&id);
        let joints = &self.inner.impulse_joints;
        self.joints.entries.retain(|h, _| joints.contains(*h));
    }

    pub fn body(&self, id: RigidBodyHandle) -> Option<&RigidBody> {
        self.inner.bodies.get(id)
    }

    pub fn body_mut(&mut self, id: RigidBodyHandle) -> Option<&mut RigidBody> {
        self.inner.bodies.get_mut(id)
    }

    pub fn set_body_type(&mut self, id: RigidBodyHandle, kind: BodyType) {
        if let Some(body) = self.body_mut(id) {
            body.set_body_type(kind.rapier(), true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::{Shape, Surface};

    #[test]
    fn box_falls_and_rests_on_ground() {
        let mut world = World::default();
        world.set_gravity(Vector::new(0.0, 0.0, -800.0));
        let ground = world.add_body(BodyType::Static, Pose::IDENTITY);
        let flat = Shape::Box { center: Vector::ZERO, rotation: Default::default(), half_extents: Vector::new(500.0, 500.0, 10.0) };
        world.add_shape(ground, &flat, Surface::default()).unwrap();
        let crate_ = world.add_body(BodyType::Dynamic, Pose::from_translation(Vector::new(0.0, 0.0, 200.0)));
        let cube = Shape::Box { center: Vector::ZERO, rotation: Default::default(), half_extents: Vector::splat(16.0) };
        world.add_shape(crate_, &cube, Surface::default()).unwrap();
        for _ in 0..240 {
            world.step(1.0 / 60.0, 1);
        }
        let z = world.body(crate_).unwrap().translation().z;
        assert!((z - 26.0).abs() < 1.0, "resting height {z}, want ~26 (ground top 10 + half 16)");

        let probe = |z| Shape::Sphere { center: Vector::new(0.0, 0.0, z), radius: 4.0 };
        assert_eq!(world.overlapping(&probe(26.0)).len(), 1, "sphere inside the resting cube");
        assert!(world.overlapping(&probe(1000.0)).is_empty());
    }

    #[test]
    fn body_types_round_trip() {
        let mut world = World::default();
        let id = world.add_body(BodyType::Keyframed, Pose::IDENTITY);
        assert_eq!(BodyType::of(world.body(id).unwrap()), BodyType::Keyframed);
        world.set_body_type(id, BodyType::Dynamic);
        assert_eq!(BodyType::of(world.body(id).unwrap()), BodyType::Dynamic);
        assert_eq!(BodyType::from_raw(2), Some(BodyType::Dynamic));
    }

    #[test]
    fn speeds_are_clamped() {
        let mut world = World::default();
        world.set_max_speed(100.0);
        let ball = world.add_body(BodyType::Dynamic, Pose::IDENTITY);
        world.add_shape(ball, &Shape::Sphere { center: Vector::ZERO, radius: 4.0 }, Surface::default()).unwrap();
        world.body_mut(ball).unwrap().set_linvel(Vector::new(5000.0, 0.0, 0.0), true);
        world.step(1.0 / 60.0, 2);
        assert!((world.body(ball).unwrap().linvel().length() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn activity_reports_moving_bodies_then_sleep() {
        let mut world = World::default();
        world.set_gravity(Vector::ZERO);
        let ball = world.add_body(BodyType::Dynamic, Pose::IDENTITY);
        world.add_shape(ball, &Shape::Sphere { center: Vector::ZERO, radius: 4.0 }, Surface::default()).unwrap();
        world.step(1.0 / 60.0, 1);
        assert_eq!(world.take_activity(), [Activity { body: ball, user_data: 0, fell_asleep: false }]);
        let fell = (0..600).any(|_| {
            world.step(1.0 / 60.0, 1);
            world.take_activity().iter().any(|a| a.fell_asleep)
        });
        assert!(fell, "a resting body reports falling asleep once");
        world.step(1.0 / 60.0, 1);
        assert!(world.take_activity().is_empty(), "sleeping bodies are quiet");
    }
}
