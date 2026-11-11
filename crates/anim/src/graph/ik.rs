//! Pose-space solvers for the IK pose nodes. Poses are parent-relative; targets are in the
//! character's (model) space.

use crate::pose::Pose;
use crate::skeleton::Skeleton;
use crate::xform::Xform;
use glam::{Quat, Vec3};

/// Turn `bone` by the model-space rotation `r` about its own origin (its children follow).
fn turn(s: &Skeleton, pose: &mut Pose, bone: usize, r: Quat) {
    let model = s.model_space(&pose.0);
    let parent = s.bone(bone).and_then(|b| b.parent).map_or(Quat::IDENTITY, |p| model[p].rot);
    pose.0[bone].rot = (parent.inverse() * r * model[bone].rot).normalize();
}

fn angle(a: Vec3, b: Vec3) -> f32 {
    a.normalize_or_zero().dot(b.normalize_or_zero()).clamp(-1.0, 1.0).acos()
}

/// A unit normal of the chain's bend plane, with fallbacks for a straight chain.
fn bend_axis(ac: Vec3, ab: Vec3, at: Vec3) -> Vec3 {
    [ac.cross(ab), at.cross(ab), ac.any_orthonormal_vector()].into_iter().find(|v| v.length_squared() > 1e-8).unwrap_or(Vec3::Y).normalize()
}

/// Analytic two-bone IK: `effector`'s parent and grandparent turn so it reaches `target`'s
/// position, then the effector takes the target's rotation. False when the chain is missing.
pub fn two_bone(s: &Skeleton, pose: &mut Pose, effector: usize, target: &Xform) -> bool {
    let Some(mid) = s.bone(effector).and_then(|b| b.parent) else { return false };
    let Some(root) = s.bone(mid).and_then(|b| b.parent) else { return false };
    if pose.0.len() < s.len() {
        return false;
    }
    let m = s.model_space(&pose.0);
    let (a, b, c, t) = (m[root].pos, m[mid].pos, m[effector].pos, target.pos);
    let (lab, lcb) = ((b - a).length(), (c - b).length());
    let lat = (t - a).length().clamp(1e-4, (lab + lcb) * 0.9999);
    let law = |x: f32, y: f32, opp: f32| ((x * x + y * y - opp * opp) / (2.0 * x * y).max(1e-8)).clamp(-1.0, 1.0).acos();
    let (ac_ab0, ba_bc0, ac_at0) = (angle(c - a, b - a), angle(a - b, c - b), angle(c - a, t - a));
    let (ac_ab1, ba_bc1) = (law(lab, lat, lcb), law(lab, lcb, lat));
    let axis0 = bend_axis(c - a, b - a, t - a);
    let axis1 = (c - a).cross(t - a);
    turn(s, pose, root, Quat::from_axis_angle(axis0, ac_ab1 - ac_ab0));
    turn(s, pose, mid, Quat::from_axis_angle(axis0, ba_bc1 - ba_bc0));
    if axis1.length_squared() > 1e-8 {
        turn(s, pose, root, Quat::from_axis_angle(axis1.normalize(), ac_at0));
    }
    let m = s.model_space(&pose.0);
    pose.0[effector].rot = (m[mid].rot.inverse() * target.rot).normalize();
    true
}

/// Look-at: the reference frame's yaw (about its Z) and pitch (about its Y, positive down) turn,
/// shared out over `bones` (end first, cumulative weights descending).
pub fn look_at(s: &Skeleton, pose: &mut Pose, reference: Quat, yaw: f32, pitch: f32, bones: &[(usize, f32)]) {
    let turn_by = Quat::from_rotation_z(yaw.to_radians()) * Quat::from_rotation_y(pitch.to_radians());
    let full = reference * turn_by * reference.inverse();
    for (k, &(bone, w)) in bones.iter().enumerate().rev() {
        let next = bones.get(k + 1).map_or(0.0, |b| b.1);
        let share = (w - next).max(0.0);
        if share > 0.0 && bone < pose.0.len() {
            turn(s, pose, bone, Quat::IDENTITY.slerp(full, share));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight arm along +X: shoulder at the origin, elbow at 10, hand at 20.
    fn arm() -> Skeleton {
        let mut s = Skeleton::default();
        s.push_local("shoulder", None, Xform::IDENTITY);
        s.push_local("elbow", Some(0), Xform::new(Vec3::new(10.0, 0.0, 0.0), Quat::IDENTITY));
        s.push_local("hand", Some(1), Xform::new(Vec3::new(10.0, 0.0, 0.0), Quat::IDENTITY));
        s
    }

    #[test]
    fn two_bone_ik_reaches_the_target_and_keeps_bone_lengths() {
        let s = arm();
        let mut pose = Pose(s.bind_pose());
        pose.0[1].rot = Quat::from_rotation_z(0.3);
        let target = Xform::new(Vec3::new(5.0, 12.0, 3.0), Quat::from_rotation_x(1.0));
        assert!(two_bone(&s, &mut pose, 2, &target));
        let m = s.model_space(&pose.0);
        assert!(m[2].pos.abs_diff_eq(target.pos, 1e-3), "{:?}", m[2].pos);
        assert!(((m[1].pos - m[0].pos).length() - 10.0).abs() < 1e-3);
        assert!(m[2].rot.angle_between(target.rot) < 1e-3, "the hand takes the target's rotation");
        assert!(!two_bone(&s, &mut pose, 1, &target), "the elbow has no grandparent");
    }

    #[test]
    fn unreachable_targets_stretch_the_chain_toward_them() {
        let s = arm();
        let mut pose = Pose(s.bind_pose());
        two_bone(&s, &mut pose, 2, &Xform::new(Vec3::new(0.0, 0.0, 100.0), Quat::IDENTITY));
        let hand = s.model_space(&pose.0)[2].pos;
        assert!(hand.normalize().dot(Vec3::Z) > 0.999 && (hand.length() - 20.0).abs() < 0.01, "{hand:?}");
    }

    #[test]
    fn look_at_shares_the_turn_down_the_chain() {
        let s = arm();
        let mut pose = Pose(s.bind_pose());
        look_at(&s, &mut pose, Quat::IDENTITY, 90.0, 0.0, &[(2, 1.0), (1, 0.5)]);
        let m = s.model_space(&pose.0);
        assert!(m[2].rot.angle_between(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)) < 1e-3, "the end turns fully: {:?}", m[2].rot.to_axis_angle());
        assert!(m[1].rot.angle_between(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)) < 1e-4, "the elbow half");
        assert!(m[0].rot.angle_between(Quat::IDENTITY) < 1e-6);
    }
}
