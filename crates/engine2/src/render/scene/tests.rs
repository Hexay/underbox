//! A scene drawn the way a client frame draws it, without managed: a world with a model object and
//! a sky, a camera renderer configured through its fields, rendered to an offscreen swap chain
//! with the default layers, then presented and read back.

use super::{camera, field};
use crate::handles::Kind;
use crate::model::content::Content;
use crate::render::{device, swap_chain, texture};
use crate::scene::object::{self, State};
use crate::scene::world::{self, SceneWorld};
use abi::slots::c_camera_renderer as c;
use abi::structs::{Angles, RenderDeviceInfo_t, Transform, Vector3};
use abi::{RawPtr, class};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

const SIZE: u32 = 64;

extern "C" fn register_handle(_: *mut c_void, _: u32) -> i32 {
    static NEXT: AtomicI32 = AtomicI32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

extern "C" fn free_handle(_: i32) {}

/// Stand in for managed's handle index (callbacks 16 and 17), as physics/aggregate_tests.rs does.
fn fake_managed() {
    let mut table = vec![0usize; abi::CALLBACK_COUNT];
    table[16] = register_handle as *const () as usize;
    table[17] = free_handle as *const () as usize;
    // SAFETY: the table holds CALLBACK_COUNT entries; only the two set are called here.
    unsafe { abi::rt::callbacks::set_table(table.as_ptr()) };
}

fn chain() -> swap_chain::Handle {
    let mut info = RenderDeviceInfo_t::default();
    (info.m_display_mode.m_n_width, info.m_display_mode.m_n_height) = (SIZE as i32, SIZE as i32);
    swap_chain::create(RawPtr::NULL, info, Some(c"scene test"))
}

/// The stand-in cube (16 units) at the origin and a sky, seen from 64 units back along -X.
fn scene() -> (world::Ref, camera::Ref) {
    fake_managed();
    let w: world::Ref = objects::create_shared(Arc::new(SceneWorld::default()));
    let model = crate::model::create_runtime("models/test/cube.vmdl", Content::default());
    let state = State { model, transform: crate::math::identity(), ..State::default() };
    object::create::<class::CSceneObject, class::CSceneObject>(w, Kind::SceneObject, state);
    object::create::<class::CSceneSkyBoxObject, class::CSceneObject>(w, Kind::SceneSkyBoxObject, State::default());
    let cam = camera::create(None, 0);
    let a = cam.addr();
    field!(set Vector3, c::GET_CAMERA_POSITION, a, crate::math::vec3(-64.0, 0.0, 0.0));
    field!(set Angles, c::GET_CAMERA_ROTATION, a, Angles::default());
    field!(set f32, c::GET_FIELD_OF_VIEW, a, 90.0);
    field!(set f32, c::GET_Z_NEAR, a, 1.0);
    field!(set f32, c::GET_Z_FAR, a, 1000.0);
    objects::get::<_, std::sync::Mutex<camera::CameraRenderer>>(cam).unwrap().lock().unwrap().worlds.push(w);
    (w, cam)
}

fn read(t: texture::Ref) -> render::Frame {
    texture::resolve(t).unwrap().gpu.as_ref().unwrap().read_back(device::gpu().unwrap())
}

#[test]
fn camera_draws_model_over_sky() {
    device::create().expect("a wgpu adapter (lavapipe on headless Linux)");
    let (_world, cam) = scene();
    let sc = chain();
    let draw = || {
        camera::render(cam, sc, swap_chain::targets(sc).unwrap().0, swap_chain::targets(sc).unwrap().1, SIZE, SIZE);
        assert!(swap_chain::present(sc));
        read(swap_chain::targets(sc).unwrap().0)
    };
    let (a, b) = (draw(), draw());
    assert_eq!(a.hash(), b.hash(), "deterministic");
    let sky = [115, 153, 217, 255];
    for (x, y) in [(0, 0), (24, 32), (40, 32), (32, 24), (32, 40)] {
        assert_eq!(a.texel(x, y), sky, "sky at ({x}, {y})");
    }
    // The cube's -X face: 16 units at 56 units under a 90° FOV spans pixels 27.4..36.6.
    for (x, y) in [(29, 29), (32, 32), (35, 35)] {
        assert_ne!(a.texel(x, y), sky, "cube at ({x}, {y})");
    }
    let depth = read(swap_chain::targets(sc).unwrap().1);
    let d = |x, y| f32::from_le_bytes(depth.texel(x, y).try_into().unwrap());
    assert!(d(32, 32) < 1.0 && d(0, 0) == 1.0, "cube writes depth, the sky doesn't");
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/frames.tsv");
    a.check_golden(&golden, "camera_draws_model_over_sky", &device::gpu().unwrap().describe());
}

/// A red cube in a 3D skybox at scale 16: skybox (30, 0, 6) is main-world (480, 0, 96), 256
/// units wide, so it shows above the main cube (rows 16..36) and behind it.
#[test]
fn skybox_world_draws_behind_the_main_world() {
    device::create().expect("a wgpu adapter");
    let (w, cam) = scene();
    let sky: world::Ref = objects::create_shared(Arc::new(SceneWorld::default()));
    let model = crate::model::create_runtime("models/test/cube.vmdl", Content::default());
    let at = Transform { position: crate::math::vec3(30.0, 0.0, 6.0), ..crate::math::identity() };
    let state = State { model, transform: at, tint: crate::math::vec4(1.0, 0.0, 0.0, 1.0), ..State::default() };
    object::create::<class::CSceneObject, class::CSceneObject>(sky, Kind::SceneObject, state);
    *world::get(w).unwrap().skybox.lock().unwrap() = world::Skybox { world: sky.addr(), scale: 16.0, ..Default::default() };
    let color = texture::create_target("skybox", SIZE, SIZE, 0);
    assert!(camera::render_to_texture(cam, color, Default::default(), Default::default()));
    let f = read(color);
    let (above, front) = (f.texel(32, 18), f.texel(32, 30));
    assert!(above[0] > 100 && above[1] < 40 && above[2] < 40, "skybox cube above the main cube: {above:?}");
    assert!(front[1] > 40 && front[0].abs_diff(front[1]) < 60, "the main cube stays in front: {front:?}");
    assert_eq!(f.texel(2, 2), [115, 153, 217, 255], "sky elsewhere");
}

#[test]
fn render_to_texture_and_bitmap() {
    device::create().expect("a wgpu adapter");
    let (_world, cam) = scene();
    let color = texture::create_target("rtt", SIZE, SIZE, 0);
    assert!(camera::render_to_texture(cam, color, Default::default(), Default::default()));
    let frame = read(color);
    assert_ne!(frame.texel(32, 32), frame.texel(0, 0));
    let mut bitmap = vec![0u8; (SIZE * SIZE * 4) as usize];
    camera::render_to_bitmap(cam, RawPtr::from_raw(bitmap.as_mut_ptr().cast()), SIZE as i32, SIZE as i32, 4);
    assert_eq!(bitmap, frame.pixels, "bitmap matches the texture render");
}

#[test]
fn transform_moves_the_object() {
    let t = Transform { position: crate::math::vec3(1.0, 2.0, 3.0), ..crate::math::identity() };
    let m = crate::render::draw::model::matrix(&t);
    assert_eq!(m.transform_point3(glam::Vec3::ZERO), glam::Vec3::new(1.0, 2.0, 3.0));
}
