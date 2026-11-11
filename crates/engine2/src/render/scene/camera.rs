//! `CCameraRenderer`: managed configures one per camera render (worlds, tags, attributes and the
//! generated camera fields), then renders it to a swap chain or texture. A render builds a view,
//! lets managed's pipeline add layers (`RenderPipeline.AddLayersToView`), runs it, and frees it.
//! Rendering is eager: the view runs inside the call (see system.rs).

use super::{execute, field, setup, view};
use crate::frustum::camera::Camera;
use crate::render::{attributes, swap_chain, texture};
use crate::scene::world;
use abi::callbacks::sandbox_rendering_render_pipeline as pipeline;
use abi::slots::c_camera_renderer as c;
use abi::structs::{Angles, Matrix, NativeRect, RenderViewport, Vector3, Vector4};
use abi::{NativeRef, RawPtr, Registry, StringToken, bind, class};
use glam::{Mat4, Vec3, Vec4};
use std::ffi::{CStr, c_void};
use std::sync::Mutex;

pub type Ref = NativeRef<class::CCameraRenderer>;

#[derive(Default)]
pub struct CameraRenderer {
    pub camera_id: i32,
    pub worlds: Vec<world::Ref>,
    pub attributes: attributes::Ref,
    pub render_tags: Vec<u32>,
    pub exclude_tags: Vec<u32>,
}

fn with<R: Default>(r: Ref, f: impl FnOnce(&mut CameraRenderer) -> R) -> R {
    objects::get::<_, Mutex<CameraRenderer>>(r).map(|c| f(&mut c.lock().unwrap())).unwrap_or_default()
}

pub(crate) fn create(_name: Option<&CStr>, camera_id: i32) -> Ref {
    objects::create(Mutex::new(CameraRenderer { camera_id, ..Default::default() }))
}

/// `numerics` holds a row-major (column-vector) matrix; see frustum/mod.rs.
pub(crate) fn mat4(m: Matrix) -> Mat4 {
    let n = m.numerics;
    Mat4::from_cols(Vec4::new(n.m11, n.m21, n.m31, n.m41), Vec4::new(n.m12, n.m22, n.m32, n.m42), Vec4::new(n.m13, n.m23, n.m33, n.m43), Vec4::new(n.m14, n.m24, n.m34, n.m44))
}

/// The camera and pixel viewport the renderer's fields describe, for a `width × height` target.
pub(super) fn camera(r: Ref, width: u32, height: u32) -> (Camera, Option<Mat4>, RenderViewport) {
    let a = r.addr();
    let vp: Vector4 = field!(get Vector4, c::GET_VIEWPORT, a);
    let v = if vp.vec.z > 0.0 && vp.vec.w > 0.0 { vp.vec } else { abi::structs::System_Numerics_Vector4 { x: 0.0, y: 0.0, z: 1.0, w: 1.0 } };
    let (w, h) = (width as f32, height as f32);
    let viewport = RenderViewport {
        m_n_top_left_x: (v.x * w) as i32,
        m_n_top_left_y: (v.y * h) as i32,
        m_n_width: ((v.z * w) as i32).max(1),
        m_n_height: ((v.w * h) as i32).max(1),
        max_z: 1.0,
        ..Default::default()
    };
    let aspect = viewport.m_n_width as f32 / viewport.m_n_height as f32;
    let pos: Vector3 = field!(get Vector3, c::GET_CAMERA_POSITION, a);
    let ang: Angles = field!(get Angles, c::GET_CAMERA_ROTATION, a);
    let mut cam = Camera { origin: crate::math::v3(pos), aspect, ..Camera::default() };
    cam.set_angles(Vec3::new(ang.pitch, ang.yaw, ang.roll));
    let (near, far, fov): (f32, f32, f32) = (field!(get f32, c::GET_Z_NEAR, a), field!(get f32, c::GET_Z_FAR, a), field!(get f32, c::GET_FIELD_OF_VIEW, a));
    cam.near = if near > 0.0 { near } else { 1.0 };
    cam.far = if far > cam.near { far } else { cam.near + 10_000.0 };
    cam.fov = if fov > 0.0 { fov } else { 90.0 };
    cam.size = (viewport.m_n_width as f32, viewport.m_n_height as f32);
    if field!(get bool, c::GET_ORTHO, a) {
        let units_per_pixel: f32 = field!(get f32, c::GET_ORTHO_SIZE, a);
        let height = units_per_pixel * viewport.m_n_height as f32;
        cam.ortho = Some((height * aspect, height));
    }
    let bounds: Vector4 = field!(get Vector4, c::GET_CLIP_SPACE_BOUNDS, a);
    if bounds.vec.z > bounds.vec.x && bounds.vec.w > bounds.vec.y {
        cam.window = [bounds.vec.x, bounds.vec.y, bounds.vec.z, bounds.vec.w];
    }
    let mut proj = field!(get bool, c::GET_HAS_OVERRIDE_PROJECTION, a).then(|| mat4(field!(get Matrix, c::GET_OVERRIDE_PROJECTION, a)));
    let flip = Vec3::new(if field!(get bool, c::GET_FLIP_X, a) { -1.0 } else { 1.0 }, if field!(get bool, c::GET_FLIP_Y, a) { -1.0 } else { 1.0 }, 1.0);
    if flip != Vec3::ONE {
        proj = Some(Mat4::from_scale(flip) * proj.unwrap_or_else(|| cam.proj()));
    }
    (cam, proj, viewport)
}

/// Render into `color` / `depth` (sized `width × height`), recording what managed reads back.
pub fn render(r: Ref, swap_chain: swap_chain::Handle, color: texture::Ref, depth: texture::Ref, width: u32, height: u32) -> bool {
    let Some(renderer) = objects::get::<_, Mutex<CameraRenderer>>(r) else { return false };
    let (camera_id, worlds, attrs, render_tags, exclude_tags) = {
        let c = renderer.lock().unwrap();
        (c.camera_id, c.worlds.clone(), c.attributes, c.render_tags.clone(), c.exclude_tags.clone())
    };
    let (cam, proj, viewport) = camera(r, width, height);
    let a = r.addr();
    let _pvs = super::pvs::enter(cam.origin, &worlds);
    let desc = view::Desc {
        camera: cam,
        proj,
        viewport,
        swap_chain,
        color,
        depth,
        worlds,
        render_tags,
        exclude_tags,
        post_process: field!(get bool, c::GET_ENABLE_POSTPROCESSING, a),
        ambient: attributes::vector4(attrs, StringToken::of("ambientColor")).map_or(Vec4::ZERO, |c| Vec4::new(c.vec.x, c.vec.y, c.vec.z, c.vec.w)),
        environment: environment(attrs),
        unique_id: field!(get i32, c::GET_VIEW_UNIQUE_ID, a),
        managed_camera_id: camera_id,
    };
    let v = view::create(desc);
    // Managed's `BackgroundColor` is gamma space; the view clears its linear target with it.
    let gamma = attributes::vector4(attrs, StringToken::of("clearColor")).map_or(Vec4::new(0.0, 0.0, 0.0, 1.0), |c| Vec4::new(c.vec.x, c.vec.y, c.vec.z, c.vec.w));
    let d = render::srgb::decode;
    let clear_color = crate::math::vec4(d(gamma.x), d(gamma.y), d(gamma.z), gamma.w);
    let clear_flags = attributes::int(attrs, StringToken::of("clearFlags")).unwrap_or(setup::CLEAR_ALL);
    let tools_view = field!(get i64, c::GET_SCENE_VIEW_FLAGS, a) & 2 != 0;
    let s = setup::create(v, &setup::Desc { attributes: attrs, viewport, clear_flags, clear_color, tools_view });
    if abi::rt::callbacks::is_set(34) && camera_id != 0 {
        pipeline::add_layers_to_view(s);
    }
    if view::with(v, |_, st| st.layers.is_empty()) {
        execute::default_layers(v, clear_flags, clear_color);
    }
    execute::run(v);
    if abi::rt::callbacks::is_set(36) && camera_id != 0 {
        pipeline::on_scene_view_submitted(v);
    }
    record(a, &view::get(v).map(|x| x.camera).unwrap_or_default(), viewport);
    objects::destroy(s);
    view::destroy(v);
    true
}

/// The exposure (one frame update per render) and fog the camera's attributes carry.
fn environment(attrs: attributes::Ref) -> view::Environment {
    let tonemap = attributes::pointer(attrs, StringToken::of("ITonemapSystem"));
    view::Environment {
        tone_map: tonemap.map(|t| crate::render::exposure::frame_update(NativeRef::from_raw(t as *mut _), true)),
        fog: attributes::constant_buffer(attrs, "FogConstants"),
        skybox_fog: attributes::constant_buffer(attrs, "SkyboxFogConstants"),
        view_fog: Some(std::sync::Arc::new(crate::render::fog::gather(attrs))),
    }
}

/// `RenderedProjectionToView`, `RenderedViewport` and `RenderedDepthRange`, as managed reads them
/// after `RenderToTexture`.
fn record(a: usize, cam: &Camera, vp: RenderViewport) {
    let inv = matrix(cam.proj().inverse());
    field!(set Matrix, c::GET_RENDERED_PROJECTION_TO_VIEW, a, inv);
    let v = |x: i32| x as f32;
    field!(set Vector4, c::GET_RENDERED_VIEWPORT, a, crate::math::vec4(v(vp.m_n_top_left_x), v(vp.m_n_top_left_y), v(vp.m_n_width), v(vp.m_n_height)));
    field!(set Vector4, c::GET_RENDERED_DEPTH_RANGE, a, crate::math::vec4(cam.near, cam.far, 0.0, 1.0));
}

/// A glam matrix in `Matrix`'s row-major layout (the inverse of [`mat4`]).
pub fn matrix(m: Mat4) -> Matrix {
    let r = m.transpose().to_cols_array();
    let mut out = Matrix::default();
    let n = &mut out.numerics;
    [n.m11, n.m12, n.m13, n.m14, n.m21, n.m22, n.m23, n.m24, n.m31, n.m32, n.m33, n.m34, n.m41, n.m42, n.m43, n.m44] = r;
    out
}

fn target_size(t: texture::Ref) -> Option<(u32, u32)> {
    texture::resolve(t).and_then(|t| t.gpu.as_ref().map(|g| (g.desc.width, g.desc.height)))
}

fn render_to_swap_chain(r: Ref, chain: swap_chain::Handle) {
    let Some((color, depth)) = swap_chain::targets(chain) else { return };
    let Some((w, h)) = target_size(color) else { return };
    render(r, chain, color, depth, w, h);
}

pub(crate) fn render_to_texture(r: Ref, color: texture::Ref, _parent: view::Ref, depth: texture::Ref) -> bool {
    let Some((w, h)) = target_size(color).or_else(|| target_size(depth)) else { return false };
    if !depth.is_null() {
        return render(r, swap_chain::Handle::default(), color, depth, w, h);
    }
    let scratch = ScratchDepth::lease(w, h);
    render(r, swap_chain::Handle::default(), color, scratch.0, w, h)
}

static FREE_DEPTHS: Mutex<Vec<((u32, u32), texture::Ref)>> = Mutex::new(Vec::new());

/// A depth target for a render managed gives none for, pooled by size across frames. Leased, not
/// shared: concurrent renders of one size would otherwise draw into each other's depth between
/// a clear and the sky.
struct ScratchDepth(texture::Ref, (u32, u32));

impl ScratchDepth {
    fn lease(w: u32, h: u32) -> ScratchDepth {
        let mut free = FREE_DEPTHS.lock().unwrap();
        let t = match free.iter().position(|(s, _)| *s == (w, h)) {
            Some(i) => free.swap_remove(i).1,
            None => texture::create_target("camera depth", w, h, 62),
        };
        ScratchDepth(t, (w, h))
    }
}

impl Drop for ScratchDepth {
    fn drop(&mut self) {
        FREE_DEPTHS.lock().unwrap().push((self.1, self.0));
    }
}

#[cfg(test)]
#[test]
fn concurrent_renders_never_share_a_scratch_depth() {
    // An odd size no other test renders at, so the pool's state is this test's.
    let (a, b) = (ScratchDepth::lease(37, 23), ScratchDepth::lease(37, 23));
    assert_ne!(a.0, b.0, "a leased target isn't handed out twice");
    let first = a.0;
    drop(a);
    assert_eq!(ScratchDepth::lease(37, 23).0, first, "a returned target is reused");
}

/// Render to a scratch RGBA8 target and copy it into managed's `w × h` bitmap.
pub(crate) fn render_to_bitmap(r: Ref, data: RawPtr<c_void>, w: i32, h: i32, bpp: i32) {
    if data.is_null() || w <= 0 || h <= 0 || bpp != 4 {
        return;
    }
    let color = texture::create_target("bitmap", w as u32, h as u32, 0);
    let depth = ScratchDepth::lease(w as u32, h as u32);
    if render(r, swap_chain::Handle::default(), color, depth.0, w as u32, h as u32)
        && let Some((frame, _)) = crate::render::readback::read(color, NativeRect::default(), 0)
    {
        // SAFETY: managed passes a pinned w * h * bpp byte bitmap (SceneCamera.RenderToBitmap).
        unsafe { data.cast::<u8>().as_mut_slice(frame.pixels.len()) }.copy_from_slice(&frame.pixels);
    }
    crate::resource::destroy_handle(color);
}

pub fn register(r: &mut Registry) {
    bind!(r, c::CREATE, create);
    bind!(r, c::DELETE_THIS, |r| {
        objects::destroy(r);
    });
    bind!(r, c::CLEAR_SCENE_WORLDS, |r| with(r, |c| c.worlds.clear()));
    bind!(r, c::ADD_SCENE_WORLD, |r, w| with(r, |c| c.worlds.push(w)));
    bind!(r, c::SET_RENDER_ATTRIBUTES, |r, a| with(r, |c| c.attributes = a));
    bind!(r, c::CLEAR_RENDER_TAGS, |r| with(r, |c| c.render_tags.clear()));
    bind!(r, c::CLEAR_EXCLUDE_TAGS, |r| with(r, |c| c.exclude_tags.clear()));
    bind!(r, c::ADD_RENDER_TAG, |r, t| with(r, |c| c.render_tags.push(t)));
    bind!(r, c::ADD_EXCLUDE_TAG, |r, t| with(r, |c| c.exclude_tags.push(t)));
    bind!(r, c::RENDER, render_to_swap_chain);
    bind!(r, c::RENDER_TO_TEXTURE, render_to_texture);
    bind!(r, c::RENDER_TO_BITMAP, render_to_bitmap);
    // TODO: cube faces (six views into array slices) and stereo (OpenXR, M3).
    bind!(r, c::RENDER_TO_CUBE_TEXTURE, |_, _, _| {});
    bind!(r, c::RENDER_STEREO, |_, _, _, _, _| {});
    bind!(r, c::SUBMIT_STEREO, |_, _, _| {});
    bind!(r, c::BLIT_STEREO, |_, _, _| {});
    // Engine overlays (console, debug text) draw natively in s&box; ours has none to add.
    bind!(r, c::RENDER_OVERLAY, |_| {});
}
