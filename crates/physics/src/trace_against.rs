//! A sweep against one free-standing shape, outside any world (`PhysicsTrace.TraceAgainst*`).

use crate::shape::{Shape, Surface};
use crate::trace::{Hit, Sweep};
use rapier3d::math::Pose;
use rapier3d::parry::query::{self, Ray, ShapeCastOptions, ShapeCastStatus};
use rapier3d::prelude::ColliderHandle;

impl Sweep {
    /// Where this sweep first meets `target` placed at `pose`; the hit's `shape` is invalid.
    pub fn against(&self, target: &Shape, pose: Pose) -> Option<Hit> {
        let target = target.collider(Surface::default())?.build();
        let target_pose = pose * *target.position();
        let delta = self.end - self.start;
        let shape = ColliderHandle::invalid();
        let Some(probe) = &self.probe else {
            let hit = target.shape().cast_ray_and_get_normal(&target_pose, &Ray::new(self.start, delta), 1.0, true)?;
            let point = self.start + delta * hit.time_of_impact;
            return Some(Hit { shape, fraction: hit.time_of_impact, position: point, point, normal: hit.normal, started_in_solid: hit.time_of_impact == 0.0 });
        };
        let probe = probe.collider(Surface::default())?.build();
        let probe_pose = Pose::from_parts(self.start, self.rotation) * *probe.position();
        let options = ShapeCastOptions { max_time_of_impact: 1.0, compute_impact_geometry_on_penetration: true, ..Default::default() };
        let hit = query::cast_shapes(&target_pose, rapier3d::math::Vector::ZERO, target.shape(), &probe_pose, delta, probe.shape(), options).ok()??;
        // Witness and normal 1 are in the target's local space.
        let started_in_solid = hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist && hit.time_of_impact == 0.0;
        Some(Hit {
            shape,
            fraction: hit.time_of_impact,
            position: self.start + delta * hit.time_of_impact,
            point: target_pose.transform_point(hit.witness1),
            normal: target_pose.rotation * hit.normal1,
            started_in_solid,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier3d::math::{Rotation, Vector};

    fn sweep(probe: Option<Shape>) -> Sweep {
        Sweep { probe, rotation: Rotation::IDENTITY, start: Vector::ZERO, end: Vector::new(100.0, 0.0, 0.0) }
    }

    #[test]
    fn sweeps_meet_a_placed_shape() {
        let ball = Shape::Sphere { center: Vector::ZERO, radius: 10.0 };
        let at = Pose::from_translation(Vector::new(50.0, 0.0, 0.0));
        let ray = sweep(None).against(&ball, at).unwrap();
        assert!((ray.fraction - 0.4).abs() < 1e-3, "{}", ray.fraction);
        assert!((ray.normal.x + 1.0).abs() < 1e-3);
        let cube = Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::splat(5.0) };
        let swept = sweep(Some(cube)).against(&ball, at).unwrap();
        assert!((swept.position.x - 35.0).abs() < 0.05, "{:?}", swept.position);
        assert!((swept.point.x - 40.0).abs() < 0.05, "contact on the ball, in world space: {:?}", swept.point);
        assert!(sweep(None).against(&ball, Pose::from_translation(Vector::new(0.0, 50.0, 0.0))).is_none());
    }
}
