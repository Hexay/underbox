//! Native trace rules at the boundaries, measured with the probe (docs/differential.md): rays
//! grazing a hull miss, a shape sweep that starts inside reports the normal it would have entered
//! through, and a zero-length shape sweep reports what it overlaps.

use crate::trace::{Hit, SWEEP_SKIN};
use crate::world::World;
use rapier3d::math::{Pose, Vector};
use rapier3d::parry::query;
use rapier3d::parry::shape::{ConvexPolyhedron, Shape as ParryShape};
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter, Ray};

/// How close (inches) a ray may come to a hull's boundary and still count as grazing it.
const GRAZE: f32 = 1e-3;

/// The normal real reports for a zero-length sweep that starts inside.
const STILL_NORMAL: Vector = Vector::X;

impl World {
    /// The first shape `keep` accepts along the ray, skipping hulls it only grazes.
    pub(crate) fn trace_ray(&self, start: Vector, delta: Vector, keep: &dyn Fn(ColliderHandle, &Collider) -> bool) -> Option<Hit> {
        let mut grazed: Vec<ColliderHandle> = Vec::new();
        loop {
            let unseen = |h: ColliderHandle, c: &Collider| !grazed.contains(&h) && keep(h, c);
            let pipeline = self.raw().query_pipeline_with_filter(QueryFilter::default().predicate(&unseen));
            let (shape, hit) = pipeline.cast_ray_and_get_normal(&Ray::new(start, delta), 1.0, true)?;
            let collider = self.shape(shape)?;
            if let Some(hull) = collider.shape().as_convex_polyhedron() {
                let local = collider.position().inverse();
                if grazes(hull, local.transform_point(start), local.rotation * delta) {
                    grazed.push(shape);
                    continue;
                }
            }
            let point = start + delta * hit.time_of_impact;
            return Some(Hit { shape, fraction: hit.time_of_impact, position: point, point, normal: hit.normal, started_in_solid: hit.time_of_impact == 0.0 });
        }
    }

    /// A zero-length shape sweep: the first shape it overlaps, as a start-solid hit.
    pub(crate) fn overlap_hit(&self, pose: &Pose, probe: &dyn ParryShape, at: Vector, keep: &dyn Fn(ColliderHandle, &Collider) -> bool) -> Option<Hit> {
        let pipeline = self.raw().query_pipeline_with_filter(QueryFilter::default().predicate(&keep));
        let (shape, _) = pipeline.intersect_shape(*pose, probe).next()?;
        Some(Hit { shape, fraction: 0.0, position: at, point: at, normal: STILL_NORMAL, started_in_solid: true })
    }

    /// For a sweep starting inside `shape`: the normal where the probe, moving along `delta`,
    /// would have entered it. Found by sweeping in from far enough back to start clear of it.
    pub(crate) fn entry_normal(&self, shape: ColliderHandle, pose: &Pose, delta: Vector, probe: &dyn ParryShape) -> Option<Vector> {
        let target = self.shape(shape)?.compute_aabb();
        let reach = probe.compute_local_bounding_sphere();
        let back = (target.maxs - target.mins).length() + (pose.translation - target.center()).length() + 2.0 * (reach.radius + reach.center.length()) + 1.0;
        let run = delta.normalize_or_zero() * back;
        let from = Pose::from_translation(-run) * *pose;
        let only = |h: ColliderHandle, _: &Collider| h == shape;
        let (_, hit) = self.cast_refined(&from, run, probe, 0.0, &only)?;
        if hit.time_of_impact <= 0.0 {
            return None;
        }
        // Real takes the normal at the skinned stop (its z is 5/10.031, not 5/10). The cast's time
        // of impact is too coarse for that, so step the gap to the skin along the run.
        let dir = run.normalize_or_zero();
        let target = self.shape(shape)?;
        let mut at = Pose::from_translation(run * hit.time_of_impact) * from;
        let mut normal = hit.normal1;
        for _ in 0..3 {
            let contact = query::contact(target.position(), target.shape(), &at, probe, 1.0).ok()??;
            normal = contact.normal1;
            let approach = -dir.dot(normal);
            if approach <= f32::EPSILON {
                break;
            }
            at = Pose::from_translation(dir * ((contact.dist - SWEEP_SKIN) / approach)) * at;
        }
        Some(normal)
    }
}

/// Whether a ray (hull-local, `delta` spans it) only touches the hull's boundary: it runs in a
/// face's plane, or enters and leaves at the same point. Real misses those; parry hits them.
fn grazes(hull: &ConvexPolyhedron, start: Vector, delta: Vector) -> bool {
    let length = delta.length();
    if length <= f32::EPSILON {
        return false;
    }
    let dir = delta / length;
    let (mut enter, mut leave) = (f32::MIN, f32::MAX);
    for face in hull.faces() {
        let vertex = hull.vertices_adj_to_face()[face.first_vertex_or_edge as usize];
        let offset = face.normal.dot(hull.points()[vertex as usize]);
        let (dist, rate) = (face.normal.dot(start) - offset, face.normal.dot(dir));
        if rate.abs() <= 1e-6 {
            if dist > -GRAZE {
                return dist < GRAZE;
            }
            continue;
        }
        let t = -dist / rate;
        if rate < 0.0 {
            enter = enter.max(t);
        } else {
            leave = leave.min(t);
        }
    }
    leave - enter < GRAZE
}

#[cfg(test)]
mod tests {
    use crate::shape::{Shape, Surface};
    use crate::trace::Sweep;
    use crate::world::{BodyType, World};
    use rapier3d::math::{Pose, Rotation, Vector};

    fn hull_world() -> World {
        let mut world = World::default();
        let body = world.add_body(BodyType::Static, Pose::IDENTITY);
        let points = [(-50.0, 0.0), (50.0, 0.0), (40.0, 25.0), (-50.0, 25.0)];
        let points = [-5.0, 5.0].into_iter().flat_map(|x| points.map(|(y, z)| Vector::new(x, y, z))).collect();
        world.add_shape(body, &Shape::Hull { pose: Pose::IDENTITY, points }, Surface::default()).unwrap();
        world.step(0.02, 2);
        world
    }

    fn ray(world: &World, from: Vector, to: Vector) -> bool {
        world.trace(&Sweep { probe: None, rotation: Rotation::IDENTITY, start: from, end: to }, &|_, _| true).is_some()
    }

    #[test]
    fn rays_grazing_a_hull_miss() {
        let world = hull_world();
        let down = |y: f32| ray(&world, Vector::new(0.0, y, 100.0), Vector::new(0.0, y, -100.0));
        let up = |y: f32| ray(&world, Vector::new(0.0, y, -100.0), Vector::new(0.0, y, 100.0));
        assert!(down(-49.9) && up(-49.9) && up(49.9), "inside the ends");
        assert!(!down(-50.0), "in the end face's plane");
        assert!(!up(-50.0) && !up(50.0), "through the bottom face's edges");
    }

    fn slab_world() -> World {
        let mut world = World::default();
        let body = world.add_body(BodyType::Static, Pose::from_translation(Vector::new(0.0, 0.0, -8.0)));
        let slab = Shape::Box { center: Vector::ZERO, rotation: Rotation::IDENTITY, half_extents: Vector::new(2000.0, 2000.0, 8.0) };
        world.add_shape(body, &slab, Surface::default()).unwrap();
        world.step(0.02, 2);
        world
    }

    /// Real's `SOLID` markers: a radius-10 sphere 5 above the slab top (so 5 inside) reports the
    /// normal at the slab edge it would have come in over, 1/32 off it.
    #[test]
    fn start_solid_sweeps_report_their_entry_normal() {
        let world = slab_world();
        let ball = Some(Shape::Sphere { center: Vector::ZERO, radius: 10.0 });
        let normal = |z: f32, to: Vector| {
            let at = Vector::new(0.0, 1200.0, z);
            let hit = world.trace(&Sweep { probe: ball.clone(), rotation: Rotation::IDENTITY, start: at, end: at + to }, &|_, _| true).unwrap();
            assert!(hit.started_in_solid && hit.fraction == 0.0);
            hit.normal
        };
        let close = |a: Vector, b: Vector| (a - b).length() < 2e-3;
        assert!(close(normal(5.0, Vector::new(100.0, 0.0, 0.0)), Vector::new(-0.8669, 0.0, 0.4984)), "{:?}", normal(5.0, Vector::new(100.0, 0.0, 0.0)));
        assert!(close(normal(9.0, Vector::new(100.0, 0.0, 0.0)), Vector::new(-0.4416, 0.0, 0.8972)));
        assert!(close(normal(-5.0, Vector::new(100.0, 0.0, 0.0)), Vector::new(-1.0, 0.0, 0.0)));
        assert!(close(normal(5.0, Vector::new(0.0, 100.0, 0.0)), Vector::new(0.0, -0.8669, 0.4984)));
        assert!(close(normal(5.0, Vector::new(0.0, 0.0, -100.0)), Vector::Z));
        assert!(close(normal(5.0, Vector::ZERO), Vector::X), "a zero-length sweep reports the overlap");
    }
}
