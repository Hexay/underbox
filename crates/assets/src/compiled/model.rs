//! Compiled models (`.vmdl_c`): DATA (skeleton, mesh groups, LOD masks, skins, physics
//! references), CTRL (which blocks hold embedded meshes and physics), the MDAT/MBUF mesh blocks
//! and PHYS. Meshes named in `m_refMeshes` are separate `.vmesh_c` files read through `load`.

use super::kv::{self, floats, int, int_value, list, obj, objects, resource, string, strings, text};
use super::mesh::{self, Mesh};
use super::physics::{self, Aggregate};
use super::vbib::{self, Packing};
use super::{Error, block_at, format};
use ::kv::{Object, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct ModelMesh {
    pub name: String,
    pub mesh: Mesh,
    /// Bit i: in mesh group i.
    pub group_mask: u64,
    /// Bit i: drawn at LOD i.
    pub lod_mask: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    /// Bind pose relative to the parent.
    pub position: [f32; 3],
    /// xyzw.
    pub rotation: [f32; 4],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaterialGroup {
    pub name: String,
    pub materials: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub name: String,
    pub meshes: Vec<ModelMesh>,
    /// Mesh files the model names that `load` couldn't supply.
    pub missing: Vec<String>,
    pub mesh_groups: Vec<String>,
    pub default_group_mask: u64,
    pub lod_distances: Vec<f32>,
    /// Skins: group 0's materials are the defaults the others replace by position.
    pub material_groups: Vec<MaterialGroup>,
    pub bones: Vec<Bone>,
    /// `.vphys` resources the model uses.
    pub physics_refs: Vec<String>,
    /// The model's own PHYS block.
    pub physics: Option<Aggregate>,
    /// `m_modelInfo.m_keyValueText` (KV3 text).
    pub key_values: String,
}

fn skeleton(o: &Object) -> Vec<Bone> {
    let s = kv::object(o, "m_modelSkeleton");
    let (names, parents) = (strings(s, "m_boneName"), kv::ints(s, "m_nParent"));
    let (pos, rot) = (kv::array(s, "m_bonePosParent"), kv::array(s, "m_boneRotParent"));
    names
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            let parent = parents.get(i).and_then(|&p| usize::try_from(p).ok()).filter(|&p| p < i);
            let rotation = rot.get(i).map_or([0.0, 0.0, 0.0, 1.0], |r| kv::floats_of(Some(r)));
            Bone { name, parent, position: kv::floats_of(pos.get(i)), rotation }
        })
        .collect()
}

/// (mesh, buffers) of the CTRL entry for mesh slot `index`, if the mesh is embedded.
fn embedded(file: &[u8], ctrl: &Object, index: usize) -> Result<Option<(String, Mesh)>, Error> {
    let Some(e) = objects(ctrl, "embedded_meshes").find(|e| kv::int_or(e, "mesh_index", kv::int_or(e, "m_nMeshIndex", -1)) == index as i64) else { return Ok(None) };
    let (name, data_block) = if e.get("vbib_block").is_some() { (string(e, "name"), int(e, "data_block")) } else { (string(e, "m_Name"), int(e, "m_nDataBlock")) };
    let data = crate::kv3bin::read(block_at(file, data_block)?)?;
    let data = data.as_object().ok_or_else(|| format("MDAT isn't an object"))?;
    let buffers = match e.get("vbib_block") {
        Some(v) => vbib::read(block_at(file, kv::int_of(v).unwrap_or(-1))?)?,
        None => vbib::from_kv3(e, file)?,
    };
    Ok(Some((name, mesh::build(data, &buffers)?)))
}

fn compiled_name(path: &str) -> String {
    if path.ends_with("_c") { path.to_owned() } else { format!("{path}_c") }
}

/// The model in `file`; `load` reads another compiled file by resource path.
pub fn read(file: &[u8], load: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model, Error> {
    let data = super::kv3_block(file, b"DATA", "DATA")?;
    let data = data.as_object().ok_or_else(|| format("model DATA isn't an object"))?;
    let ctrl = crate::container::block(file, b"CTRL")?.map(crate::kv3bin::read).transpose()?;
    let ctrl = ctrl.as_ref().and_then(Value::as_object).cloned().unwrap_or_default();
    let refs = kv::array(data, "m_refMeshes");
    let (groups, lods) = (kv::ints(data, "m_refMeshGroupMasks"), kv::ints(data, "m_refLODGroupMasks"));
    let mut model = Model {
        name: string(data, "m_name"),
        mesh_groups: strings(data, "m_meshGroups"),
        default_group_mask: kv::int_or(data, "m_nDefaultMeshGroupMask", -1) as u64,
        lod_distances: kv::array(data, "m_lodGroupSwitchDistances").iter().filter_map(Value::as_f64).map(|d| d as f32).collect(),
        material_groups: objects(data, "m_materialGroups").map(|g| MaterialGroup { name: string(g, "m_name"), materials: strings(g, "m_materials") }).collect(),
        bones: skeleton(data),
        physics_refs: strings(data, "m_refPhysicsData"),
        key_values: string(kv::object(data, "m_modelInfo"), "m_keyValueText"),
        ..Model::default()
    };
    for (i, r) in refs.iter().enumerate() {
        let path = r.as_str().unwrap_or_default();
        let found = match embedded(file, &ctrl, i)? {
            Some(m) => Some(m),
            None if path.is_empty() => None,
            None => match load(&compiled_name(path)) {
                Some(bytes) => Some((path.to_owned(), mesh::read(&bytes)?)),
                None => {
                    model.missing.push(path.to_owned());
                    None
                }
            },
        };
        if let Some((name, mesh)) = found {
            let mask = |m: &[i64]| m.get(i).map_or(u64::MAX, |&v| v as u64);
            model.meshes.push(ModelMesh { name, mesh, group_mask: mask(&groups), lod_mask: mask(&lods) });
        }
    }
    if let Some(block) = kv::object(&ctrl, "embedded_physics").get("phys_data_block").and_then(kv::int_of) {
        let phys = crate::kv3bin::read(block_at(file, block)?)?;
        model.physics = phys.as_object().map(physics::from_kv3);
    }
    Ok(model)
}

fn write_data(m: &Model) -> Value {
    let s = &m.bones;
    let skeleton = obj(vec![
        ("m_boneName", list(s.iter().map(|b| text(&b.name)))),
        ("m_nParent", list(s.iter().map(|b| int_value(b.parent.map_or(-1, |p| p as i64))))),
        ("m_bonePosParent", list(s.iter().map(|b| floats(&b.position)))),
        ("m_boneRotParent", list(s.iter().map(|b| floats(&b.rotation)))),
    ]);
    let groups = m.material_groups.iter().map(|g| obj(vec![("m_name", text(&g.name)), ("m_materials", list(g.materials.iter().map(|x| resource(x))))]));
    obj(vec![
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
    ])
}

/// A `.vmdl_c` with every mesh embedded (an MDAT and MBUF block each, the older CTRL layout) and
/// `physics` as its PHYS block. `missing` isn't written.
pub fn write(m: &Model, packing: Packing) -> Vec<u8> {
    // Block 0 is RERL (see `super::write`), DATA is 1, CTRL 2; meshes follow.
    let mut blocks = vec![(*b"DATA", super::kv3(&write_data(m))), (*b"CTRL", Vec::new())];
    let mut entries = Vec::new();
    for (i, mm) in m.meshes.iter().enumerate() {
        let first = blocks.len() as i64 + 1;
        blocks.push((*b"MDAT", super::kv3(&mesh::write_data(&mm.mesh))));
        blocks.push((*b"MBUF", vbib::write(&mesh::write_buffers(&mm.mesh), packing)));
        entries.push(obj(vec![("name", text(&mm.name)), ("mesh_index", int_value(i as i64)), ("data_block", int_value(first)), ("vbib_block", int_value(first + 1)), ("morph_block", int_value(-1))]));
    }
    let mut ctrl = vec![("embedded_meshes", list(entries))];
    if let Some(p) = &m.physics {
        ctrl.push(("embedded_physics", obj(vec![("phys_data_block", int_value(blocks.len() as i64 + 1)), ("parts", int_value(p.parts.len() as i64))])));
        blocks.push((*b"PHYS", super::kv3(&physics::write_kv3(p))));
    }
    blocks[1].1 = super::kv3(&obj(ctrl));
    let mut refs: Vec<&str> = m.meshes.iter().flat_map(|x| x.mesh.draws.iter().map(|d| d.material.as_str())).collect();
    refs.extend(m.material_groups.iter().flat_map(|g| g.materials.iter().map(String::as_str)));
    refs.extend(m.physics_refs.iter().map(String::as_str));
    refs.sort_unstable();
    refs.dedup();
    super::write(0, &refs, blocks)
}
