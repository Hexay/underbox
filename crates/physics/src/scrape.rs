//! Scrape contacts: where a body's shapes slide against others last step, with the friction
//! force there (managed's `CollisionSoundSystem` turns these into scrape sounds).

use crate::world::World;
use rapier3d::math::Vector;
use rapier3d::prelude::{ColliderHandle, RigidBodyHandle};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scrape {
    pub shape: ColliderHandle,
    pub other: ColliderHandle,
    /// The manifold's contacts' centre, world space.
    pub point: Vector,
    /// From `shape` towards `other`.
    pub normal: Vector,
    /// Friction impulse over the last substep, as a force.
    pub friction_force: f32,
}

impl World {
    /// Up to `max` of `body`'s contact manifolds that carried friction last step.
    pub fn scrape_contacts(&self, body: RigidBodyHandle, max: usize) -> Vec<Scrape> {
        let Some(rb) = self.body(body) else { return Vec::new() };
        let dt = self.inner.integration_parameters.dt.max(f32::EPSILON);
        let mut out = Vec::new();
        if max == 0 {
            return out;
        }
        for &shape in rb.colliders() {
            for pair in self.inner.narrow_phase.contact_pairs_with(shape).filter(|p| p.has_any_active_contact()) {
                let flip = pair.collider1 != shape;
                let other = if flip { pair.collider1 } else { pair.collider2 };
                let (Some(c1), Some(c2)) = (self.shape(pair.collider1), self.shape(pair.collider2)) else { continue };
                for m in pair.manifolds() {
                    // Gotcha: the twist-friction solver keeps one manifold-wide friction impulse in
                    // every point's warmstart value and leaves `tangent_impulse` at zero.
                    let per_point: f32 = m.points.iter().map(|p| p.data.tangent_impulse.norm()).sum();
                    let manifold = m.points.iter().map(|p| p.data.warmstart_tangent_impulse.norm()).fold(0.0, f32::max);
                    let friction = if per_point > 0.0 { per_point } else { manifold };
                    if friction <= 0.0 || m.points.is_empty() {
                        continue;
                    }
                    let sum = m.points.iter().fold(Vector::ZERO, |acc, p| acc + if flip { c2.position() * p.local_p2 } else { c1.position() * p.local_p1 });
                    let normal = if flip { -m.data.normal } else { m.data.normal };
                    out.push(Scrape { shape, other, point: sum / m.points.len() as f32, normal, friction_force: friction / dt });
                    if out.len() >= max {
                        return out;
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::{BodyType, Pose, Shape, Surface, Vector, World};

    #[test]
    fn a_sliding_box_scrapes_the_ground_and_a_resting_one_does_not() {
        let mut w = World::default();
        let ground = w.add_body(BodyType::Static, Pose::IDENTITY);
        let slab = Shape::Box { center: Vector::ZERO, rotation: crate::Rotation::IDENTITY, half_extents: Vector::new(500.0, 500.0, 10.0) };
        w.add_shape(ground, &slab, Surface::default()).unwrap();
        let cube = Shape::Box { center: Vector::ZERO, rotation: crate::Rotation::IDENTITY, half_extents: Vector::splat(5.0) };
        let (slider, rester) = (w.add_body(BodyType::Dynamic, Pose::from_translation(Vector::new(0.0, 0.0, 15.0))), w.add_body(BodyType::Dynamic, Pose::from_translation(Vector::new(100.0, 0.0, 15.0))));
        let slider_shape = w.add_shape(slider, &cube, Surface::default()).unwrap();
        w.add_shape(rester, &cube, Surface::default()).unwrap();
        for _ in 0..30 {
            w.step(1.0 / 60.0, 1);
        }
        w.body_mut(slider).unwrap().set_linvel(Vector::new(200.0, 0.0, 0.0), true);
        w.step(1.0 / 60.0, 1);
        let s = w.scrape_contacts(slider, 8);
        assert!(!s.is_empty(), "sliding makes friction");
        assert_eq!(s[0].shape, slider_shape);
        assert!(s[0].friction_force > 0.0 && (s[0].point.z - 10.0).abs() < 0.5, "{s:?}");
        assert!(s[0].normal.z < -0.9, "points from the box into the ground: {:?}", s[0].normal);
        assert!(w.scrape_contacts(rester, 8).iter().all(|s| s.friction_force < 1.0), "resting barely rubs");
        assert_eq!(w.scrape_contacts(slider, 0).len(), 0);
    }
}
