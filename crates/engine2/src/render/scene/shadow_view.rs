//! `CSceneSystem::AddShadowView`: managed's shadow mapper (ShadowMapper.Native.cs) asks for one per
//! sun cascade and local-light face while ours calls it from the view's lighting
//! (render/lighting/shadow.rs). Native queues a child view; ours draws it at once, before the
//! parent's layers: the parent's casters inside the shadow frustum, depth only, back faces culled,
//! managed's rasterizer bias, one instanced draw per mesh.

use super::view;
use crate::frustum::camera::Camera;
use crate::render::depth;
use crate::render::draw::{self, Target, caster, model as model_draw};
use crate::render::lighting::Lighting;
use crate::render::texture;
use crate::scene::object;
use abi::structs::RenderViewport;
use abi::{NativeRef, class};
use glam::Mat4;
use render::RasterState;
use render::pass::Load;
use std::sync::Mutex;

/// What one shadow view draws: `required` and `excluded` are `SceneObjectFlags` masks.
pub struct ShadowView {
    pub parent: view::Ref,
    pub frustum: NativeRef<class::CFrustum>,
    pub viewport: RenderViewport,
    pub target: texture::Ref,
    pub slice: u32,
    pub required: i64,
    pub excluded: i64,
    pub bias: (i32, f32),
}

/// The shadow camera's world → clip, as managed reads it back (`GetReverseZViewProjTranspose`).
fn camera(frustum: NativeRef<class::CFrustum>) -> Option<Camera> {
    objects::get::<_, Mutex<Camera>>(frustum).map(|c| *c.lock().unwrap())
}

fn view_proj(c: &Camera) -> Mat4 {
    let p = c.proj();
    if depth::SHADOW.is_reverse() { Camera::reverse_z(p) * c.view() } else { p * c.view() }
}

/// The parent's shadow casters inside `c`, with the view's flag masks.
fn casters(v: &ShadowView, c: &Camera, view_proj: Mat4) -> Vec<draw::Draw> {
    let worlds = view::with(v.parent, |_, s| s.worlds.clone());
    let mut draws = Vec::new();
    for (r, obj) in object::refs_in_worlds(&worlds) {
        let deform = model_draw::Deform::of(r);
        let s = obj.state.lock().unwrap();
        let flags = s.flags;
        let wanted = flags & v.required == v.required && flags & v.excluded == 0;
        if !(s.rendering && obj.light.is_none() && deform.casts_shadow() && wanted) {
            continue;
        }
        let b = s.world_bounds();
        if c.intersects_box(crate::math::v3(b.mins), crate::math::v3(b.maxs)) {
            draws.extend(model_draw::shadow_draws(&s, &deform, view_proj));
        }
    }
    draws
}

/// Draw `v` into its target slice: cleared, then every caster.
pub fn render(v: &ShadowView) {
    let Some(c) = camera(v.frustum) else { return };
    let view_proj = view_proj(&c);
    let state = RasterState { cull: Some(render::wgpu::Face::Back), depth_compare: depth::SHADOW.compare(), depth_bias: depth::SHADOW.bias(v.bias.0, v.bias.1), ..Default::default() };
    let draws: Vec<_> = casters(v, &c, view_proj).into_iter().map(|d| draw::Draw { state: RasterState { topology: d.state.topology, ..state }, ..d }).collect();
    let target = Target { depth: v.target, depth_slice: v.slice, viewport: v.viewport, reverse_z: depth::SHADOW.is_reverse(), ..Default::default() };
    draw::execute(&target, &Lighting::default(), Load { color: None, depth: Some(depth::SHADOW.clear()) }, &caster::instanced(draws, view_proj));
}
