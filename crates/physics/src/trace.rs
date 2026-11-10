//! Sweeps: a ray or shape moved from `start` to `end`, reporting what it hits first (or all of it).

use crate::shape::{Shape, Surface};
use crate::world::World;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::parry::query::{ShapeCastOptions, ShapeCastStatus};
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter, Ray};

/// A trace: `probe` (`None` for a ray) at `rotation`, swept from `start` to `end`.
#[derive(Debug, Clone)]
pub struct Sweep {
    pub probe: Option<Shape>,
    pub rotation: Rotation,
    pub start: Vector,
    pub end: Vector,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub shape: ColliderHandle,
    /// 0 at `start`, 1 at `end`.
    pub fraction: f32,
    /// Where the probe's origin is at impact.
    pub position: Vector,
    /// The contact point, on the hit shape.
    pub point: Vector,
    /// The hit shape's outward normal there.
    pub normal: Vector,
    pub started_in_solid: bool,
}

/// Bounds `trace_all`'s re-casting; a sweep through more shapes than this is truncated.
const MAX_HITS: usize = 256;

impl World {
    /// The first shape `keep` accepts along the sweep.
    pub fn trace(&self, sweep: &Sweep, keep: &dyn Fn(ColliderHandle, &Collider) -> bool) -> Option<Hit> {
        let pipeline = self.raw().query_pipeline_with_filter(QueryFilter::default().predicate(&keep));
        let delta = sweep.end - sweep.start;
        let Some(probe) = &sweep.probe else {
            let (shape, hit) = pipeline.cast_ray_and_get_normal(&Ray::new(sweep.start, delta), 1.0, true)?;
            let point = sweep.start + delta * hit.time_of_impact;
            return Some(Hit { shape, fraction: hit.time_of_impact, position: point, point, normal: hit.normal, started_in_solid: hit.time_of_impact == 0.0 });
        };
        let collider = probe.collider(Surface::default())?.build();
        let pose = Pose::from_parts(sweep.start, sweep.rotation) * *collider.position();
        let options = ShapeCastOptions { max_time_of_impact: 1.0, compute_impact_geometry_on_penetration: true, ..Default::default() };
        let (shape, hit) = pipeline.cast_shape(&pose, delta, collider.shape(), options)?;
        let started_in_solid = hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist && hit.time_of_impact == 0.0;
        let position = sweep.start + delta * hit.time_of_impact;
        Some(Hit { shape, fraction: hit.time_of_impact, position, point: hit.witness1, normal: hit.normal1, started_in_solid })
    }

    /// Every shape `keep` accepts along the sweep, nearest first.
    pub fn trace_all(&self, sweep: &Sweep, keep: &dyn Fn(ColliderHandle, &Collider) -> bool) -> Vec<Hit> {
        let mut hits: Vec<Hit> = Vec::new();
        while hits.len() < MAX_HITS {
            let unseen = |h: ColliderHandle, c: &Collider| keep(h, c) && !hits.iter().any(|x| x.shape == h);
            let Some(hit) = self.trace(sweep, &unseen) else { break };
            hits.push(hit);
        }
        hits.sort_by(|a, b| a.fraction.total_cmp(&b.fraction));
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::BodyType;

    fn wall_at_x(world: &mut World, x: f32) -> ColliderHandle {
        let body = world.add_body(BodyType::Static, Pose::from_translation(Vector::new(x, 0.0, 0.0)));
        let slab = Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::new(1.0, 50.0, 50.0) };
        world.add_shape(body, &slab, Surface::default()).unwrap()
    }

    fn sweep(probe: Option<Shape>) -> Sweep {
        Sweep { probe, rotation: Rotation::IDENTITY, start: Vector::ZERO, end: Vector::new(100.0, 0.0, 0.0) }
    }

    #[test]
    fn ray_and_sphere_hit_the_near_wall_first() {
        let mut world = World::default();
        let near = wall_at_x(&mut world, 40.0);
        let far = wall_at_x(&mut world, 80.0);
        world.step(1.0 / 60.0, 1);

        let ray = world.trace(&sweep(None), &|_, _| true).unwrap();
        assert_eq!(ray.shape, near);
        assert!((ray.fraction - 0.39).abs() < 1e-3, "face at x=39: {}", ray.fraction);
        assert!((ray.normal.x + 1.0).abs() < 1e-3, "normal faces the ray: {:?}", ray.normal);

        let ball = world.trace(&sweep(Some(Shape::Sphere { center: Vector::ZERO, radius: 4.0 })), &|_, _| true).unwrap();
        assert!((ball.position.x - 35.0).abs() < 0.05, "sphere centre stops at 39-4: {:?}", ball.position);
        assert!((ball.point.x - 39.0).abs() < 0.05, "contact on the wall face: {:?}", ball.point);

        let skip_near = world.trace(&sweep(None), &|h, _| h != near).unwrap();
        assert_eq!(skip_near.shape, far, "filters skip shapes");

        let all = world.trace_all(&sweep(None), &|_, _| true);
        assert_eq!(all.iter().map(|h| h.shape).collect::<Vec<_>>(), [near, far]);
    }

    #[test]
    fn starting_inside_reports_solid() {
        let mut world = World::default();
        wall_at_x(&mut world, 0.0);
        world.step(1.0 / 60.0, 1);
        assert!(world.trace(&sweep(None), &|_, _| true).unwrap().started_in_solid);
    }
}
