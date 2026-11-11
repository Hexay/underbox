//! Compiled models (`.vmdl_c`): DATA (skeleton, mesh groups, LOD masks, skins, physics and
//! animation-group references, key values), CTRL (which blocks hold embedded meshes, morph sets,
//! physics and animation), the MDAT/MBUF/MRPH mesh blocks, PHYS, and AGRP/ANIM/ASEQ. Meshes
//! named in `m_refMeshes` are `.vmesh_c` files and animation groups `.vagrp_c` files, both
//! read through `load`. The writer is [`super::model_write`].

use super::anim::{self, Animation, Rig};
use super::flex::Controller;
use super::kv::{self, int, objects, string, strings};
use super::mesh::{self, Mesh};
use super::morph::{self, MorphSet, Target};
use super::physics::{self, Aggregate};
use super::rig::{self, Attachment, HitboxSet};
use super::vbib;
use super::{Error, block_at, format};
use ::kv::{Object, Value};

pub use super::model_write::write;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelMesh {
    pub name: String,
    pub mesh: Mesh,
    /// Bit i: in mesh group i.
    pub group_mask: u64,
    /// Bit i: drawn at LOD i.
    pub lod_mask: u64,
    pub morph: Option<MorphSet>,
    /// The morph set's targets, decoded from its atlas (empty if the atlas is missing).
    pub targets: Vec<Target>,
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
    /// Files the model names (meshes, animation groups) that `load` couldn't supply or decode.
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
    /// From every mesh's data, first of each name kept.
    pub attachments: Vec<Attachment>,
    pub hitbox_sets: Vec<HitboxSet>,
    /// `.vagrp` resources (`m_refAnimGroups`); their animations are in `animations`.
    pub anim_groups: Vec<String>,
    /// Embedded animations (after their sequences), then each group's.
    pub animations: Vec<Animation>,
    /// AnimGraph2 graphs (`m_animGraph2Refs[].m_hGraph`), the default first.
    pub graph2_refs: Vec<String>,
}

impl Model {
    /// The flex controllers: the first morph set's that has any.
    pub fn flex_controllers(&self) -> &[Controller] {
        self.meshes.iter().filter_map(|m| m.morph.as_ref()).map(|m| m.controllers.as_slice()).find(|c| !c.is_empty()).unwrap_or_default()
    }

    /// The key values as a tree (`None` when empty or unparsable).
    pub fn key_value_tree(&self) -> Option<Value> {
        kv::kv3_text(&self.key_values)
    }

    /// The animation graph: key value `anim_graph_resource`, else the default AnimGraph2.
    pub fn anim_graph(&self) -> Option<String> {
        let kv = self.key_value_tree();
        let named = kv.as_ref().and_then(Value::as_object).and_then(|o| o.str("anim_graph_resource")).filter(|s| !s.is_empty());
        named.map(str::to_owned).or_else(|| self.graph2_refs.first().cloned())
    }
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

fn object_at(file: &[u8], index: i64) -> Result<Object, Error> {
    match crate::kv3bin::read(block_at(file, index)?)? {
        Value::Object(o) => Ok(o),
        _ => Err(format(format!("block {index} isn't an object"))),
    }
}

/// A mesh with its data object and morph set.
type Loaded = (String, Mesh, Object, Option<MorphSet>);

/// The CTRL entry for mesh slot `index`, if the mesh is embedded. Without `full` the mesh is
/// left empty (its buffers aren't decoded).
fn embedded(file: &[u8], ctrl: &Object, index: usize, full: bool) -> Result<Option<Loaded>, Error> {
    let Some(e) = objects(ctrl, "embedded_meshes").find(|e| kv::int_or(e, "mesh_index", kv::int_or(e, "m_nMeshIndex", -1)) == index as i64) else { return Ok(None) };
    let old = e.get("vbib_block").is_some();
    let (name, data_block, morph_block) =
        if old { (string(e, "name"), int(e, "data_block"), kv::int_or(e, "morph_block", -1)) } else { (string(e, "m_Name"), int(e, "m_nDataBlock"), kv::int_or(e, "m_nMorphBlock", -1)) };
    let data = object_at(file, data_block)?;
    let morph = (morph_block >= 0).then(|| object_at(file, morph_block)).transpose()?.map(|o| morph::read(&o));
    if !full {
        return Ok(Some((name, Mesh::default(), data, morph)));
    }
    let buffers = match e.get("vbib_block") {
        Some(v) => vbib::read(block_at(file, kv::int_of(v).unwrap_or(-1))?)?,
        None => vbib::from_kv3(e, file)?,
    };
    Ok(Some((name, mesh::build(&data, &buffers)?, data, morph)))
}

/// A `.vmesh_c` named by the model; without `full`, only its data and morph set.
fn external(path: &str, bytes: &[u8], full: bool) -> Result<Loaded, Error> {
    let (mesh, data) = if full {
        mesh::read_with_data(bytes)?
    } else {
        let Value::Object(data) = super::kv3_block(bytes, b"DATA", "DATA")? else { return Err(format("mesh DATA isn't an object")) };
        (Mesh::default(), data)
    };
    let own = crate::container::block(bytes, b"MRPH")?.map(crate::kv3bin::read).transpose()?;
    Ok((path.to_owned(), mesh, data, own.as_ref().and_then(Value::as_object).map(morph::read)))
}

pub(crate) fn compiled_name(path: &str) -> String {
    if path.ends_with("_c") { path.to_owned() } else { format!("{path}_c") }
}

/// A mesh's own morph set, else the `.vmorf` its data names; and with `full`, its targets from the atlas.
fn morph_targets(data: &Object, own: Option<MorphSet>, load: &dyn Fn(&str) -> Option<Vec<u8>>, full: bool) -> (Option<MorphSet>, Vec<Target>) {
    let set = own.or_else(|| {
        let path = string(data, "m_morphSet");
        let bytes = (!path.is_empty()).then(|| load(&compiled_name(&path))).flatten()?;
        let block = crate::container::block(&bytes, b"MRPH").ok().flatten().or_else(|| crate::container::block(&bytes, b"DATA").ok().flatten())?;
        crate::kv3bin::read(block).ok()?.as_object().map(morph::read)
    });
    let targets = set
        .as_ref()
        .filter(|s| full && !s.atlas.is_empty())
        .and_then(|s| {
            let t = super::texture::read(&load(&compiled_name(&s.atlas))?).ok()?;
            Some(s.targets(&super::texture::decode(&t, 0, 0, 0)?))
        })
        .unwrap_or_default();
    (set, targets)
}

/// Just the skeleton: only DATA is decoded.
pub fn bones(file: &[u8]) -> Result<Vec<Bone>, Error> {
    let data = super::kv3_block(file, b"DATA", "DATA")?;
    Ok(skeleton(data.as_object().ok_or_else(|| format("model DATA isn't an object"))?))
}

fn animations(file: &[u8], ctrl: &Object, model: &mut Model, load: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<(), Error> {
    let flex: Vec<String> = model.flex_controllers().iter().map(|c| c.name.clone()).collect();
    let bones = model.bones.clone();
    let rig = Rig { bones: &bones, flex: &flex };
    if let Some(e) = ctrl.get("embedded_animation").and_then(Value::as_object) {
        let group = object_at(file, int(e, "group_data_block"))?;
        let mut anims = anim::read(&object_at(file, int(e, "anim_data_block"))?, kv::object(&group, "m_decodeKey"), &rig);
        if let Some(seq) = Some(kv::int_or(e, "seqgroup_data_block", -1)).filter(|&b| b > 0) {
            anims = anim::sequences(&object_at(file, seq)?, anims);
        }
        model.animations.extend(anims);
    }
    for path in model.anim_groups.clone().iter().filter(|p| !p.is_empty()) {
        match load(&compiled_name(path)).map(|bytes| anim::read_group(&bytes, &rig, load)) {
            Some(Ok(a)) => model.animations.extend(a),
            Some(Err(e)) => model.missing.push(format!("{path}: {e}")),
            None => model.missing.push(path.clone()),
        }
    }
    Ok(())
}

/// The model in `file`; `load` reads another compiled file by resource path.
pub fn read(file: &[u8], load: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model, Error> {
    read_parts(file, load, true)
}

/// Everything but geometry: meshes come back empty and morph sets without targets, so only
/// the skeleton, rig, flex data, key values and animations are decoded.
pub fn read_rig(file: &[u8], load: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Model, Error> {
    read_parts(file, load, false)
}

fn read_parts(file: &[u8], load: &dyn Fn(&str) -> Option<Vec<u8>>, full: bool) -> Result<Model, Error> {
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
        anim_groups: strings(data, "m_refAnimGroups"),
        graph2_refs: objects(data, "m_animGraph2Refs").map(|g| string(g, "m_hGraph")).filter(|g| !g.is_empty()).collect(),
        ..Model::default()
    };
    for (i, r) in refs.iter().enumerate() {
        let path = r.as_str().unwrap_or_default();
        let found = match embedded(file, &ctrl, i, full)? {
            Some(m) => Some(m),
            None if path.is_empty() => None,
            None => match load(&compiled_name(path)) {
                Some(bytes) => Some(external(path, &bytes, full)?),
                None => {
                    model.missing.push(path.to_owned());
                    None
                }
            },
        };
        let Some((name, mesh, data, own)) = found else { continue };
        let (morph, targets) = morph_targets(&data, own, load, full);
        for a in rig::attachments(&data) {
            if !model.attachments.iter().any(|x| x.name.eq_ignore_ascii_case(&a.name)) {
                model.attachments.push(a);
            }
        }
        for s in rig::hitbox_sets(&data) {
            if !model.hitbox_sets.iter().any(|x| x.name.eq_ignore_ascii_case(&s.name)) {
                model.hitbox_sets.push(s);
            }
        }
        let mask = |m: &[i64]| m.get(i).map_or(u64::MAX, |&v| v as u64);
        model.meshes.push(ModelMesh { name, mesh, group_mask: mask(&groups), lod_mask: mask(&lods), morph, targets });
    }
    if let Some(block) = kv::object(&ctrl, "embedded_physics").get("phys_data_block").and_then(kv::int_of) {
        model.physics = Some(physics::from_kv3(&object_at(file, block)?));
    }
    animations(file, &ctrl, &mut model, load)?;
    Ok(model)
}
