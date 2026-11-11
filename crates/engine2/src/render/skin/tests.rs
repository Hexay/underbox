//! Skinning without a client boot: the fixture's rigged model (assets fixture) reports its skin,
//! and skinning_cs on the GPU writes posed, morphed vertices into the vertex cache. Skips without
//! an sbox-public checkout (core shaders) or a GPU.

use super::*;
use crate::render::device;
use assets::compiled::fixture;
use glam::Vec3;
use std::sync::Once;

/// The fixture's compiled files and sbox-public's core shaders, on the search paths.
fn mount() -> Option<()> {
    static MOUNT: Once = Once::new();
    let core = crate::render::shading::tests::core_dir()?;
    MOUNT.call_once(|| {
        crate::fs::add_search_path("GAME", &core.to_string_lossy(), false);
        let dir = std::env::temp_dir().join(format!("underbox_skin_{}", std::process::id()));
        let mut files = fixture::crate_files();
        files.extend(fixture::rigged_files());
        for (path, bytes) in files {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        }
        crate::fs::add_search_path("GAME", dir.to_str().unwrap(), true);
    });
    Some(())
}

fn rigged() -> crate::model::Ref {
    crate::model::load(Some(&std::ffi::CString::new(format!("{}_c", fixture::RIGGED)).unwrap()), abi::Guid::default())
}

#[test]
fn the_rigged_mesh_reports_its_skin() {
    let Some(()) = mount() else { return eprintln!("skipped: no sbox-public checkout") };
    let r = rigged();
    let model = crate::model::get(r).expect("rigged model loads");
    let skin = mesh_skin(&model, r, 0).expect("a skinned mesh");
    assert_eq!((skin.vertices, skin.bones, skin.weights), (24, 3, 1));
    assert_eq!((skin.input.bone_index, skin.input.bone_weight, skin.input.normal), (76, 80, 16));
    let (mut bones, mut weights) = (0i32, 0i32);
    let n = skinning(r, 0, RawPtr::from_raw((&raw mut bones).cast()), RawPtr::from_raw((&raw mut weights).cast()));
    assert_eq!((n, bones, weights), (24, 3, 1));
    let spine = inverse_bind_poses(r)[1];
    assert!(spine.transform_point3(Vec3::new(0.0, 0.0, fixture::HALF)).length() < 1e-5, "the spine's inverse bind pose undoes its bind");
    let crate_model = crate::model::load(Some(&std::ffi::CString::new(format!("{}_c", fixture::MODEL)).unwrap()), abi::Guid::default());
    assert!(mesh_skin(&crate::model::get(crate_model).unwrap(), crate_model, 0).is_none(), "a one-bone model draws rigidly");
}

/// What skinning_cs wrote for `n` vertices from `offset`: positions.
fn cached_positions(gpu: &render::Gpu, offset: u32, n: usize) -> Vec<Vec3> {
    let bytes = cache::buffer().unwrap().read(gpu, u64::from(offset) * cache::VERTEX_BYTES, n as u64 * cache::VERTEX_BYTES);
    let f = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    (0..n).map(|v| Vec3::new(f(v * 24), f(v * 24 + 4), f(v * 24 + 8))).collect()
}

#[test]
fn skinning_cs_poses_and_morphs_into_the_vertex_cache() {
    let Some(()) = mount() else { return eprintln!("skipped: no sbox-public checkout") };
    let Some(gpu) = device::create() else { return eprintln!("skipped: no GPU") };
    let r = rigged();
    let model = crate::model::get(r).unwrap();
    let skin = mesh_skin(&model, r, 0).unwrap();
    let mesh = model.content.meshes[0].mesh.clone();
    // Skin matrices: the spine's turns a quarter about z and moves 100 along x; the head's lifts 50.
    let spine = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0)) * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let bones = [Mat4::IDENTITY, spine, Mat4::from_translation(Vec3::new(0.0, 0.0, 50.0))];
    let weights: crate::render::draw::morph::Weights = [("smile".to_owned(), 1.0)].into();
    let (data, subrects) = morphs::compose(&[(&weights, &mesh)]);
    let offset = cache::allocate(skin.vertices).unwrap();
    let entries = native::entries(&bones, [1.0; 4], offset, skin.weights, subrects.first().copied());
    let transforms = Arc::new(render::Buffer::with_data(gpu, "test transforms", render::BufferKind::Storage, &entries));
    run(dispatch::SHADER, &skin, &transforms, &[Job { slot: 0, offset }], morphs::upload(&data).as_ref()).expect("skinning_cs builds and dispatches");
    let got = cached_positions(gpu, offset, 24);
    for (i, v) in mesh.vertices().iter().enumerate() {
        let lift = if (20..24).contains(&i) { fixture::SMILE_LIFT } else { 0.0 };
        let bone = if i >= 20 { 2 } else { 1 };
        let want = bones[bone].transform_point3(Vec3::from(v.position) + Vec3::new(0.0, 0.0, lift));
        assert!((got[i] - want).length() < 1e-3, "vertex {i}: got {}, want {want}", got[i]);
    }
}
