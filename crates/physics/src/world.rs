use crate::contacts::{ContactEvent, Contacts};
use crate::filter::{Hooks, ShapeMeta};
use crate::joint::Joints;
use crate::mass::BodyMass;
use crate::rules::CollisionRules;
use rapier3d::math::{Pose, Vector};
use rapier3d::pipeline::PhysicsWorld;
use rapier3d::prelude::{ColliderHandle, RigidBody, RigidBodyBuilder, RigidBodyHandle, RigidBodyType};
// Fx, not SipHash: these are looked up per body and pair every step (docs/perf-physics.md).
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

/// Rapier's tolerances assume metres; s&box measures in inches.
pub const INCHES_PER_METRE: f32 = 39.37;

/// Native's world gravity until managed sets one (inches/s²).
pub const DEFAULT_GRAVITY: Vector = Vector::new(0.0, 0.0, -850.0);

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
    /// Auto-sleeping bodies' thresholds (inches/s); see `sleep.rs`.
    pub(crate) sleep_thresholds: HashMap<RigidBodyHandle, f32>,
    awake: HashSet<RigidBodyHandle>,
    activity: Vec<Activity>,
    pub(crate) step_start: HashMap<RigidBodyHandle, crate::bounce::Start>,
    pub(crate) frozen: HashMap<(ColliderHandle, ColliderHandle), crate::persist::Frozen>,
}

impl Default for World {
    fn default() -> Self {
        let mut inner = PhysicsWorld::new();
        inner.integration_parameters.length_unit = INCHES_PER_METRE;
        // Fitted to real engine2's box landings (docs/differential.md, divergence 2). A wider
        // speculative margin fitted better but kept bodies near animated ones awake (zoo chairs).
        // Kept over Box2D's single relax iteration: see docs/differential-joints.md, 17.
        inner.integration_parameters.num_internal_pgs_iterations = 2;
        inner.integration_parameters.num_internal_stabilization_iterations = 3;
        // Fitted to real's resting depths: 0.0086 on the ground, stacks as in rest_tests.rs.
        inner.integration_parameters.static_contact_softness.natural_frequency = 50.0;
        inner.integration_parameters.contact_softness.natural_frequency = 25.0;
        inner.gravity = DEFAULT_GRAVITY;
        Self {
            inner,
            sleeping: true,
            max_speed: f32::MAX,
            rules: CollisionRules::default(),
            meta: HashMap::default(),
            mass: HashMap::default(),
            contacts: Contacts::default(),
            joints: Joints::default(),
            sleep_thresholds: HashMap::default(),
            awake: HashSet::default(),
            activity: Vec::new(),
            step_start: HashMap::default(),
            frozen: HashMap::default(),
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
        let shapes: Vec<_> = self.meta.keys().copied().collect();
        for id in shapes {
            self.refresh_hooks(id);
        }
    }

    /// Advance `dt` seconds, integrating `max(substeps, 4)` times as native does (measured in
    /// docs/differential.md, divergence 3): one collision pass, one Rapier solver iteration per
    /// integration. Then collect contacts and activity.
    pub fn step(&mut self, dt: f32, substeps: u32) {
        self.inner.integration_parameters.dt = dt;
        self.inner.integration_parameters.num_solver_iterations = substeps.max(MIN_SUBSTEPS) as usize;
        self.drive_joints();
        self.arm_sleep();
        self.record_step_start();
        let start = self.force_turned_contacts();
        let hooks = Hooks { rules: &self.rules, meta: &self.meta };
        self.inner.step_with_events(&hooks, &());
        self.release_forced_contacts(start);
        self.bounce_approaching_contacts();
        self.clamp_speeds();
        self.measure_joints(1);
        let hooks = Hooks { rules: &self.rules, meta: &self.meta };
        self.contacts.update(&self.inner, &hooks);
        self.restart_sleep_on_new_contacts();
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
        let mut awake = HashSet::with_capacity_and_hasher(self.awake.len(), Default::default());
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
        self.set_auto_sleep(id, self.sleeping, crate::sleep::DEFAULT_SLEEP_THRESHOLD);
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
        let partners: Vec<_> = self.inner.impulse_joints.attached_joints(id).map(|(a, b, ..)| if a == id { b } else { a }).collect();
        self.inner.remove_body_with_colliders(id, true);
        for body in partners {
            self.refresh_body_hooks(body);
        }
        self.mass.remove(&id);
        self.sleep_thresholds.remove(&id);
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
