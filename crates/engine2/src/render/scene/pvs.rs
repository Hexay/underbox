//! PVS culling: an object of a world with a PVS (`SetPVS`) draws only when the camera's cluster
//! sees a cluster its world bounds touch. Bounds are managed's `SetBounds`, else the model's
//! render bounds moved by the transform (a point at the origin without a model).
//!
//! Managed's scene renderer culls in its own mirror and never asks; a camera render [`enter`]s
//! its eye and PVS so the occlusion cull it dispatches can drop hidden instances
//! (`compute/pvs_cull.rs`).

use crate::render::draw::model as model_draw;
use crate::scene::{object, world};
use abi::NativeRef;
use glam::Vec3;
use std::cell::RefCell;

pub fn visible(eye: Vec3, obj: &object::SceneObject, s: &object::State) -> bool {
    let pvs = world::get(obj.world).map_or(0, |w| *w.pvs.lock().unwrap());
    pvs == 0 || box_visible(eye, pvs, world_bounds(s))
}

/// Whether `eye` sees the box `lo..hi` through the PVS at address `pvs`.
pub fn box_visible(eye: Vec3, pvs: usize, (lo, hi): (Vec3, Vec3)) -> bool {
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

thread_local! {
    /// Camera renders in progress on this thread: (eye, PVS address or 0), innermost last.
    static VIEWS: RefCell<Vec<(Vec3, usize)>> = const { RefCell::new(Vec::new()) };
}

/// Ends the camera render [`enter`] began.
pub struct ViewScope(());

impl Drop for ViewScope {
    fn drop(&mut self) {
        VIEWS.with(|v| v.borrow_mut().pop());
    }
}

/// A camera render at `eye` over `worlds` (the first with a PVS counts) until the scope drops.
pub fn enter(eye: Vec3, worlds: &[world::Ref]) -> ViewScope {
    let pvs = worlds.iter().find_map(|w| world::get(*w).map(|w| *w.pvs.lock().unwrap()).filter(|p| *p != 0)).unwrap_or(0);
    VIEWS.with(|v| v.borrow_mut().push((eye, pvs)));
    ViewScope(())
}

/// The innermost camera render's eye and PVS, when it has one.
pub fn current() -> Option<(Vec3, usize)> {
    VIEWS.with(|v| v.borrow().last().copied()).filter(|(_, pvs)| *pvs != 0)
}
