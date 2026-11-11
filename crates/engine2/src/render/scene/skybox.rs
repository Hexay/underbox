//! The 3D skybox (`CRenderPipelineSetup::Add3DSkybox`): a child view drawing the skybox world a
//! world names (`Add3DSkyboxWorld`, `Set3DSkyboxParameters`, kept by crate::world::pvs) from the
//! camera's position in skybox space, run as one layer of its parent after the opaque geometry.
//! Managed's `Skybox3DPipeline` adds its layers (callback 35); without it the view gets sky and
//! opaque layers.
//!
//! Skybox depth is squeezed into `[DEPTH_NEAR, 1]` so it lands behind everything the parent
//! drew but still sorts against itself (native's skybox range, in our forward depth).

use super::{field, layer, setup, view};
use crate::render::texture;
use crate::scene::world::{self, Skybox};
use abi::callbacks::sandbox_rendering_render_pipeline as pipeline;
use abi::slots::{c_render_pipeline_setup as s, i_scene_view as v};
use abi::{NativeRef, StringToken};
use glam::{Mat4, Vec3, Vec4};

const DEPTH_NEAR: f32 = 0.9999;
const SKYBOX_LAYERS_CALLBACK: usize = 35;

/// Main-world position of skybox-space `q` is `scale * (q - origin)` (`SkyboxToWorld`).
fn skybox_to_world(sky: &Skybox) -> Mat4 {
    let origin = crate::math::v3(sky.origin);
    Mat4::from_scale(Vec3::splat(sky.scale)) * Mat4::from_translation(-origin)
}

/// Clip-space z moved into `[DEPTH_NEAR, 1]`.
fn squeeze_depth() -> Mat4 {
    Mat4::from_cols(Vec4::X, Vec4::Y, Vec4::new(0.0, 0.0, 1.0 - DEPTH_NEAR, 0.0), Vec4::new(0.0, 0.0, DEPTH_NEAR, 1.0))
}

/// The first of `worlds` with a skybox to draw, unless `r_3d_skybox` is off.
fn skybox_of(worlds: &[world::Ref]) -> Option<Skybox> {
    if crate::convar::find("r_3d_skybox").is_some_and(|c| c.get_int() == 0) {
        return None;
    }
    worlds.iter().filter_map(|w| world::get(*w)).map(|w| *w.skybox.lock().unwrap()).find(|s| s.world != 0 && s.scale > 0.0)
}

/// A view of `sky`'s world through `parent`'s camera, drawing into `color`/`depth`.
fn child_view(parent: view::Ref, p: &view::SceneView, sky: &Skybox, color: texture::Ref, depth: texture::Ref) -> view::Ref {
    let mut camera = p.camera;
    camera.origin = crate::math::v3(sky.origin) + camera.origin / sky.scale;
    camera.near /= sky.scale;
    camera.far /= sky.scale;
    camera.ortho = camera.ortho.map(|(w, h)| (w / sky.scale, h / sky.scale));
    let view_proj = squeeze_depth() * p.view_proj * skybox_to_world(sky);
    let desc = view::Desc {
        camera,
        proj: Some(view_proj * camera.view().inverse()),
        viewport: p.viewport,
        swap_chain: p.swap_chain,
        color,
        depth,
        worlds: vec![NativeRef::from_raw(sky.world as *mut _)],
        render_tags: Vec::new(),
        exclude_tags: Vec::new(),
        post_process: false,
        ambient: p.ambient,
        environment: view::Environment { tone_map: p.environment.tone_map, fog: p.environment.skybox_fog.clone(), skybox_fog: None },
        unique_id: field!(get i32, v::GET_M_VIEW_UNIQUE_ID, parent.addr()),
        managed_camera_id: field!(get i32, v::GET_M_MANAGED_CAMERA_ID, parent.addr()),
    };
    let child = view::create(desc);
    view::with(child, |_, st| st.parent = parent);
    child
}

/// Sky then opaque, into the view's targets without clearing them.
fn default_layers(child: view::Ref, vp: abi::structs::RenderViewport) {
    let forward = || layer::Kind::Render(StringToken::of("Forward"));
    let sky = view::add_layer(child, layer::create(child, "3DSkybox Sky".into(), vp, forward(), 0, 0), Default::default());
    layer::with(sky, |s| s.match_id = StringToken::of("SkyBox").0);
    view::add_layer(child, layer::create(child, "3DSkyboxForward".into(), vp, forward(), crate::scene::object::IS_OPAQUE, 0), Default::default());
}

/// Add `parent`'s skybox layer, drawing into `color`/`depth`; false when its worlds have none.
/// `managed` is the parent's setup, handed on to managed's skybox pipeline.
pub fn add(parent: view::Ref, color: texture::Ref, depth: texture::Ref, managed: Option<setup::Ref>) -> bool {
    let Some(p) = view::get(parent) else { return false };
    let worlds = p.state.lock().unwrap().worlds.clone();
    let Some(sky) = skybox_of(&worlds) else { return false };
    let child = child_view(parent, &p, &sky, color, depth);
    if let Some(parent_setup) = managed.filter(|_| abi::rt::callbacks::is_set(SKYBOX_LAYERS_CALLBACK)) {
        let attributes = field!(get NativeRef<abi::class::CRenderAttributes>, s::GET_M_P_ATTRIBUTES, parent_setup.addr());
        let desc = setup::Desc { attributes, viewport: p.viewport, clear_flags: 0, clear_color: Default::default(), tools_view: false };
        let child_setup = setup::create(child, &desc);
        pipeline::add_skybox3_d_layers_to_view(child_setup);
        objects::destroy(child_setup);
    }
    if view::with(child, |_, st| st.layers.is_empty()) {
        default_layers(child, p.viewport);
    }
    view::add_layer(parent, layer::create(parent, "3DSkybox".into(), p.viewport, layer::Kind::View(child), 0, 0), Default::default());
    true
}

/// `Add3DSkybox`: the skybox draws into the setup's current colour and depth targets.
pub fn add_for_setup(r: setup::Ref) -> bool {
    let a = r.addr();
    let color = view::rt_texture(field!(get view::RtHandle, s::GET_M_H_COLOR, a));
    let depth = view::rt_texture(field!(get view::RtHandle, s::GET_M_H_DEPTH, a));
    add(setup::view_of(r), color, depth, Some(r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skybox_space_maps_into_the_main_world_and_depth_stays_far() {
        let sky = Skybox { world: 1, origin: crate::math::vec3(10.0, 0.0, -5.0), scale: 16.0, ..Default::default() };
        let p = skybox_to_world(&sky).transform_point3(Vec3::new(11.0, 0.0, -5.0));
        assert!(p.abs_diff_eq(Vec3::new(16.0, 0.0, 0.0), 1e-4), "{p}");
        let squeezed = |z: f32| (squeeze_depth() * Vec4::new(0.0, 0.0, z, 1.0)).z;
        assert_eq!((squeezed(0.0), squeezed(1.0)), (DEPTH_NEAR, 1.0));
    }
}
