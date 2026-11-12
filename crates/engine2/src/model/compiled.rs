//! Models from compiled `.vmdl_c` files (see `assets::compiled::model`): meshes with their draw
//! calls, materials and morph targets, mesh groups, LOD masks and distances, skins, physics, key
//! values (game data) and the skeleton.

use super::content::{Content, MaterialGroup, ModelMesh};
use crate::mesh::material_ref::MaterialRef;
use crate::mesh::render_mesh::{DrawCall, Morph, MorphDelta, RenderMesh};
use crate::mesh::vertex::Vertex;
use crate::model::physics::compiled as physics;
use abi::structs::BBox;
use anim::{Skeleton, Xform};
use assets::compiled::{mesh, model};
use glam::{Quat, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

/// The compiled file for a resource path managed asks for (with or without `_c` or `.vmdl`).
pub fn compiled_path(path: &str) -> String {
    format!("{}_c", super::source_path(path))
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    crate::fs::read(path, "GAME")
}

type Reader = fn(&[u8], &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<model::Model, assets::compiled::Error>;

fn read_with(path: &str, reader: Reader) -> Option<model::Model> {
    let file = read_file(&compiled_path(path))?;
    let m = reader(&file, &read_file).inspect_err(|e| abi::rt::log::warn(&format!("{path}: {e:?}"))).ok()?;
    if !m.missing.is_empty() {
        abi::rt::log::warn(&format!("{path}: missing {}", m.missing.join(", ")));
    }
    Some(m)
}

/// The compiled model at `path`; `None` when there's no `.vmdl_c` (or it doesn't parse).
pub fn read(path: &str) -> Option<model::Model> {
    read_with(path, model::read)
}

/// As [`read`] without geometry (see `model::read_rig`).
pub fn read_rig(path: &str) -> Option<model::Model> {
    read_with(path, model::read_rig)
}

fn vertex(v: &assets::compiled::vertex::Vertex) -> Vertex {
    let [a, b] = v.uv;
    Vertex { position: v.position, color: v.color, normal: v.normal, tex0: [a[0], a[1], 0.0, 0.0], tex1: [b[0], b[1], 0.0, 0.0], tangent: v.tangent }
}

/// Materials by path, each loaded once per model.
#[derive(Default)]
struct Materials(HashMap<String, Option<Arc<MaterialRef>>>);

impl Materials {
    fn get(&mut self, path: &str) -> Option<Arc<MaterialRef>> {
        if path.is_empty() {
            return None;
        }
        self.0.entry(path.to_owned()).or_insert_with(|| MaterialRef::load(path)).clone()
    }
}

fn morph(t: &assets::compiled::morph::Target) -> Morph {
    let deltas = t.deltas.iter().map(|&(vertex, position, normal)| MorphDelta { vertex: vertex as i32, position, normal }).collect();
    Morph { name: t.name.clone(), deltas }
}

fn skin(v: &assets::compiled::vertex::Vertex) -> ([u8; 4], [u8; 4]) {
    (v.bones, mesh::unorm_weights(v.weights))
}

/// `skinned` (the model has a skeleton to pose) keeps each vertex's bones and weights.
fn render_mesh(name: &str, m: &mesh::Mesh, targets: &[assets::compiled::morph::Target], materials: &mut Materials, skinned: bool) -> Arc<RenderMesh> {
    let vertices: Vec<Vertex> = m.vertices.iter().map(vertex).collect();
    let rm = if skinned {
        RenderMesh::from_skinned_vertices(name, &vertices, &m.vertices.iter().map(skin).collect::<Vec<_>>(), &m.indices)
    } else {
        RenderMesh::from_vertices(name, &vertices, &m.indices)
    };
    {
        let mut s = rm.state();
        s.draw_calls = m
            .draws
            .iter()
            .map(|d| DrawCall {
                material: materials.get(&d.material),
                prim: d.primitive,
                start_index: d.start_index as i32,
                index_count: d.index_count as i32,
                start_vertex: 0,
                vertex_count: vertices.len() as i32,
                tint: (d.tint != [1.0; 4]).then_some(d.tint),
            })
            .collect();
        s.material = s.draw_calls.first().and_then(|d| d.material.clone());
        s.morphs = targets.iter().map(morph).collect();
        s.bounds = m.bounds.map(|(lo, hi)| BBox { mins: crate::math::vec3(lo[0], lo[1], lo[2]), maxs: crate::math::vec3(hi[0], hi[1], hi[2]) });
    }
    rm
}

/// The engine's content for a compiled model.
pub fn content(m: &model::Model) -> Content {
    let mut materials = Materials::default();
    let skinned = m.bones.len() > 1;
    let meshes = m
        .meshes
        .iter()
        .map(|mm| ModelMesh { mesh: render_mesh(&mm.name, &mm.mesh, &mm.targets, &mut materials, skinned), group_mask: mm.group_mask, lod_mask: mm.lod_mask as u8 })
        .collect();
    let material_groups = m
        .material_groups
        .iter()
        .map(|g| MaterialGroup { name: g.name.clone(), materials: g.materials.iter().filter_map(|p| materials.get(p)).collect() })
        .collect();
    let physics = match &m.physics {
        Some(a) => Some(physics::data(physics::EMBEDDED, a)),
        None => m.physics_refs.iter().find_map(|p| physics::load(p)),
    };
    Content {
        meshes,
        groups: m.mesh_groups.clone(),
        default_group_mask: m.default_group_mask,
        material_groups,
        lod_distances: m.lod_distances.clone(),
        physics,
        game_data: m.key_value_tree().map(|v| crate::kv3::node::from_value(&v)),
        ..Content::default()
    }
}

/// The bind-pose skeleton of a compiled model.
pub fn skeleton(m: &model::Model) -> Skeleton {
    let mut s = Skeleton::default();
    for b in &m.bones {
        let q = Quat::from_array(b.rotation);
        let rot = if q.length_squared() > 0.0 { q.normalize() } else { Quat::IDENTITY };
        let local = Xform { pos: Vec3::from(b.position), rot, scale: 1.0 };
        s.push_local(&b.name, b.parent, local);
    }
    s
}
