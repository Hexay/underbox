//! `SceneCullingBox`: oriented boxes per world (`AddCullingBox`) that hide objects from the
//! world's camera views (collect.rs; shadow views still see them). An inside box hides objects
//! wholly inside it; once a world has outside boxes, objects touching none of them are hidden.
//!
//! Managed passes its `Size` (times scale) as the box's full size. "Touching" tests the
//! separating axes of both boxes' faces only, so an object near a box edge may count as touching.

use crate::frustum::camera::basis;
use abi::structs::{Angles, Vector3};
use glam::Vec3;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

struct CullingBox {
    id: u32,
    inside: bool,
    center: Vec3,
    axes: [Vec3; 3],
    half: Vec3,
}

impl CullingBox {
    /// Along axis `i`: the distance between the centers and the radius of the box `center ± extents`.
    fn along(&self, i: usize, center: Vec3, extents: Vec3) -> (f32, f32) {
        let a = self.axes[i];
        ((center - self.center).dot(a).abs(), extents.dot(a.abs()))
    }

    fn contains(&self, center: Vec3, extents: Vec3) -> bool {
        (0..3).all(|i| {
            let (d, r) = self.along(i, center, extents);
            d + r <= self.half[i]
        })
    }

    fn touches(&self, center: Vec3, extents: Vec3) -> bool {
        let own = (0..3).all(|i| {
            let (d, r) = self.along(i, center, extents);
            d <= self.half[i] + r
        });
        let reach = self.axes.iter().zip(self.half.to_array()).fold(Vec3::ZERO, |acc, (a, h)| acc + a.abs() * h);
        own && ((center - self.center).abs() - extents).cmple(reach).all()
    }
}

static BOXES: LazyLock<Mutex<HashMap<usize, Vec<CullingBox>>>> = LazyLock::new(Default::default);

pub fn add(world: usize, id: u32, inside: bool, origin: Vector3, angles: Angles, size: Vector3) {
    let (forward, left, up) = basis(Vec3::new(angles.pitch, angles.yaw, angles.roll));
    let b = CullingBox { id, inside, center: crate::math::v3(origin), axes: [forward, left, up], half: crate::math::v3(size).abs() * 0.5 };
    BOXES.lock().unwrap().entry(world).or_default().push(b);
}

pub fn remove(world: usize, id: u32) {
    if let Some(list) = BOXES.lock().unwrap().get_mut(&world) {
        list.retain(|b| b.id != id);
    }
}

/// Whether `world`'s boxes leave the world-space box `lo..hi` visible.
pub fn visible(world: usize, (lo, hi): (Vec3, Vec3)) -> bool {
    let boxes = BOXES.lock().unwrap();
    let Some(list) = boxes.get(&world) else { return true };
    let (center, extents) = ((lo + hi) * 0.5, ((hi - lo) * 0.5).max(Vec3::ZERO));
    if list.iter().any(|b| b.inside && b.contains(center, extents)) {
        return false;
    }
    let mut outside = list.iter().filter(|b| !b.inside).peekable();
    outside.peek().is_none() || outside.any(|b| b.touches(center, extents))
}

#[cfg(test)]
#[path = "culling_box_tests.rs"]
mod tests;
