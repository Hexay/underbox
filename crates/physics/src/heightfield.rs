//! Terrain heightfields in Rubikon's layout. Samples are row-major (`x + y * size_x`). In the
//! shape's frame, height runs along +Y and sample (x, y) sits at (y * cell, h, x * cell) — managed
//! turns that frame into terrain space (Terrain.GetTargetTransform). `CastHeightField` uses a
//! second frame, sample (x, y) at (x * cell, h, -y * cell) (Terrain.RayIntersects).

use rapier3d::math::{Pose, Vector};
use rapier3d::parry::query::{Ray, RayCast};
use rapier3d::parry::shape::{HeightField, HeightFieldCellStatus};
use rapier3d::parry::utils::Array2;

/// A grid of height samples with per-sample holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub size_x: usize,
    pub size_y: usize,
    /// Raw samples, `x + y * size_x`; world height is `sample * height_scale`.
    pub heights: Vec<u16>,
    /// The cell whose low corner is this sample has no surface.
    pub holes: Vec<bool>,
    pub cell: f32,
    pub height_scale: f32,
}

impl Grid {
    /// `None` unless it's at least 2x2 with one height per sample.
    pub fn new(size_x: usize, size_y: usize, heights: Vec<u16>, cell: f32, height_scale: f32) -> Option<Self> {
        (size_x >= 2 && size_y >= 2 && heights.len() == size_x * size_y)
            .then(|| Self { size_x, size_y, holes: vec![false; heights.len()], heights, cell, height_scale })
    }

    fn scale(&self, along_x: usize, along_z: usize) -> Vector {
        Vector::new((along_x - 1) as f32 * self.cell, self.height_scale, (along_z - 1) as f32 * self.cell)
    }

    /// The parry shape in the collider frame, and where its centre sits in that frame.
    pub fn shape(&self) -> (HeightField, Pose) {
        // Array2 is column-major (i + j * nrows): rows along Z = sample x, columns along X = sample y.
        let data = self.heights.iter().map(|&h| h as f32).collect();
        let scale = self.scale(self.size_y, self.size_x);
        let mut field = HeightField::new(Array2::new(self.size_x, self.size_y, data), scale);
        for y in 0..self.size_y - 1 {
            for x in 0..self.size_x - 1 {
                if self.holes[x + y * self.size_x] {
                    field.set_cell_status(x, y, HeightFieldCellStatus::CELL_REMOVED);
                }
            }
        }
        (field, Pose::from_translation(scale * Vector::new(0.5, 0.0, 0.5)))
    }

    /// `CastHeightField`: where the ray `start + t * delta` (t in 0..=1) first meets the grid in
    /// the cast frame, as (point, normal, t).
    pub fn cast_ray(&self, start: Vector, delta: Vector) -> Option<(Vector, Vector, f32)> {
        // Rows along Z run from y = size_y - 1 (most negative Z) up to y = 0.
        let rows = self.size_y;
        let data = (0..self.size_x).flat_map(|x| (0..rows).map(move |i| (x, rows - 1 - i))).map(|(x, y)| self.heights[x + y * self.size_x] as f32);
        let scale = self.scale(self.size_x, rows);
        let field = HeightField::new(Array2::new(rows, self.size_x, data.collect()), scale);
        let pose = Pose::from_translation(scale * Vector::new(0.5, 0.0, -0.5));
        let hit = field.cast_ray_and_get_normal(&pose, &Ray::new(start, delta), 1.0, true)?;
        Some((start + delta * hit.time_of_impact, hit.normal, hit.time_of_impact))
    }

    /// Overwrite the `w`x`h` block at (`x`, `y`): heights from a full-size grid, holes from a
    /// block-sized one. Out-of-range parts are ignored.
    pub fn update(&mut self, (x, y, w, h): (usize, usize, usize, usize), heights: Option<&[u16]>, holes: Option<&[bool]>) {
        for by in 0..h.min(self.size_y.saturating_sub(y)) {
            for bx in 0..w.min(self.size_x.saturating_sub(x)) {
                let i = (x + bx) + (y + by) * self.size_x;
                if let Some(&v) = heights.and_then(|s| s.get(i)) {
                    self.heights[i] = v;
                }
                if let Some(&hole) = holes.and_then(|s| s.get(bx + by * w)) {
                    self.holes[i] = hole;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier3d::math::Rotation;

    /// 4x4 samples, all flat at 10 except sample (1, 1) at 30; one unit per sample step.
    fn grid() -> Grid {
        let mut heights = vec![10; 16];
        heights[1 + 4] = 30;
        Grid::new(4, 4, heights, 1.0, 1.0).unwrap()
    }

    /// Terrain-space (sample units) probes off the grid lines, with the height each should
    /// find; a mirrored or transposed mapping finds 10 at the first two.
    const PROBES: [(f32, f32, f32); 3] = [(0.5, 1.0, 20.0), (1.0, 0.5, 20.0), (2.5, 2.5, 10.0)];

    /// Managed's offsets: GetTargetTransform for the collider, RayIntersects for the cast (cell 1).
    fn terrain(yaw: f32, roll: f32) -> Pose {
        let r = Rotation::from_rotation_z(yaw.to_radians()) * Rotation::from_rotation_x(roll.to_radians());
        Pose::from_parts(Vector::new(0.5, 0.5, 0.0), r)
    }

    #[test]
    fn samples_land_where_terrain_expects_them() {
        let g = grid();
        let (field, local) = g.shape();
        let to_terrain = terrain(90.0, 90.0) * local;
        // Sample (x, y) belongs at terrain ((x + 0.5) * cell, (y + 0.5) * cell, height).
        let down = |x: f32, y: f32| {
            let ray = Ray::new(Vector::new(x + 0.5, y + 0.5, 100.0), Vector::new(0.0, 0.0, -1.0));
            field.cast_ray(&to_terrain, &ray, 1000.0, true).map(|t| 100.0 - t)
        };
        let cast = terrain(0.0, 90.0);
        let cast_down = |x: f32, y: f32| {
            let start = cast.inverse().transform_point(Vector::new(x + 0.5, y + 0.5, 100.0));
            let delta = cast.rotation.inverse() * Vector::new(0.0, 0.0, -200.0);
            g.cast_ray(start, delta).map(|(p, _, _)| cast.transform_point(p).z)
        };
        for (x, y, h) in PROBES {
            assert!(down(x, y).is_some_and(|z| (z - h).abs() < 1e-3), "collider at ({x},{y}): {:?}", down(x, y));
            assert!(cast_down(x, y).is_some_and(|z| (z - h).abs() < 1e-3), "cast at ({x},{y}): {:?}", cast_down(x, y));
        }
    }

    #[test]
    fn holes_and_partial_updates() {
        let mut g = grid();
        let mut full = vec![0; 16];
        full[6] = 50;
        g.update((2, 1, 1, 1), Some(&full), None);
        assert_eq!((g.heights[6], g.heights[0]), (50, 10), "heights index the full grid, inside the block only");
        g.update((0, 0, 1, 1), None, Some(&[true]));
        assert!(g.holes[0]);
        let (field, local) = g.shape();
        let ray = Ray::new(local.inverse().transform_point(Vector::new(0.25, 100.0, 0.25)), Vector::new(0.0, -1.0, 0.0));
        assert!(field.cast_ray(&Pose::IDENTITY, &ray, 1000.0, true).is_none(), "the hole's cell has no surface");
    }
}
