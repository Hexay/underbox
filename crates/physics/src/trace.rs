//! Sweeps: a ray or shape moved from `start` to `end`, reporting what it hits first (or all of it).

use crate::shape::{Shape, Surface};
use crate::world::World;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::parry::query::{PointQueryWithLocation, ShapeCastOptions, ShapeCastStatus};
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

/// Native shape sweeps stop this far short of the surface, measured along its normal; rays don't.
pub const SWEEP_SKIN: f32 = 1.0 / 32.0;

/// How far before a shape sweep's first hit (inches) it is cast again for precision.
const REFINE: f32 = 4.0;

/// Pull a shape sweep's time of impact back so it ends [`SWEEP_SKIN`] off the hit surface.
fn skinned(toi: f32, delta: Vector, normal: Vector) -> f32 {
    let approach = -delta.dot(normal);
    if toi <= 0.0 || approach <= f32::EPSILON {
        return toi;
    }
    (toi - SWEEP_SKIN / approach).max(0.0)
}

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
        let (shape, mut hit) = pipeline.cast_shape(&pose, delta, collider.shape(), options)?;
        let started_in_solid = hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist && hit.time_of_impact == 0.0;
        // Gotcha: parry's time of impact over a long sweep can overshoot by ~0.1; recast the last stretch.
        let back = (hit.time_of_impact - REFINE / delta.length().max(f32::EPSILON)).max(0.0);
        if back > 0.0 {
            let only = |h: ColliderHandle, c: &Collider| h == shape && keep(h, c);
            let near = self.raw().query_pipeline_with_filter(QueryFilter::default().predicate(&only));
            let pose = Pose::from_translation(delta * back) * pose;
            if let Some((_, mut refined)) = near.cast_shape(&pose, delta * (1.0 - back), collider.shape(), options) {
                refined.time_of_impact = back + refined.time_of_impact * (1.0 - back);
                hit = refined;
            }
        }
        let fraction = skinned(hit.time_of_impact, delta, hit.normal1);
        let position = sweep.start + delta * fraction;
        Some(Hit { shape, fraction, position, point: hit.witness1, normal: hit.normal1, started_in_solid })
    }

    /// The triangle of mesh shape `shape` nearest world `point` (a hit's contact point). For a
    /// heightfield it's the cell under the point, as its low-corner sample's index in
    /// [`crate::Grid`] (`x + y * size_x`). `None` for other shapes.
    pub fn triangle_at(&self, shape: ColliderHandle, point: Vector) -> Option<u32> {
        let c = self.shape(shape)?;
        let local = c.position().inverse_transform_point(point);
        if let Some(field) = c.shape().as_heightfield() {
            // Rows (i) run along the sample x axis, columns (j) along y: see Grid::shape. parry
            // counts cells, one fewer than samples.
            let (i, j) = field.cell_at_point(local)?;
            return u32::try_from(i + j * (field.nrows() + 1)).ok();
        }
        let mesh = c.shape().as_trimesh()?;
        let (_, (triangle, _)) = mesh.project_local_point_and_get_location(local, false);
        Some(triangle)
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
    fn compound_sweep_stops_on_its_leading_part() {
        let mut world = World::default();
        wall_at_x(&mut world, 40.0);
        world.step(1.0 / 60.0, 1);
        let dumbbell = Shape::Compound(vec![
            Shape::Sphere { center: Vector::ZERO, radius: 2.0 },
            Shape::Sphere { center: Vector::new(10.0, 0.0, 0.0), radius: 2.0 },
        ]);
        let hit = world.trace(&sweep(Some(dumbbell)), &|_, _| true).unwrap();
        assert!((hit.position.x - 27.0).abs() < 0.05, "front ball (origin+10, r 2) meets the face at 39: {:?}", hit.position);
    }

    #[test]
    fn mesh_hits_name_their_triangle() {
        let mut world = World::default();
        let body = world.add_body(BodyType::Static, Pose::from_translation(Vector::new(50.0, 0.0, 0.0)));
        let wall = Shape::Mesh {
            vertices: vec![Vector::new(0.0, -10.0, -10.0), Vector::new(0.0, 10.0, -10.0), Vector::new(0.0, 10.0, 10.0), Vector::new(0.0, -10.0, 10.0)],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        world.add_shape(body, &wall, Surface::default()).unwrap();
        let slab = wall_at_x(&mut world, 200.0);
        world.step(1.0 / 60.0, 1);
        let at = |y: f32, z: f32| {
            let s = Sweep { probe: None, rotation: Rotation::IDENTITY, start: Vector::new(0.0, y, z), end: Vector::new(100.0, y, z) };
            let hit = world.trace(&s, &|_, _| true).unwrap();
            world.triangle_at(hit.shape, hit.point)
        };
        assert_eq!((at(5.0, -5.0), at(-5.0, 5.0)), (Some(0), Some(1)));
        assert_eq!(world.triangle_at(slab, Vector::ZERO), None, "not a mesh");
    }

    #[test]
    fn heightfield_hits_name_their_cell() {
        let mut world = World::default();
        let body = world.add_body(BodyType::Static, Pose::IDENTITY);
        let grid = crate::Grid::new(3, 4, vec![0; 12], 10.0, 1.0).unwrap();
        let field = world.add_shape(body, &Shape::HeightField(grid), Surface::default()).unwrap();
        world.step(1.0 / 60.0, 1);
        // In the shape frame sample (x, y) is at (y * cell, h, x * cell).
        let cell = |x: f32, y: f32| {
            let s = Sweep { probe: None, rotation: Rotation::IDENTITY, start: Vector::new(y * 10.0, 50.0, x * 10.0), end: Vector::new(y * 10.0, -50.0, x * 10.0) };
            let hit = world.trace(&s, &|_, _| true).unwrap();
            world.triangle_at(hit.shape, hit.point)
        };
        assert_eq!(cell(0.5, 0.5), Some(0));
        assert_eq!(cell(1.5, 0.5), Some(1), "x steps by one");
        assert_eq!(cell(0.5, 2.5), Some(6), "y steps by size_x");
        assert_eq!(world.triangle_at(field, Vector::new(500.0, 0.0, 0.0)), None, "off the grid");
    }

    #[test]
    fn shape_sweeps_keep_a_skin_along_the_normal_and_rays_do_not() {
        let mut world = World::default();
        wall_at_x(&mut world, 40.0);
        world.step(1.0 / 60.0, 1);
        let ball = Some(Shape::Sphere { center: Vector::ZERO, radius: 4.0 });
        let straight = world.trace(&sweep(ball.clone()), &|_, _| true).unwrap();
        assert!((straight.position.x - (35.0 - SWEEP_SKIN)).abs() < 1e-3, "{:?}", straight.position);
        let slant = Sweep { probe: ball, rotation: Rotation::IDENTITY, start: Vector::new(0.0, -20.0, 0.0), end: Vector::new(100.0, 80.0, 0.0) };
        let hit = world.trace(&slant, &|_, _| true).unwrap();
        assert!((hit.position.x - (35.0 - SWEEP_SKIN)).abs() < 1e-3, "skin is normal distance: {:?}", hit.position);
        assert!((world.trace(&sweep(None), &|_, _| true).unwrap().position.x - 39.0).abs() < 1e-4);
        let long = Sweep { probe: Some(Shape::Sphere { center: Vector::ZERO, radius: 1.0 }), rotation: Rotation::IDENTITY, start: Vector::new(-560.0, 0.0, 0.0), end: Vector::new(640.0, 0.0, 0.0) };
        let hit = world.trace(&long, &|_, _| true).unwrap();
        assert!((hit.position.x - (38.0 - SWEEP_SKIN)).abs() < 1e-3, "a long sweep lands as precisely: {:?}", hit.position);
    }

    #[test]
    fn starting_inside_reports_solid() {
        let mut world = World::default();
        wall_at_x(&mut world, 0.0);
        world.step(1.0 / 60.0, 1);
        assert!(world.trace(&sweep(None), &|_, _| true).unwrap().started_in_solid);
    }
}
