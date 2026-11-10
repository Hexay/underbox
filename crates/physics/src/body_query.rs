//! Questions about one body's shapes: closest point, overlap and penetration against another body,
//! and buoyancy.

use crate::world::{INCHES_PER_METRE, World};
use rapier3d::math::{Pose, Vector};
use rapier3d::parry::query;
use rapier3d::prelude::{Collider, RigidBodyHandle};

impl World {
    fn colliders_of(&self, body: RigidBodyHandle) -> impl Iterator<Item = &Collider> {
        self.body(body).into_iter().flat_map(|rb| rb.colliders().iter().filter_map(|&c| self.shape(c)))
    }

    /// `body`'s shapes with `other`'s, `other` moved to `other_pose`.
    fn shape_pairs(&self, body: RigidBodyHandle, other: RigidBodyHandle, other_pose: Pose) -> Vec<(Pose, &Collider, Pose, &Collider)> {
        let Some(rb) = self.body(other) else { return Vec::new() };
        let to_pose = other_pose * rb.position().inverse();
        let mut pairs = Vec::new();
        for a in self.colliders_of(body) {
            for b in self.colliders_of(other) {
                pairs.push((*a.position(), a, to_pose * *b.position(), b));
            }
        }
        pairs
    }

    /// The point on `body`'s shapes nearest `point` (itself when inside).
    pub fn closest_point(&self, body: RigidBodyHandle, point: Vector) -> Option<Vector> {
        let projections = self.colliders_of(body).map(|c| c.shape().project_point(c.position(), point, true).point);
        projections.min_by(|a, b| a.distance_squared(point).total_cmp(&b.distance_squared(point)))
    }

    /// Whether `body` overlaps `other` placed at `other_pose`.
    pub fn bodies_overlap(&self, body: RigidBodyHandle, other: RigidBodyHandle, other_pose: Pose) -> bool {
        self.shape_pairs(body, other, other_pose)
            .into_iter()
            .any(|(pa, a, pb, b)| query::intersection_test(&pa, a.shape(), &pb, b.shape()).is_ok_and(|i| i.intersecting))
    }

    /// The deepest penetration of `body` into `other` placed at `other_pose`: the direction to move
    /// `body` out, and how far.
    pub fn penetration(&self, body: RigidBodyHandle, other: RigidBodyHandle, other_pose: Pose) -> Option<(Vector, f32)> {
        let contacts = self.shape_pairs(body, other, other_pose).into_iter().filter_map(|(pa, a, pb, b)| query::contact(&pa, a.shape(), &pb, b.shape(), 0.0).ok().flatten());
        let deepest = contacts.filter(|c| c.dist < 0.0).min_by(|a, b| a.dist.total_cmp(&b.dist))?;
        Some((-deepest.normal1, -deepest.dist))
    }

    /// Push `body` up out of a fluid below the plane (`origin`, `normal`): buoyancy from each
    /// shape's submerged fraction (estimated from its bounds along `normal`), plus drag towards
    /// the fluid's velocity. `density` is kg/m³; `gravity` inches/s².
    #[allow(clippy::too_many_arguments)]
    pub fn apply_buoyancy(&mut self, body: RigidBodyHandle, origin: Vector, normal: Vector, density: f32, linear_drag: f32, angular_drag: f32, fluid_velocity: Vector, gravity: Vector, dt: f32) {
        let density = density / INCHES_PER_METRE.powi(3);
        let normal = normal.normalize_or_zero();
        let mut impulses = Vec::new();
        let mut submerged_total = 0.0;
        let mut volume_total = 0.0;
        for c in self.colliders_of(body) {
            let aabb = c.compute_aabb();
            let (center, half) = (aabb.center(), aabb.half_extents());
            let reach = half.dot(normal.abs());
            let depth = (origin - center).dot(normal);
            let fraction = if reach > 0.0 { ((depth + reach) / (2.0 * reach)).clamp(0.0, 1.0) } else { 0.0 };
            let volume = c.shape().mass_properties(1.0).mass();
            volume_total += volume;
            if fraction > 0.0 {
                submerged_total += volume * fraction;
                let under = center - normal * reach * (1.0 - fraction);
                impulses.push((-gravity * density * volume * fraction * dt, under));
            }
        }
        let Some(rb) = self.body_mut(body) else { return };
        if impulses.is_empty() || volume_total <= 0.0 {
            return;
        }
        for (impulse, at) in impulses {
            rb.apply_impulse_at_point(impulse, at, true);
        }
        let share = submerged_total / volume_total;
        let mass = rb.mass();
        let drag = (fluid_velocity - rb.linvel()) * (linear_drag * share * mass * dt).min(mass);
        rb.apply_impulse(drag, true);
        let spin = rb.angvel() * (1.0 - (angular_drag * share * dt).min(1.0));
        rb.set_angvel(spin, true);
    }
}

#[cfg(test)]
mod tests {
    use crate::shape::{Shape, Surface};
    use crate::world::{BodyType, World};
    use rapier3d::math::{Pose, Rotation, Vector};

    fn cube(world: &mut World, at: Vector) -> rapier3d::prelude::RigidBodyHandle {
        let b = world.add_body(BodyType::Dynamic, Pose::from_translation(at));
        world.add_shape(b, &Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::splat(5.0) }, Surface::default());
        b
    }

    #[test]
    fn closest_point_overlap_and_penetration() {
        let mut world = World::default();
        let a = cube(&mut world, Vector::ZERO);
        let b = cube(&mut world, Vector::new(100.0, 0.0, 0.0));
        assert_eq!(world.closest_point(a, Vector::new(20.0, 0.0, 0.0)), Some(Vector::new(5.0, 0.0, 0.0)));
        assert!(!world.bodies_overlap(a, b, *world.body(b).unwrap().position()));
        let near = Pose::from_translation(Vector::new(8.0, 0.0, 0.0));
        assert!(world.bodies_overlap(a, b, near));
        let (dir, depth) = world.penetration(a, b, near).unwrap();
        assert!((depth - 2.0).abs() < 1e-3 && (dir - Vector::new(-1.0, 0.0, 0.0)).length() < 1e-3, "{dir:?} {depth}");
    }

    #[test]
    fn a_light_cube_floats() {
        let mut world = World::default();
        let block = cube(&mut world, Vector::ZERO);
        world.set_mass(block, 0.5 * 1000.0 * 1000.0 / 61_023.74);
        for _ in 0..600 {
            world.apply_buoyancy(block, Vector::ZERO, Vector::Z, 1000.0, 1.0, 1.0, Vector::ZERO, world.gravity(), 1.0 / 60.0);
            world.step(1.0 / 60.0, 1);
        }
        let z = world.body(block).unwrap().translation().z;
        assert!(z.abs() < 1.5, "half-density cube floats half under: centre at {z}");
    }
}
