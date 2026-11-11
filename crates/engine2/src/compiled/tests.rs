//! Compiled resources from a game folder: the fixture crate model with its draw calls, groups,
//! skins, skeleton and physics; its material and texture through the compiled loaders; and map
//! world physics that the crate lands on.

use crate::math::{identity, vec3};
use crate::physics::world::{self, PhysicsWorld};
use crate::physics::{aggregate, body};
use abi::structs::Transform;
use assets::compiled::fixture;
use std::path::PathBuf;
use std::sync::{Arc, Once};

const WORLD_PHYSICS: &str = "maps/compiledtest/world_physics.vphys";

/// One game folder with the fixture files for every test here, on the search paths.
pub(super) fn game_dir() -> PathBuf {
    static MOUNT: Once = Once::new();
    let dir = std::env::temp_dir().join(format!("underbox_compiled_{}", std::process::id()));
    MOUNT.call_once(|| {
        let mut files = fixture::crate_files();
        files.extend(fixture::rigged_files());
        files.push((format!("{WORLD_PHYSICS}_c"), assets::compiled::physics::write(&fixture::world_physics(256.0, &[]))));
        for (path, bytes) in files {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        }
        crate::fs::add_search_path("GAME", dir.to_str().unwrap(), true);
    });
    dir
}

#[test]
fn compiled_model_loads_meshes_draws_groups_and_skins() {
    game_dir();
    let r = crate::model::load(Some(&std::ffi::CString::new(format!("{}_c", fixture::MODEL)).unwrap()), abi::Guid::default());
    let m = crate::model::get(r).unwrap();
    let c = &m.content;
    assert_eq!(c.meshes.len(), 2);
    assert_eq!((c.meshes[0].lod_mask, c.meshes[1].lod_mask, c.meshes[0].group_mask), (1, 2, 1));
    let draws = c.meshes[0].mesh.draw_calls();
    assert_eq!(draws.iter().map(|d| (d.start_index, d.index_count)).collect::<Vec<_>>(), [(0, 30), (30, 6)]);
    assert!(draws.iter().all(|d| d.material.is_some()), "draws keep their material handle");
    assert_eq!(c.meshes[0].mesh.vertices().len(), 24);
    let b = c.render_bounds();
    assert_eq!((b.mins.vec.z, b.maxs.vec.z), (0.0, 2.0 * fixture::HALF));
    assert_eq!((c.groups.as_slice(), c.default_group_mask, c.lod_distances.as_slice()), (["body".to_owned()].as_slice(), 1, [0.0, 400.0].as_slice()));
    assert_eq!(c.material_groups.iter().map(|g| (g.name.as_str(), g.materials.len())).collect::<Vec<_>>(), [("default", 1), ("dark", 1)]);
    let physics = c.physics.as_ref().expect("PHYS block");
    assert_eq!(physics.parts[0].shapes.hulls.len(), 1);
    assert_eq!(crate::anim::data::by_name(fixture::MODEL).anim.skeleton.len(), 1);
}

#[test]
fn compiled_texture_decodes_to_rgba() {
    game_dir();
    let t = super::texture::load(fixture::TEXTURE).unwrap();
    assert_eq!((t.config.m_n_width, t.config.m_n_height, t.config.m_n_image_format), (16, 16, 0));
    assert_eq!(t.data.len(), 16 * 16 * 4);
    let near = |a: &[u8], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 8);
    assert!(near(&t.data[..4], fixture::CHECKER[0]) && near(&t.data[16..20], fixture::CHECKER[1]));
}

#[test]
fn compiled_material_has_shader_params_and_textures() {
    game_dir();
    let m = super::material::load(fixture::MATERIAL).unwrap();
    assert_eq!(m.shader, "shaders/complex.shader");
    let names: Vec<_> = m.vectors.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["g_flModelTintAmount", "g_vColorTint"]);
    assert_eq!(m.textures.len(), 1);
    let t = crate::render::texture::resolve(m.textures[0].1).unwrap();
    assert!(!crate::resource::Resource::is_error(&*t), "made from the compiled texture");
}

#[test]
fn compiled_ragdoll_spawns_jointed_bodies() {
    crate::handles::fake_managed();
    game_dir();
    let r = crate::model::load(Some(&std::ffi::CString::new(fixture::RIGGED).unwrap()), abi::Guid::default());
    let data = crate::model::get(r).and_then(|m| m.content.physics.clone()).expect("ragdoll");
    let joints: Vec<_> = (0..data.joints.len()).filter_map(|i| data.joint(i)).collect();
    assert_eq!(joints.iter().map(|j| (j.kind, j.body1, j.body2)).collect::<Vec<_>>(), [(4, 0, 1), (3, 1, 2)]);
    assert!(joints[0].enable_swing_limit && joints[0].enable_twist_limit && joints[0].swing_limit == [0.0, 0.5]);
    assert_eq!((joints[1].twist_limit, joints[1].angular_frequency), ([-0.75, 0.75], 4.0));
    let w = new_world();
    let doll = aggregate::spawn(w, data, identity(), 1).unwrap();
    let a = aggregate::get(doll).unwrap();
    assert_eq!((a.bodies.len(), a.joints.lock().unwrap().len()), (3, 2));
    assert_eq!(a.bodies.iter().map(|(bone, _)| bone.as_str()).collect::<Vec<_>>(), ["root", "spine", "head"]);
    aggregate::destroy(doll);
}

/// The wave's managed event: frames 1..=3 at 10 fps, its payload owned by the clip resource.
#[test]
fn compiled_clip_events_read_through_the_clip_resource() {
    game_dir();
    let clip = crate::anim::resources::clip(&format!("{}/wave", fixture::RIGGED)).expect("wave");
    assert_eq!(clip.clip.events.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["AE_FOOTSTEP", "AE_MANAGED_EVENT"]);
    let h: abi::NativeRef<abi::class::CAnimClipResource> = crate::resource::new_handle(Arc::new(clip));
    let (mut start, mut duration) = (0.0, 0.0);
    let payload = crate::anim::resources::event_data(h, 1, &mut start, &mut duration);
    assert!((start - 0.1).abs() < 1e-6 && (duration - 0.2).abs() < 1e-6, "{start} {duration}");
    let value = crate::kv3::node::to_value(&crate::kv3::node::get(payload).expect("payload"));
    let member = |k: &str| value.as_object().and_then(|o| o.get(k)).and_then(|v| v.as_str()).map(str::to_owned);
    assert_eq!((member("Type").as_deref(), member("Data").as_deref()), (Some("Footfall"), Some("{\"Foot\":1}")));
    assert_eq!(crate::anim::resources::event_data(h, 1, &mut start, &mut duration), payload, "one payload per event");
    assert!(crate::anim::resources::event_data(h, 2, &mut start, &mut duration).is_null());
    crate::resource::destroy_handle(h);
    assert!(crate::kv3::node::get(payload).is_none(), "freed with the resource");
}

fn new_world() -> world::Ref {
    objects::create_shared(Arc::new(PhysicsWorld::default()))
}

#[test]
fn compiled_crate_lands_on_compiled_world_physics() {
    crate::handles::fake_managed();
    game_dir();
    let w = new_world();
    let ground = crate::model::physics::compiled::load(WORLD_PHYSICS).expect("world physics");
    assert_eq!((ground.parts[0].shapes.meshes.len(), ground.attributes[0].as_slice()), (1, ["solid".to_owned(), "world".to_owned()].as_slice()));
    let ground = aggregate::spawn(w, ground, identity(), 2).unwrap();
    let r = crate::model::load(Some(&std::ffi::CString::new(fixture::MODEL).unwrap()), abi::Guid::default());
    let data = crate::model::get(r).and_then(|m| m.content.physics.clone()).unwrap();
    let start = Transform { position: vec3(0.0, 0.0, 50.0), ..identity() };
    let crate_ = aggregate::spawn(w, data, start, 1).unwrap();
    let b = aggregate::get(crate_).unwrap().bodies[0].1;
    for _ in 0..180 {
        world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
    }
    let z = body::with_rb(b, |rb| rb.translation().z);
    assert!(z.abs() < 1.0, "rests on the ground mesh, z = {z}");
    aggregate::destroy(crate_);
    aggregate::destroy(ground);
}
