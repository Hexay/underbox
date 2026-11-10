//! Bone transforms: translation, rotation and uniform scale (Source 2 bones scale uniformly,
//! so composition stays closed; managed's `Transform.Scale` vector is read as its x).

use glam::{Quat, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xform {
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: f32,
}

impl Default for Xform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Xform {
    pub const IDENTITY: Self = Self { pos: Vec3::ZERO, rot: Quat::IDENTITY, scale: 1.0 };

    pub fn new(pos: Vec3, rot: Quat) -> Self {
        Self { pos, rot, scale: 1.0 }
    }

    /// `child` (relative to `self`) taken into `self`'s parent space.
    pub fn mul(&self, child: &Xform) -> Xform {
        Xform { pos: self.pos + self.rot * (child.pos * self.scale), rot: (self.rot * child.rot).normalize(), scale: self.scale * child.scale }
    }

    pub fn inverse(&self) -> Xform {
        let rot = self.rot.conjugate();
        let scale = if self.scale == 0.0 { 0.0 } else { 1.0 / self.scale };
        Xform { pos: rot * (-self.pos * scale), rot, scale }
    }

    /// `other` expressed relative to `self`: `self.mul(&self.to_local(o)) == o`.
    pub fn to_local(&self, other: &Xform) -> Xform {
        self.inverse().mul(other)
    }

    pub fn point(&self, p: Vec3) -> Vec3 {
        self.pos + self.rot * (p * self.scale)
    }

    /// Linear position/scale, shortest-arc normalised lerp for rotation.
    pub fn lerp(&self, other: &Xform, t: f32) -> Xform {
        Xform { pos: self.pos.lerp(other.pos, t), rot: nlerp(self.rot, other.rot, t), scale: self.scale + (other.scale - self.scale) * t }
    }

    /// The change from `base` to `self`, for additive layers: `base.add(&self.delta(base)) == self`.
    pub fn delta(&self, base: &Xform) -> Xform {
        let scale = if base.scale == 0.0 { 1.0 } else { self.scale / base.scale };
        Xform { pos: self.pos - base.pos, rot: (base.rot.conjugate() * self.rot).normalize(), scale }
    }

    /// Apply an additive `delta` at weight `w`.
    pub fn add(&self, delta: &Xform, w: f32) -> Xform {
        let d = Xform::IDENTITY.lerp(delta, w);
        Xform { pos: self.pos + d.pos, rot: (self.rot * d.rot).normalize(), scale: self.scale * d.scale }
    }
}

pub fn nlerp(a: Quat, b: Quat, t: f32) -> Quat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    (a * (1.0 - t) + b * t).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    pub fn close(a: &Xform, b: &Xform) -> bool {
        a.pos.abs_diff_eq(b.pos, 1e-4) && (a.rot.dot(b.rot).abs() - 1.0).abs() < 1e-5 && (a.scale - b.scale).abs() < 1e-5
    }

    #[test]
    fn compose_and_invert() {
        let parent = Xform { pos: Vec3::new(10.0, 0.0, 0.0), rot: Quat::from_rotation_z(FRAC_PI_2), scale: 2.0 };
        let child = Xform::new(Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY);
        let world = parent.mul(&child);
        assert!(world.pos.abs_diff_eq(Vec3::new(10.0, 2.0, 0.0), 1e-5));
        assert!(close(&parent.mul(&parent.to_local(&world)), &world));
        assert!(close(&parent.mul(&parent.inverse()), &Xform::IDENTITY));
    }

    #[test]
    fn lerp_takes_shortest_arc() {
        let a = Xform::new(Vec3::ZERO, Quat::from_rotation_z(0.1));
        let b = Xform::new(Vec3::splat(2.0), -Quat::from_rotation_z(0.3));
        let mid = a.lerp(&b, 0.5);
        assert!(mid.pos.abs_diff_eq(Vec3::ONE, 1e-6));
        assert!(mid.rot.abs_diff_eq(Quat::from_rotation_z(0.2), 1e-4) || mid.rot.abs_diff_eq(-Quat::from_rotation_z(0.2), 1e-4));
    }

    #[test]
    fn additive_round_trip() {
        let base = Xform { pos: Vec3::X, rot: Quat::from_rotation_y(0.4), scale: 1.5 };
        let target = Xform { pos: Vec3::Y, rot: Quat::from_rotation_x(0.7), scale: 3.0 };
        assert!(close(&base.add(&target.delta(&base), 1.0), &target));
        assert!(close(&base.add(&target.delta(&base), 0.0), &base));
    }
}
