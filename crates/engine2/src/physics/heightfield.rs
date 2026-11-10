//! Terrain: `IPhysicsBody.AddHeightFieldShape`, `IPhysicsShape.UpdateHeightShape` and
//! `g_pPhysicsSystem.CastHeightField`, over [`physics::Grid`] (which documents the frames).
//! Material 255 marks a hole. TODO: per-material surfaces (`settings.surface_indices`).

use super::shape::{self, Type, add};
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

fn grid(heights: RawPtr<c_void>, materials: RawPtr<c_void>, size_x: i32, size_y: i32, cell: f32, height_scale: f32) -> Option<Grid> {
    let (sx, sy) = dims(size_x, size_y)?;
    // SAFETY: managed passes `size_x * size_y` heights and materials (or a null materials pointer).
    let (heights, materials) = unsafe { (slice::<u16>(heights, sx * sy)?, slice::<u8>(materials, sx * sy)) };
    let mut grid = Grid::new(sx, sy, heights.to_vec(), cell, height_scale)?;
    if let Some(m) = materials {
        grid.holes = m.iter().map(|&m| m == HOLE).collect();
    }
    Some(grid)
}

#[allow(clippy::too_many_arguments)]
fn update(r: shape::Ref, heights: RawPtr<c_void>, materials: RawPtr<c_void>, x: i32, y: i32, w: i32, h: i32, cell: f32, height_scale: f32) {
    let Some(shape) = shape::get(r) else { return };
    let Geometry::HeightField(mut grid) = shape.geometry.lock().unwrap().clone() else { return };
    let (Some((x, y)), Some((w, h))) = (dims(x, y), dims(w, h)) else { return };
    (grid.cell, grid.height_scale) = (cell, height_scale);
    // SAFETY: heights cover the whole grid (Terrain passes its full map); materials only the block.
    let heights = unsafe { slice::<u16>(heights, grid.heights.len()) };
    let holes: Option<Vec<bool>> = unsafe { slice::<u8>(materials, w * h) }.map(|m| m.iter().map(|&m| m == HOLE).collect());
    grid.update((x, y, w, h), heights, holes.as_deref());
    shape::update(r, Geometry::HeightField(grid));
}

#[allow(clippy::too_many_arguments)]
fn cast(out: &mut Vector3, normal: &mut Vector3, fraction: &mut f32, start: Vector3, delta: Vector3, heights: RawPtr<c_void>, size_x: i32, size_y: i32, cell: f32, height_scale: f32) -> bool {
    let Some(grid) = grid(heights, RawPtr::from_raw(std::ptr::null_mut()), size_x, size_y, cell, height_scale) else { return false };
    let Some((p, n, t)) = grid.cast_ray(to_vec(start), to_vec(delta)) else { return false };
    (*out, *normal, *fraction) = (from_vec(p), from_vec(n), t);
    true
}

pub fn register(r: &mut Registry) {
    bind!(r, b::ADD_HEIGHT_FIELD_SHAPE, |body, heights, materials, size_x, size_y, cell, height_scale, _material_count, settings| {
        match grid(heights, materials, size_x, size_y, cell, height_scale) {
            Some(g) => add(body, Type::HeightField, Geometry::HeightField(g), settings),
            None => abi::HandleId::new(-1),
        }
    });
    bind!(r, s::UPDATE_HEIGHT_SHAPE, update);
    bind!(r, sys::CAST_HEIGHT_FIELD, cast);
}
