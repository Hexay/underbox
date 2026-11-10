use crate::shape::{Shape, Surface};
use rapier3d::math::{Pose, Vector};
use rapier3d::pipeline::PhysicsWorld;
use rapier3d::prelude::{Collider, ColliderHandle, RigidBody, RigidBodyBuilder, RigidBodyHandle, RigidBodyType};

/// Rapier's tolerances assume metres; s&box measures in inches.
pub const INCHES_PER_METRE: f32 = 39.37;

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

pub struct World {
    inner: PhysicsWorld,
    sleeping: bool,
}

impl Default for World {
    fn default() -> Self {
        let mut inner = PhysicsWorld::new();
        inner.integration_parameters.length_unit = INCHES_PER_METRE;
        Self { inner, sleeping: true }
    }
}

impl World {
    pub(crate) fn raw(&self) -> &PhysicsWorld {
        &self.inner
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

    /// Advance `dt` seconds in `substeps` equal steps.
    pub fn step(&mut self, dt: f32, substeps: u32) {
        let substeps = substeps.max(1);
        self.inner.integration_parameters.dt = dt / substeps as f32;
        for _ in 0..substeps {
            self.inner.step();
        }
    }

    pub fn add_body(&mut self, kind: BodyType, pose: Pose) -> RigidBodyHandle {
        let body = RigidBodyBuilder::new(kind.rapier()).pose(pose).can_sleep(self.sleeping);
        self.inner.insert_body(body)
    }

    /// Remove a body and every shape on it.
    pub fn remove_body(&mut self, id: RigidBodyHandle) {
        self.inner.remove_body_with_colliders(id, true);
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

    /// Scale the body's shape densities so it weighs `mass` (native keeps shape proportions).
    pub fn set_mass(&mut self, id: RigidBodyHandle, mass: f32) {
        let Some(body) = self.inner.bodies.get(id) else { return };
        let current = body.mass();
        if current <= 0.0 || mass <= 0.0 {
            return;
        }
        for &shape in body.colliders() {
            if let Some(c) = self.inner.colliders.get_mut(shape) {
                c.set_density(c.density() * mass / current);
            }
        }
        let PhysicsWorld { bodies, colliders, .. } = &mut self.inner;
        if let Some(body) = bodies.get_mut(id) {
            body.recompute_mass_properties_from_colliders(colliders);
        }
    }

    pub fn add_shape(&mut self, body: RigidBodyHandle, shape: &Shape, surface: Surface) -> Option<ColliderHandle> {
        let collider = shape.collider(surface)?;
        self.inner.bodies.get(body)?;
        Some(self.inner.insert_collider(collider, Some(body)))
    }

    /// Swap a shape's geometry for `shape`'s, keeping its body and surface.
    pub fn update_shape(&mut self, id: ColliderHandle, shape: &Shape) {
        let Some(c) = self.inner.colliders.get_mut(id) else { return };
        let surface = Surface { friction: c.friction(), elasticity: c.restitution(), trigger: c.is_sensor() };
        let Some(new) = shape.collider(surface).map(|b| b.build()) else { return };
        c.set_shape(new.shared_shape().clone());
        if let Some(local) = new.position_wrt_parent() {
            c.set_position_wrt_parent(*local);
        }
    }

    /// A shape's world-space bounds, (mins, maxs).
    pub fn shape_bounds(&self, id: ColliderHandle) -> Option<(Vector, Vector)> {
        let aabb = self.inner.colliders.get(id)?.compute_aabb();
        Some((aabb.mins, aabb.maxs))
    }

    /// The union of a body's shapes' world-space bounds.
    pub fn body_bounds(&self, id: RigidBodyHandle) -> Option<(Vector, Vector)> {
        let bounds = self.inner.bodies.get(id)?.colliders().iter().filter_map(|&c| self.shape_bounds(c));
        bounds.reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
    }

    /// Shapes overlapping `probe` (in world space). Gotcha: the broad phase updates in `step`,
    /// so shapes added or moved since the last step aren't seen yet.
    pub fn overlapping(&self, probe: &Shape) -> Vec<ColliderHandle> {
        let Some(probe) = probe.collider(Surface::default()).map(|b| b.build()) else { return Vec::new() };
        let pipeline = self.inner.query_pipeline();
        pipeline.intersect_shape(*probe.position(), probe.shape()).map(|(h, _)| h).collect()
    }

    pub fn remove_shape(&mut self, id: ColliderHandle) {
        self.inner.remove_collider(id);
    }

    pub fn shape(&self, id: ColliderHandle) -> Option<&Collider> {
        self.inner.colliders.get(id)
    }

    pub fn shape_mut(&mut self, id: ColliderHandle) -> Option<&mut Collider> {
        self.inner.colliders.get_mut(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
