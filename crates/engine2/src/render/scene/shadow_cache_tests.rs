//! Shadow views drawn frame after frame: a static scene keeps its map from the second frame on,
//! every input that moves its pixels draws it again, and a cached map equals an uncached one.

use super::super::shadow_view::{ShadowView, draw};
use super::super::{tests::fake_managed, view};
use crate::frustum::camera::Camera;
use crate::handles::Kind;
use crate::model::content::Content;
use crate::render::{device, texture};
use crate::scene::object::{self, State};
use crate::scene::world::{self, SceneWorld};
use abi::{NativeRef, class};
use glam::{Quat, Vec3, Vec4};
use std::sync::{Arc, Mutex};

const MAP: u32 = 128;

/// A sun looking straight down on a world, one shadow map per [`Sun::view`].
struct Sun {
    world: world::Ref,
    parent: view::Ref,
    frustum: NativeRef<class::CFrustum>,
}

impl Sun {
    fn new() -> Sun {
        fake_managed();
        let world: world::Ref = objects::create_shared(Arc::new(SceneWorld::default()));
        let desc = view::Desc {
            camera: Camera::default(),
            proj: None,
            viewport: Default::default(),
            swap_chain: Default::default(),
            color: texture::Ref::NULL,
            depth: texture::Ref::NULL,
            worlds: vec![world],
            render_tags: Vec::new(),
            exclude_tags: Vec::new(),
            post_process: false,
            ambient: Vec4::ZERO,
            environment: Default::default(),
            unique_id: 0,
            managed_camera_id: 0,
        };
        let mut c = Camera { origin: Vec3::new(0.0, 0.0, 500.0), near: 1.0, far: 2000.0, ortho: Some((256.0, 256.0)), ..Camera::default() };
        c.set_angles(Vec3::new(90.0, 0.0, 0.0));
        Sun { world, parent: view::create(desc), frustum: objects::create(Mutex::new(c)) }
    }

    fn cube(&self, at: Vec3) -> object::Ref {
        let model = crate::model::create_runtime("models/test/cube.vmdl", Content::default());
        object::create_native::<class::CSceneObject, class::CSceneObject>(self.world, Kind::SceneObject, State { model, transform: placed(at), ..State::default() }, None).0
    }

    fn view(&self, name: &str) -> ShadowView {
        let target = texture::create_target(name, MAP, MAP, 62);
        ShadowView { parent: self.parent, frustum: self.frustum, viewport: Default::default(), target, slice: 0, required: 0, excluded: 0, bias: (-1, -1.5) }
    }

    fn move_light(&self, by: Vec3) {
        objects::get::<_, Mutex<Camera>>(self.frustum).unwrap().lock().unwrap().origin += by;
    }
}

fn placed(at: Vec3) -> abi::structs::Transform {
    abi::structs::Transform { position: crate::math::vec3(at.x, at.y, at.z), ..crate::math::identity() }
}

fn move_to(r: object::Ref, at: Vec3) {
    object::with(r, |s| s.transform = placed(at));
}

fn map(v: &ShadowView) -> Vec<u8> {
    texture::resolve(v.target).unwrap().gpu.as_ref().unwrap().read_back(device::gpu().unwrap()).pixels
}

#[test]
fn a_static_scene_keeps_its_map_until_an_input_moves() {
    device::create().expect("a wgpu adapter");
    let sun = Sun::new();
    let cube = sun.cube(Vec3::ZERO);
    let v = sun.view("cached cascade");
    assert!(draw(&v, true), "frame 1 draws");
    assert!(!draw(&v, true) && !draw(&v, true), "frames 2 and 3 keep it");
    let kept = map(&v);
    assert!(!draw(&v, true), "a read-back is no write");

    move_to(cube, Vec3::new(20.0, 0.0, 0.0));
    assert!(draw(&v, true), "a moved caster draws");
    assert!(!draw(&v, true));
    assert_ne!(map(&v), kept, "and its shadow moved");

    sun.move_light(Vec3::new(8.0, 0.0, 0.0));
    assert!(draw(&v, true), "a moved light draws");
    assert!(!draw(&v, true));

    let mat = crate::render::material::create_raw(Some(c"materials/test/shadow_cache.vmat"), Some(c"shaders/complex.shader"), true);
    object::with(cube, |s| s.material_override = crate::mesh::material_ref::MaterialRef::keep(mat));
    assert!(draw(&v, true), "a material change draws");
    assert!(!draw(&v, true));

    let other = sun.cube(Vec3::new(-40.0, 0.0, 0.0));
    assert!(draw(&v, true), "an added caster draws");
    object::with(other, |s| s.rendering = false);
    assert!(draw(&v, true), "a hidden caster draws");
    assert!(!draw(&v, true));

    texture::resolve(v.target).unwrap().gpu.as_ref().unwrap().wrote();
    assert!(draw(&v, true), "another write to the map draws");
    assert!(draw(&v, false) && draw(&v, false), "without the cache every frame draws");
}

/// Two maps of one scene, one cached and one not, frame by frame while a caster and then the
/// light move and stop: byte-identical every frame.
#[test]
fn a_cached_map_equals_a_drawn_one_through_motion() {
    device::create().expect("a wgpu adapter");
    let sun = Sun::new();
    sun.cube(Vec3::new(-50.0, 30.0, 0.0));
    let mover = sun.cube(Vec3::ZERO);
    let (cached, drawn) = (sun.view("cached"), sun.view("drawn"));
    let mut kept = 0;
    for frame in 0..12 {
        if (3..6).contains(&frame) {
            move_to(mover, Vec3::new(10.0 * frame as f32, -5.0 * frame as f32, 0.0));
        }
        if frame == 8 {
            sun.move_light(Vec3::new(0.0, 12.0, 0.0));
        }
        kept += u32::from(!draw(&cached, true));
        assert!(draw(&drawn, false));
        assert!(map(&cached) == map(&drawn), "frame {frame}: cached map differs");
    }
    assert_eq!(kept, 7, "frames 1, 2, 6, 7, 9, 10 and 11 keep the map");
}

/// A posed animatable caster keeps its map while the pose holds and draws once a bone moves.
#[test]
fn a_skinned_caster_draws_when_its_pose_changes() {
    let Some(()) = crate::render::skin::tests::mount() else { return eprintln!("skipped: no sbox-public checkout") };
    device::create().expect("a wgpu adapter");
    let sun = Sun::new();
    let name = std::ffi::CString::new(format!("{}_c", assets::compiled::fixture::RIGGED)).unwrap();
    let model = crate::model::load(Some(&name), abi::Guid::default());
    let r = object::create_native::<class::CSceneAnimatableObject, class::CSceneObject>(sun.world, Kind::SceneAnimatableObject, State { model, ..State::default() }, None).0;
    let pose = |turn: f32| crate::anim::object::with(r.cast(), |s| s.world = vec![anim::Xform { rot: Quat::from_rotation_z(turn), ..Default::default() }; 3]);
    pose(0.0);
    let v = sun.view("skinned cascade");
    assert!(draw(&v, true));
    assert!(!draw(&v, true), "a held pose keeps the map");
    pose(0.5);
    assert!(draw(&v, true), "a moved bone draws");
    assert!(!draw(&v, true));
}
