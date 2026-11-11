//! PVS culling: an object of a world with a PVS (`SetPVS`) draws only when the camera's cluster
//! sees a cluster its world bounds touch. Bounds are managed's `SetBounds`, else the model's
//! render bounds moved by the transform (a point at the origin without a model).

use crate::render::draw::model as model_draw;
use crate::scene::{object, world};
use abi::NativeRef;
use glam::Vec3;

pub fn visible(eye: Vec3, obj: &object::SceneObject, s: &object::State) -> bool {
    let pvs = world::get(obj.world).map_or(0, |w| *w.pvs.lock().unwrap());
    if pvs == 0 {
        return true;
    }
    let (lo, hi) = world_bounds(s);
    crate::world::pvs::box_visible_from(NativeRef::from_raw(pvs as *mut _), eye, lo, hi)
}

fn world_bounds(s: &object::State) -> (Vec3, Vec3) {
    let v = crate::math::v3;
    let (lo, hi) = (v(s.bounds.mins), v(s.bounds.maxs));
    if hi.cmpgt(lo).any() {
        return (lo, hi);
    }
    let b = crate::model::bounds(s.model);
    let (a, b) = (v(b.mins), v(b.maxs));
    let m = model_draw::matrix(&s.transform);
    let corners = (0..8).map(|i| m.transform_point3(Vec3::new([a.x, b.x][i & 1], [a.y, b.y][(i >> 1) & 1], [a.z, b.z][i >> 2])));
    corners.fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)))
}
