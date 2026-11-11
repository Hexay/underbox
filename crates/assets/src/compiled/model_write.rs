//! Writing `.vmdl_c` fixtures: every mesh embedded (MDAT + MBUF, MRPH for a morph set, the older
//! CTRL layout), attachments and hitboxes in the first mesh's data, PHYS, and the animations
//! embedded as AGRP (decode key) + ANIM blocks.

use super::anim::{Rig, encode};
use super::kv::{floats, int_value, list, obj, resource, text};
use super::model::Model;
use super::vbib::{self, Packing};
use super::{mesh, morph, physics, rig};
use ::kv::{Object, Value};

/// Frames per frame block in written animations.
const BLOCK: usize = 4;

fn write_data(m: &Model) -> Value {
    let s = &m.bones;
    let skeleton = obj(vec![
        ("m_boneName", list(s.iter().map(|b| text(&b.name)))),
        ("m_nParent", list(s.iter().map(|b| int_value(b.parent.map_or(-1, |p| p as i64))))),
        ("m_bonePosParent", list(s.iter().map(|b| floats(&b.position)))),
        ("m_boneRotParent", list(s.iter().map(|b| floats(&b.rotation)))),
    ]);
    let groups = m.material_groups.iter().map(|g| obj(vec![("m_name", text(&g.name)), ("m_materials", list(g.materials.iter().map(|x| resource(x))))]));
    let mut members = vec![
        ("m_name", text(&m.name)),
        ("m_modelSkeleton", skeleton),
        ("m_refMeshes", list(m.meshes.iter().map(|_| text("")))),
        ("m_refMeshGroupMasks", list(m.meshes.iter().map(|x| int_value(x.group_mask as i64)))),
        ("m_refLODGroupMasks", list(m.meshes.iter().map(|x| int_value(x.lod_mask as i64)))),
        ("m_meshGroups", list(m.mesh_groups.iter().map(|g| text(g)))),
        ("m_nDefaultMeshGroupMask", int_value(m.default_group_mask as i64)),
        ("m_lodGroupSwitchDistances", floats(&m.lod_distances)),
        ("m_materialGroups", list(groups)),
        ("m_refPhysicsData", list(m.physics_refs.iter().map(|p| resource(p)))),
        ("m_modelInfo", obj(vec![("m_keyValueText", text(&m.key_values))])),
    ];
    if !m.anim_groups.is_empty() {
        members.push(("m_refAnimGroups", list(m.anim_groups.iter().map(|p| resource(p)))));
    }
    obj(members)
}

/// Only models with a rig get the members, so rigless fixtures keep their bytes.
fn with_rig(data: Value, m: &Model) -> Value {
    let Value::Object(Object(mut members)) = data else { return data };
    if m.attachments.is_empty() && m.hitbox_sets.is_empty() {
        return Value::Object(Object(members));
    }
    members.extend(rig::write(&m.attachments, &m.hitbox_sets).into_iter().map(|(k, v)| (k.to_owned(), v)));
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
        let group = next(&blocks);
        ctrl.push(("embedded_animation", obj(vec![("group_data_block", int_value(group)), ("anim_data_block", int_value(group + 1))])));
        blocks.push((*b"AGRP", super::kv3(&obj(vec![("m_decodeKey", key)]))));
        blocks.push((*b"ANIM", super::kv3(&anim)));
    }
    blocks[1].1 = super::kv3(&obj(ctrl));
    let mut refs: Vec<&str> = m.meshes.iter().flat_map(|x| x.mesh.draws.iter().map(|d| d.material.as_str())).collect();
    refs.extend(m.material_groups.iter().flat_map(|g| g.materials.iter().map(String::as_str)));
    refs.extend(m.physics_refs.iter().chain(&m.anim_groups).map(String::as_str));
    refs.extend(m.meshes.iter().filter_map(|x| x.morph.as_ref()).map(|s| s.atlas.as_str()));
    refs.sort_unstable();
    refs.dedup();
    super::write(0, &refs, blocks)
}
