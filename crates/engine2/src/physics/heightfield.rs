//! Terrain: `IPhysicsBody.AddHeightFieldShape`, `IPhysicsShape.UpdateHeightShape` and
//! `g_pPhysicsSystem.CastHeightField`, over [`physics::Grid`] (which documents the frames).
//! Each sample's material is a slot into the settings' `SurfaceIndices` (Terrain.Collider.cs);
//! a hit reports its cell's low-corner sample as the triangle (`World::triangle_at`), so the
//! shape's `triangle_slots` are the per-sample materials. Material 255 marks a hole.

use super::shape::{self, Type, add_with_slots, slot_surfaces};
use crate::math::{from_vec, to_vec};
use abi::slots::{g_p_physics_system as sys, i_physics_body as b, i_physics_shape as s};
use abi::structs::Vector3;
use abi::{RawPtr, Registry, bind};
use physics::{Grid, Shape as Geometry};
use std::ffi::c_void;

const HOLE: u8 = 255;

fn dims(x: i32, y: i32) -> Option<(usize, usize)> {
    Some((usize::try_from(x).ok()?, usize::try_from(y).ok()?))
}

/// SAFETY: `p` is null or points at `len` readable `T`s.
unsafe fn slice<'a, T>(p: RawPtr<c_void>, len: usize) -> Option<&'a [T]> {
    // SAFETY: per this function's contract.
    (!p.is_null()).then(|| unsafe { p.cast::<T>().as_slice(len) })
}

/// The grid and its per-sample materials (empty when managed passed none).
fn grid(heights: RawPtr<c_void>, materials: RawPtr<c_void>, size_x: i32, size_y: i32, cell: f32, height_scale: f32) -> Option<(Grid, Vec<u8>)> {
    let (sx, sy) = dims(size_x, size_y)?;
    // SAFETY: managed passes `size_x * size_y` heights and materials (or a null materials pointer).
    let (heights, materials) = unsafe { (slice::<u16>(heights, sx * sy)?, slice::<u8>(materials, sx * sy)) };
    let mut grid = Grid::new(sx, sy, heights.to_vec(), cell, height_scale)?;
    let materials = materials.map(<[u8]>::to_vec).unwrap_or_default();
    if !materials.is_empty() {
        grid.holes = materials.iter().map(|&m| m == HOLE).collect();
    }
    Some((grid, materials))
}

#[allow(clippy::too_many_arguments)]
fn update(r: shape::Ref, heights: RawPtr<c_void>, materials: RawPtr<c_void>, x: i32, y: i32, w: i32, h: i32, cell: f32, height_scale: f32) {
    let Some(shape) = shape::get(r) else { return };
    let Geometry::HeightField(mut grid) = shape.geometry.lock().unwrap().clone() else { return };
    let (Some((x, y)), Some((w, h))) = (dims(x, y), dims(w, h)) else { return };
    (grid.cell, grid.height_scale) = (cell, height_scale);
    // SAFETY: heights cover the whole grid (Terrain passes its full map); materials only the block.
    let heights = unsafe { slice::<u16>(heights, grid.heights.len()) };
    let block = unsafe { slice::<u8>(materials, w * h) };
    let holes: Option<Vec<bool>> = block.map(|m| m.iter().map(|&m| m == HOLE).collect());
    grid.update((x, y, w, h), heights, holes.as_deref());
    if let Some(block) = block {
        let size_x = grid.size_x;
        shape::with_state(r, |s| {
            s.triangle_slots.resize(grid.heights.len(), 0);
            for by in 0..h.min(grid.size_y.saturating_sub(y)) {
                for bx in 0..w.min(size_x.saturating_sub(x)) {
                    s.triangle_slots[(x + bx) + (y + by) * size_x] = block[bx + by * w];
                }
            }
        });
    }
    shape::update(r, Geometry::HeightField(grid));
}

#[allow(clippy::too_many_arguments)]
fn cast(out: &mut Vector3, normal: &mut Vector3, fraction: &mut f32, start: Vector3, delta: Vector3, heights: RawPtr<c_void>, size_x: i32, size_y: i32, cell: f32, height_scale: f32) -> bool {
    let Some((grid, _)) = grid(heights, RawPtr::from_raw(std::ptr::null_mut()), size_x, size_y, cell, height_scale) else { return false };
    let Some((p, n, t)) = grid.cast_ray(to_vec(start), to_vec(delta)) else { return false };
    (*out, *normal, *fraction) = (from_vec(p), from_vec(n), t);
    true
}

pub fn register(r: &mut Registry) {
    bind!(r, b::ADD_HEIGHT_FIELD_SHAPE, |body, heights, materials, size_x, size_y, cell, height_scale, _material_count, settings| {
        match grid(heights, materials, size_x, size_y, cell, height_scale) {
            Some((g, m)) => add_with_slots(body, Type::HeightField, Geometry::HeightField(g), settings, m, slot_surfaces(&settings)),
            None => abi::HandleId::new(-1),
        }
    });
    bind!(r, s::UPDATE_HEIGHT_SHAPE, update);
    bind!(r, sys::CAST_HEIGHT_FIELD, cast);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vec3;
    use crate::physics::trace::trace;
    use crate::physics::world::{self, PhysicsWorld};
    use abi::structs::Request;
    use std::sync::Arc;

    /// A 3x3 terrain, one unit per cell, flat at height 0, whose sample materials are 0, 1, 255...
    #[test]
    fn terrain_cells_report_their_material_surface() {
        crate::handles::fake_managed();
        let w: world::Ref = objects::create_shared(Arc::new(PhysicsWorld::default()));
        let body = super::super::body::spawn(w, physics::BodyType::Static, physics::Pose::IDENTITY, &Default::default()).unwrap();
        let surfaces = [5i32, 6, 7, 8];
        let settings = abi::structs::PhysicsShapeSettings { surface_indices: surfaces.as_ptr() as usize, surface_count: 4, surface_index: 2, friction: -1.0, elasticity: -1.0, ..Default::default() };
        let heights = [0u16; 9];
        let mut materials = [0u8, 1, 0, 3, HOLE, 0, 0, 0, 0];
        let raw = |p: *mut c_void| RawPtr::from_raw(p);
        let (g, m) = grid(raw(heights.as_ptr() as *mut _), raw(materials.as_mut_ptr().cast()), 3, 3, 10.0, 1.0).unwrap();
        add_with_slots(body, Type::HeightField, Geometry::HeightField(g), settings, m, slot_surfaces(&settings));
        let shape_ref = crate::physics::body::get(body).unwrap().shapes.lock().unwrap()[0];
        world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
        // Shape frame: sample (x, y) at (y * cell, h, x * cell); the body is unrotated.
        let down = |x: f32, y: f32| {
            let mut req = Request::default();
            (req.start_pos, req.end_pos, req.world.self_) = (vec3(y * 10.0, 50.0, x * 10.0), vec3(y * 10.0, -50.0, x * 10.0), w.addr());
            let r = trace(req);
            (r.fraction > 0.0 && r.fraction < 1.0, r.triangle_index, r.surface_property)
        };
        assert_eq!(down(0.5, 0.5), (true, 0, 5));
        assert_eq!(down(1.5, 0.5), (true, 1, 6));
        assert_eq!(down(0.5, 1.5), (true, 3, 8));
        assert!(!down(1.5, 1.5).0, "sample (1, 1) is a hole");

        let mut block = [2u8];
        update(shape_ref, raw(std::ptr::null_mut()), raw(block.as_mut_ptr().cast()), 1, 0, 1, 1, 10.0, 1.0);
        world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
        assert_eq!(down(1.5, 0.5), (true, 1, 7), "UpdateHeightShape repaints the block's materials");
    }
}
