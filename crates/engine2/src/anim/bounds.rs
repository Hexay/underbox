//! `m_localBounds`/`m_worldBounds` of an animatable object, as real: each mesh's per-bone box
//! (`assets::compiled::rig::BoneBounds`) carried by its bone's current transform, then boxed in
//! object or world space. Models without per-bone boxes report their render bounds.

use super::object::{AnimState, Ref, object_xform, with};
use abi::structs::BBox;
use anim::Xform;
use assets::compiled::rig::BoneBounds;
use glam::Vec3;

/// The boxes posed by `s.world`, each bone's transform first mapped by `space`; `None` if no
/// box names a bone of the skeleton.
pub(super) fn posed(s: &AnimState, boxes: &[BoneBounds], space: impl Fn(&Xform) -> Xform) -> Option<BBox> {
    let corners = boxes.iter().filter_map(|b| {
        let bone = space(s.world.get(s.skeleton().find(&b.bone)?)?);
        let (c, h) = (Vec3::from(b.center), Vec3::from(b.half));
        Some((0..8).map(move |i| bone.point(c + h * Vec3::new([-1.0, 1.0][i & 1], [-1.0, 1.0][(i >> 1) & 1], [-1.0, 1.0][i >> 2]))))
    });
    let (lo, hi) = corners.flatten().fold((Vec3::MAX, Vec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
    lo.cmple(hi).all().then(|| BBox { mins: crate::math::from_v3(lo), maxs: crate::math::from_v3(hi) })
}

fn of(r: Ref, space: impl Fn(&Xform) -> Xform) -> Option<BBox> {
    let model = crate::model::get(with(r, |s| s.model))?;
    with(r, |s| posed(s, &model.content.bone_bounds, space))
}

pub fn local(r: Ref) -> BBox {
    let object = object_xform(r);
    of(r, |w| object.to_local(w)).unwrap_or_else(|| crate::model::bounds(with(r, |s| s.model)))
}

pub fn world(r: Ref) -> BBox {
    of(r, |w| *w).unwrap_or_else(|| crate::scene::object::with(r.cast(), |s| s.world_bounds()))
}
