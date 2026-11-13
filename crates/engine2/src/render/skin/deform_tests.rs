//! skinning_cs through deformation volumes on the puppet fixture (bind pose, so model space is
//! world space): a box sucking the top half to its centre, a transform everywhere, and the same
//! box moving the mesh rigidly by its bones' anchors.

use super::deform::{Deformation, VOLUME_BYTES};
use super::tests::{cached_positions, mount};
use super::*;
use crate::render::device;
use assets::compiled::fixture;
use glam::Vec3;

const SPHERE: f32 = 0.0;
const BOX: f32 = 1.0;
const INFINITE: f32 = 1000.0;
const TRANSFORM: f32 = 2.0;
const SUCK: f32 = 3.0;
const IDENTITY: [[f32; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// One `SceneDeformationVolumeData` placed at the model origin, as `ModelDeformer.Pack` writes it:
/// full weight, no falloff, accurate normals.
fn volume(shape: f32, operation: f32, center: Vec3, size: Vec3, radius: f32, transform: [[f32; 4]; 3]) -> Vec<u8> {
    let rows = [IDENTITY, IDENTITY].concat();
    let rest = [[size.x, size.y, size.z, radius], [1.0, 0.0, 0.0, 0.0], [1.0, operation, 0.0, 1.0], [center.x, center.y, center.z, shape]];
    let out: Vec<u8> = rows.iter().chain(&rest).chain(&transform).flatten().flat_map(|f| f.to_le_bytes()).collect();
    assert_eq!(out.len(), VOLUME_BYTES);
    out
}

/// The puppet in bind pose, deformed by `volumes` (rigidly from `anchors` when given): its 24
/// skinned positions, and the bind positions.
fn deformed(volumes: &[u8], anchors: &[[f32; 4]]) -> Option<(Vec<Vec3>, Vec<Vec3>)> {
    mount()?;
    let gpu = device::create()?;
    let r = crate::model::load(Some(&std::ffi::CString::new(format!("{}_c", fixture::PUPPET)).unwrap()), abi::Guid::default());
    let model = crate::model::get(r).unwrap();
    let skin = mesh_skin(&model, r, 0).unwrap();
    let offset = cache::allocate(skin.vertices).unwrap();
    let transforms = Arc::new(render::Buffer::with_data(gpu, "deformed transforms", render::BufferKind::Storage, &native::entries(&[Mat4::IDENTITY; 3], [1.0; 4], offset, skin.weights, None)));
    let deformers = crate::anim::object::Deformers { volumes: volumes.to_vec(), anchors: anchors.to_vec() };
    let (deformation, count, anchor): (Deformation, u32, u32) = deform::native(&deformers, 3).unwrap();
    let job = Job { slot: 0, offset, volume_offset: 0, volume_count: count, anchor_offset: anchor };
    run(dispatch::SHADER, &skin, &transforms, &[job], false, Some(&deformation)).expect("skinning_cs builds with D_DEFORMATION_VOLUME");
    let bind = model.content.meshes[0].mesh.vertices().iter().map(|v| Vec3::from(v.position)).collect();
    Some((cached_positions(gpu, offset, 24), bind))
}

/// A box around the puppet's top (z 12..20; the puppet is 16 tall).
fn top_box(operation: f32, transform: [[f32; 4]; 3]) -> Vec<u8> {
    volume(BOX, operation, Vec3::new(0.0, 0.0, 16.0), Vec3::new(10.0, 10.0, 4.0), 0.0, transform)
}

#[test]
fn a_box_sucks_what_it_holds_to_its_centre() {
    let Some((got, bind)) = deformed(&top_box(SUCK, IDENTITY), &[]) else { return eprintln!("skipped: no sbox-public checkout or GPU") };
    for (i, (g, b)) in got.iter().zip(&bind).enumerate() {
        let want = if b.z > 12.0 { Vec3::new(0.0, 0.0, 16.0) } else { *b };
        assert!((*g - want).length() < 1e-3, "vertex {i}: got {g}, want {want}");
    }
}

#[test]
fn an_infinite_transform_moves_everything_and_stacks_in_order() {
    let lift = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 10.0]];
    // The lift first carries the bottom corners (11.3 from the origin) out of a sphere that would suck them in.
    let sphere = volume(SPHERE, SUCK, Vec3::ZERO, Vec3::ZERO, 12.0, IDENTITY);
    let stack = [volume(INFINITE, TRANSFORM, Vec3::ZERO, Vec3::ZERO, 0.0, lift), sphere].concat();
    let Some((got, bind)) = deformed(&stack, &[]) else { return eprintln!("skipped: no sbox-public checkout or GPU") };
    for (i, (g, b)) in got.iter().zip(&bind).enumerate() {
        let want = *b + Vec3::new(0.0, 0.0, 10.0);
        assert!((*g - want).length() < 1e-3, "vertex {i}: got {g}, want {want}");
    }
}

#[test]
fn anchors_move_the_mesh_rigidly_by_its_weights() {
    let shift = [[1.0, 0.0, 0.0, 5.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
    // Root's anchor sits outside the box, the spine's and head's inside.
    let anchors = [[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 16.0, 0.0], [0.0, 0.0, 16.0, 0.0]];
    let Some((got, bind)) = deformed(&top_box(TRANSFORM, shift), &anchors) else { return eprintln!("skipped: no sbox-public checkout or GPU") };
    for (i, (g, b)) in got.iter().zip(&bind).enumerate() {
        // Weights as the fixture assigns them (puppet.rs), in unorm bytes.
        let moved = match i {
            20.. => 1.0,
            _ if b.z > 0.0 => 128.0 / 255.0,
            _ => 0.0,
        };
        let want = *b + Vec3::new(5.0 * moved, 0.0, 0.0);
        assert!((*g - want).length() < 1e-2, "vertex {i}: got {g}, want {want}");
    }
}
