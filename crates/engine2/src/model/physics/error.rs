//! The error model's collision. Real engine2's compiled `models/dev/error.vmdl_c` carries one hull
//! (the "ERROR" sign's outline, 10 deep along X); this is that hull as measured with probe rays
//! (docs/differential.md, divergence 7), since its source mesh isn't public.

use super::data::{HullDesc, Part, PhysicsData, Shapes};
use super::hull::Hull;
use glam::Vec3;
use std::sync::Arc;

/// The outline in the YZ plane, extruded to x = ±5.
const OUTLINE: [(f32, f32); 6] = [(-50.0, 0.0), (50.0, 0.0), (47.81, 20.17), (46.47, 22.78), (42.82, 24.99), (-50.0, 25.08)];

pub fn physics() -> Option<Arc<PhysicsData>> {
    let points: Vec<Vec3> = [-5.0, 5.0].into_iter().flat_map(|x| OUTLINE.map(|(y, z)| Vec3::new(x, y, z))).collect();
    let hull = Hull::from_points(&points)?;
    let part = Part { shapes: Shapes { hulls: vec![HullDesc { hull, ids: Default::default() }], ..Default::default() }, ..Default::default() };
    Some(PhysicsData::new(super::compiled::EMBEDDED, vec![part], vec![], vec![0]))
}

#[cfg(test)]
mod tests {
    #[test]
    fn hull_matches_the_measured_bounds() {
        let data = super::physics().unwrap();
        let (lo, hi) = data.parts[0].shapes.hulls[0].hull.bbox();
        assert_eq!((lo.x, lo.y, lo.z, hi.x, hi.y), (-5.0, -50.0, 0.0, 5.0, 50.0));
        assert!((hi.z - 25.08).abs() < 1e-3);
    }
}
