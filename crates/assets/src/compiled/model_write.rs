//! Writing `.vmdl_c` fixtures: every mesh embedded (MDAT + MBUF, MRPH for a morph set, the older
//! CTRL layout), attachments and hitboxes in the first mesh's data, PHYS, and the animations
//! embedded as AGRP (decode key) + ANIM blocks.

use super::anim::{Rig, encode, layout};
use super::kv::{floats, int_value, list, obj, resource, text};
use super::model::Model;
use super::vbib::{self, Packing};
use super::{mesh, morph, physics, rig};
use ::kv::{Object, Value};

/// Frames per frame block in written animations.
const BLOCK: usize = 4;

/// `PermModelInfo_t`: only the key values are ours; real's loader wants the rest present.
fn model_info(m: &Model) -> Value {
    let zero3 = || floats(&[0.0; 3]);
    obj(vec![
        ("m_nFlags", int_value(0)),
        ("m_vHullMin", zero3()),
        ("m_vHullMax", zero3()),
        ("m_vViewMin", zero3()),
        ("m_vViewMax", zero3()),
        ("m_flMass", Value::Double(0.0)),
        ("m_vEyePosition", zero3()),
        ("m_flMaxEyeDeflection", Value::Double(0.0)),
        ("m_sSurfaceProperty", text("")),
        ("m_keyValueText", text(&m.key_values)),
    ])
}

/// `PermModelData_t` with every member real's compiler writes, in its order: its schema
/// conversion rejects a DATA block that leaves members out (docs/differential.md).
fn write_data(m: &Model) -> Value {
    let s = &m.bones;
    let skeleton = obj(vec![
        ("m_boneName", list(s.iter().map(|b| text(&b.name)))),
        ("m_nParent", list(s.iter().map(|b| int_value(b.parent.map_or(-1, |p| p as i64))))),
        ("m_boneSphere", list(s.iter().map(|_| Value::Double(0.0)))),
        ("m_nFlag", list(s.iter().map(|_| int_value(0)))),
        ("m_bonePosParent", list(s.iter().map(|b| floats(&b.position)))),
        ("m_boneRotParent", list(s.iter().map(|b| floats(&b.rotation)))),
    ]);
    let groups = m.material_groups.iter().map(|g| obj(vec![("m_name", text(&g.name)), ("m_materials", list(g.materials.iter().map(|x| resource(x))))]));
    // Vertex blend indices are model bone indices, so each mesh's remapping is the identity.
    let remap_starts = (0..m.meshes.len()).map(|i| int_value((i * s.len()) as i64));
    let remap = m.meshes.iter().flat_map(|_| (0..s.len()).map(|b| int_value(b as i64)));
    let empty = || list([]);
    obj(vec![
        ("m_name", text(&m.name)),
        ("m_modelInfo", model_info(m)),
        ("m_ExtParts", empty()),
        ("m_refMeshes", list(m.meshes.iter().map(|_| Value::Null))),
        ("m_refMeshGroupMasks", list(m.meshes.iter().map(|x| int_value(x.group_mask as i64)))),
        ("m_refPhysGroupMasks", empty()),
        ("m_refLODGroupMasks", list(m.meshes.iter().map(|x| int_value(x.lod_mask as i64)))),
        ("m_lodGroupSwitchDistances", floats(&m.lod_distances)),
        ("m_refPhysicsData", list(m.physics_refs.iter().map(|p| resource(p)))),
        ("m_refPhysicsHitboxData", empty()),
        ("m_refAnimGroups", list(m.anim_groups.iter().map(|p| resource(p)))),
        ("m_refSequenceGroups", empty()),
        ("m_meshGroups", list(m.mesh_groups.iter().map(|g| text(g)))),
        ("m_materialGroups", list(groups)),
        ("m_nDefaultMeshGroupMask", int_value(m.default_group_mask as i64)),
        ("m_modelSkeleton", skeleton),
        ("m_remappingTable", list(remap)),
        ("m_remappingTableStarts", list(remap_starts)),
        ("m_boneFlexDrivers", empty()),
        ("m_BodyGroupsHiddenInTools", empty()),
    ])
}

/// The mesh data's (empty) attachments and hitbox sets replaced by the model's.
fn with_rig(data: Value, m: &Model) -> Value {
    let Value::Object(Object(mut members)) = data else { return data };
    for (k, v) in rig::write(&m.attachments, &m.hitbox_sets) {
        match members.iter_mut().find(|(name, _)| name == k) {
            Some(slot) => slot.1 = v,
            None => members.push((k.to_owned(), v)),
        }
    }
    Value::Object(Object(members))
}

/// A `.vmdl_c` from `m`. `missing`, `anim_groups`' animations and mesh morph targets aren't
/// written (targets live in the atlas the morph set names).
pub fn write(m: &Model, packing: Packing) -> Vec<u8> {
    // Block 0 is RERL (see `super::write`), DATA is 1, CTRL 2; the rest follow.
    let mut blocks = vec![(*b"DATA", super::kv3(&write_data(m))), (*b"CTRL", Vec::new())];
    let next = |blocks: &Vec<_>| blocks.len() as i64 + 1;
    let mut entries = Vec::new();
    for (i, mm) in m.meshes.iter().enumerate() {
        let first = next(&blocks);
        let data = mesh::write_data(&mm.mesh);
        blocks.push((*b"MDAT", super::kv3(&if i == 0 { with_rig(data, m) } else { data })));
        blocks.push((*b"MBUF", vbib::write(&mesh::write_buffers(&mm.mesh), packing)));
        let morph_block = mm.morph.as_ref().map_or(-1, |s| {
            blocks.push((*b"MRPH", super::kv3(&morph::write(s))));
            first + 2
        });
        entries.push(obj(vec![("name", text(&mm.name)), ("mesh_index", int_value(i as i64)), ("data_block", int_value(first)), ("vbib_block", int_value(first + 1)), ("morph_block", int_value(morph_block))]));
    }
    let mut ctrl = vec![("embedded_meshes", list(entries))];
    if let Some(p) = &m.physics {
        ctrl.push(("embedded_physics", obj(vec![("phys_data_block", int_value(next(&blocks))), ("parts", int_value(p.parts.len() as i64))])));
        blocks.push((*b"PHYS", super::kv3(&physics::write_kv3(p))));
    }
    if !m.animations.is_empty() {
        let flex: Vec<String> = m.flex_controllers().iter().map(|c| c.name.clone()).collect();
        let (anim, key) = encode::write(&m.animations, &Rig { bones: &m.bones, flex: &flex }, BLOCK);
        let seqs: Vec<(&str, &str, bool, bool)> = m.animations.iter().map(|a| (a.name.as_str(), a.name.as_str(), a.looping, a.hidden)).collect();
        let first = next(&blocks);
        let members = [("group_data_block", first + 2), ("anim_data_block", first), ("seqgroup_data_block", first + 1)];
        ctrl.push(("embedded_animation", obj(members.into_iter().map(|(k, b)| (k, int_value(b))).collect())));
        blocks.push((*b"ANIM", super::kv3(&anim)));
        blocks.push((*b"ASEQ", super::kv3(&layout::sequences(&seqs))));
        blocks.push((*b"AGRP", super::kv3(&layout::group(key, &[]))));
    }
    blocks[1].1 = super::kv3(&obj(ctrl));
    let mut refs: Vec<&str> = m.meshes.iter().flat_map(|x| x.mesh.draws.iter().map(|d| d.material.as_str())).collect();
    refs.extend(m.material_groups.iter().flat_map(|g| g.materials.iter().map(String::as_str)));
    refs.extend(m.physics_refs.iter().chain(&m.anim_groups).map(String::as_str));
    refs.extend(m.meshes.iter().filter_map(|x| x.morph.as_ref()).map(|s| s.atlas.as_str()));
    refs.sort_unstable();
    refs.dedup();
    super::write(super::RESOURCE_VERSION, &refs, blocks)
}
