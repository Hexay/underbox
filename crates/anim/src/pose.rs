//! Poses: one parent-relative transform per skeleton bone, and the ways graphs combine them.

use crate::xform::Xform;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pose(pub Vec<Xform>);

impl Pose {
    pub fn bind(skeleton: &crate::Skeleton) -> Self {
        Self(skeleton.bind_pose())
    }

    /// Every bone at identity: the additive "no change" and ZeroPose.
    pub fn zero(bones: usize) -> Self {
        Self(vec![Xform::IDENTITY; bones])
    }

    /// `self` toward `other` by `t` (0 = self).
    pub fn blend(&self, other: &Pose, t: f32) -> Pose {
        self.blend_masked(other, t, None)
    }

    /// As [`Pose::blend`], each bone's weight scaled by `mask` (missing entries weigh 0).
    pub fn blend_masked(&self, other: &Pose, t: f32, mask: Option<&[f32]>) -> Pose {
        let w = |i: usize| t * mask.map_or(1.0, |m| m.get(i).copied().unwrap_or(0.0));
        Pose(self.0.iter().zip(&other.0).enumerate().map(|(i, (a, b))| a.lerp(b, w(i))).collect())
    }

    /// Layer an additive pose (deltas from [`Pose::delta`]) on top at weight `t`.
    pub fn add(&self, additive: &Pose, t: f32) -> Pose {
        Pose(self.0.iter().zip(&additive.0).map(|(a, d)| a.add(d, t)).collect())
    }

    /// Per-bone change from `base` to `self`.
    pub fn delta(&self, base: &Pose) -> Pose {
        Pose(self.0.iter().zip(&base.0).map(|(a, b)| a.delta(b)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Quat, Vec3};

    fn pose(xs: &[f32]) -> Pose {
        Pose(xs.iter().map(|&x| Xform::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY)).collect())
    }

    #[test]
    fn blend_and_mask() {
        let (a, b) = (pose(&[0.0, 0.0]), pose(&[10.0, 10.0]));
        assert_eq!(a.blend(&b, 0.25).0[1].pos.x, 2.5);
        let masked = a.blend_masked(&b, 1.0, Some(&[1.0]));
        assert_eq!((masked.0[0].pos.x, masked.0[1].pos.x), (10.0, 0.0));
    }

    #[test]
    fn additive_layers() {
        let (base, target) = (pose(&[1.0]), pose(&[4.0]));
        let d = target.delta(&base);
        assert_eq!(pose(&[10.0]).add(&d, 0.5).0[0].pos.x, 11.5);
    }
}
