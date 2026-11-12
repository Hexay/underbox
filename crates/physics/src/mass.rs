//! Body mass, centre and inertia with managed's overrides.
//!
//! Colliders are built massless; a body's mass properties are summed here from its shapes'
//! densities ([`ShapeMeta`]) and handed to Rapier as the body's additional mass properties, so
//! the overrides (mass, centre, inertia, inertia scale) apply on top in one place.

use crate::filter::{CUBIC_INCHES_PER_CUBIC_METRE, SHAPELESS_BODY_DENSITY, ShapeMeta};
use crate::world::World;
use rapier3d::math::{Rotation, Vector};
use rapier3d::prelude::{MassProperties, RigidBody, RigidBodyHandle};

#[derive(Debug, Clone, Copy)]
pub struct BodyMass {
    pub mass: Option<f32>,
    pub center: Option<Vector>,
    /// Principal inertia and its frame.
    pub inertia: Option<(Vector, Rotation)>,
    pub inertia_scale: f32,
    /// While set, shape changes don't rebuild mass until [`World::build_mass`].
    pub deferred: bool,
}

impl Default for BodyMass {
    fn default() -> Self {
        Self { mass: None, center: None, inertia: None, inertia_scale: 1.0, deferred: false }
    }
}

impl World {
    /// The mass properties the body's shapes give, before overrides.
    fn shape_mass(&self, id: RigidBodyHandle) -> MassProperties {
        let Some(rb) = self.body(id) else { return MassProperties::default() };
        let empty = ShapeMeta::default();
        let parts = rb.colliders().iter().filter_map(|&c| {
            let co = self.shape(c)?;
            let meta = self.meta.get(&c).unwrap_or(&empty);
            let density = if meta.massless { 0.0 } else { meta.density };
            let local = co.position_wrt_parent().copied().unwrap_or_default();
            Some(co.shared_shape().mass_properties(density).transform_by(&local))
        });
        parts.sum()
    }

    /// Recompute the body's mass properties from its shapes and overrides, unless deferred.
    pub fn rebuild_mass(&mut self, id: RigidBodyHandle) {
        let Some(over) = self.mass.get(&id).copied() else { return };
        if over.deferred {
            return;
        }
        let mut props = self.shape_mass(id);
        if let Some(mass) = over.mass.filter(|m| *m > 0.0) {
            if props.mass() > 0.0 {
                props.set_mass(mass, true);
            } else {
                props = MassProperties::new(props.local_com, mass, Vector::splat(mass));
            }
        }
        if let Some(center) = over.center {
            props.local_com = center;
        }
        let (inertia, frame) = over.inertia.unwrap_or((props.principal_inertia(), props.principal_inertia_local_frame));
        let props = MassProperties::with_principal_inertia_frame(props.local_com, props.mass(), inertia * over.inertia_scale, frame);
        let raw = self.raw_mut();
        if let Some(rb) = raw.bodies.get_mut(id) {
            rb.set_additional_mass_properties(props, true);
            rb.recompute_mass_properties_from_colliders(&raw.colliders);
        }
    }

    fn dynamic(&self, id: RigidBodyHandle) -> Option<&RigidBody> {
        self.body(id).filter(|rb| rb.is_dynamic())
    }

    /// Native's `GetMass`: 0 for static and keyframed bodies, whose overrides are kept for
    /// when they turn dynamic (docs/differential-joints.md, 20).
    pub fn reported_mass(&self, id: RigidBodyHandle) -> f32 {
        self.dynamic(id).map_or(0.0, |rb| rb.mass())
    }

    /// Native's local mass centre: a body that isn't dynamic reports its shapes' centroid,
    /// ignoring an overridden centre.
    pub fn reported_local_center(&self, id: RigidBodyHandle) -> Vector {
        match self.dynamic(id) {
            Some(rb) => rb.local_center_of_mass(),
            None => self.shape_mass(id).local_com,
        }
    }

    /// Native's body density: its first shape's, in kg/m³.
    pub fn reported_density(&self, id: RigidBodyHandle) -> f32 {
        let first = self.body(id).and_then(|rb| rb.colliders().first().copied());
        first.and_then(|c| self.meta.get(&c)).map_or(SHAPELESS_BODY_DENSITY, |m| m.density * CUBIC_INCHES_PER_CUBIC_METRE)
    }

    /// Native's principal inertia and its frame: zero for bodies that aren't dynamic, and the
    /// body-axis diagonal (unsorted, identity frame) when the tensor is already diagonal.
    pub fn reported_inertia(&self, id: RigidBodyHandle) -> (Vector, Rotation) {
        let Some(rb) = self.dynamic(id) else { return (Vector::ZERO, Rotation::IDENTITY) };
        let props = &rb.mass_properties().local_mprops;
        let (p, frame) = (props.principal_inertia(), props.principal_inertia_local_frame);
        let cols = [Vector::X, Vector::Y, Vector::Z].map(|e| frame * e);
        let entry = |i: usize, j: usize| (0..3).map(|k| p[k] * cols[k][i] * cols[k][j]).sum::<f32>();
        let diagonal = Vector::new(entry(0, 0), entry(1, 1), entry(2, 2));
        let off = entry(0, 1).abs().max(entry(0, 2).abs()).max(entry(1, 2).abs());
        if off <= 1e-5 * diagonal.max_element() { (diagonal, Rotation::IDENTITY) } else { (p, frame) }
    }

    pub fn mass_overrides(&self, id: RigidBodyHandle) -> BodyMass {
        self.mass.get(&id).copied().unwrap_or_default()
    }

    /// Change the body's overrides and rebuild its mass.
    pub fn set_mass_overrides(&mut self, id: RigidBodyHandle, f: impl FnOnce(&mut BodyMass)) {
        if let Some(m) = self.mass.get_mut(&id) {
            f(m);
            self.rebuild_mass(id);
        }
    }

    /// Scale to `mass`, keeping the shapes' proportions (native's `SetMass`).
    pub fn set_mass(&mut self, id: RigidBodyHandle, mass: f32) {
        self.set_mass_overrides(id, |m| m.mass = (mass > 0.0).then_some(mass));
    }

    /// Defer mass rebuilds; returns the previous setting. Turning it off rebuilds.
    pub fn set_mass_deferred(&mut self, id: RigidBodyHandle, deferred: bool) -> bool {
        let Some(m) = self.mass.get_mut(&id) else { return false };
        let was = std::mem::replace(&mut m.deferred, deferred);
        self.rebuild_mass(id);
        was
    }

    /// Rebuild mass now, even while deferred.
    pub fn build_mass(&mut self, id: RigidBodyHandle) {
        let was = self.set_mass_deferred(id, false);
        self.set_mass_deferred(id, was);
    }
}

#[cfg(test)]
mod tests {
    use crate::shape::{Shape, Surface};
    use crate::world::{BodyType, World};
    use rapier3d::math::{Pose, Rotation, Vector};

    fn cube(world: &mut World) -> rapier3d::prelude::RigidBodyHandle {
        world.set_gravity(Vector::ZERO);
        let body = world.add_body(BodyType::Dynamic, Pose::IDENTITY);
        let shape = Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::splat(10.0) };
        world.add_shape(body, &shape, Surface::default()).unwrap();
        body
    }

    #[test]
    fn mass_comes_from_density_and_overrides_apply() {
        let mut world = World::default();
        let body = cube(&mut world);
        let natural = world.body(body).unwrap().mass();
        assert!((natural - 8000.0 * crate::filter::DEFAULT_DENSITY).abs() < 1e-2, "20in cube at 2000 kg/m³: {natural}");

        world.set_mass(body, 50.0);
        assert!((world.body(body).unwrap().mass() - 50.0).abs() < 1e-4);

        world.set_mass_overrides(body, |m| m.center = Some(Vector::new(0.0, 0.0, -5.0)));
        assert_eq!(world.body(body).unwrap().local_center_of_mass(), Vector::new(0.0, 0.0, -5.0));

        world.set_mass_overrides(body, |m| m.inertia = Some((Vector::new(1.0, 2.0, 3.0), Rotation::IDENTITY)));
        let inertia = world.body(body).unwrap().mass_properties().local_mprops.principal_inertia();
        assert!((inertia - Vector::new(1.0, 2.0, 3.0)).length() < 1e-3, "{inertia:?}");
    }

    /// Probe `MASS`: the zoo sign's box (10 × 100 × 25, centred at (0, 10, 12.5)).
    #[test]
    fn only_dynamic_bodies_report_mass_and_inertia() {
        let mut world = World::default();
        let body = world.add_body(BodyType::Static, Pose::IDENTITY);
        let sign = Shape::Box { center: Vector::new(0.0, 10.0, 12.5), rotation: Rotation::IDENTITY, half_extents: Vector::new(5.0, 50.0, 12.5) };
        world.add_shape(body, &sign, Surface::default()).unwrap();
        world.set_mass(body, 5.0);
        world.set_mass_overrides(body, |m| m.center = Some(Vector::new(1.0, 2.0, 3.0)));
        assert_eq!(world.reported_mass(body), 0.0);
        assert_eq!(world.reported_inertia(body), (Vector::ZERO, Rotation::IDENTITY));
        assert!((world.reported_local_center(body) - Vector::new(0.0, 10.0, 12.5)).length() < 1e-4);
        assert!((world.reported_density(body) - 2000.0).abs() < 0.01);

        world.set_body_type(body, BodyType::Dynamic);
        assert!((world.reported_mass(body) - 5.0).abs() < 1e-4, "the override survives");
        assert_eq!(world.reported_local_center(body), Vector::new(1.0, 2.0, 3.0));
        world.set_mass_overrides(body, |m| (m.mass, m.center) = (None, None));
        let (inertia, frame) = world.reported_inertia(body);
        let expected = Vector::new(725_469.1, 49_502.6, 689_622.4);
        assert!(((inertia - expected) / expected).abs().max_element() < 1e-4, "body axes, unsorted: {inertia:?}");
        assert_eq!(frame, Rotation::IDENTITY);
    }

    #[test]
    fn deferred_mass_waits_for_build() {
        let mut world = World::default();
        let body = cube(&mut world);
        world.set_mass_deferred(body, true);
        world.add_shape(body, &Shape::Sphere { center: Vector::new(30.0, 0.0, 0.0), radius: 10.0 }, Surface::default());
        let before = world.body(body).unwrap().mass();
        world.build_mass(body);
        assert!(world.body(body).unwrap().mass() > before);
    }
}
