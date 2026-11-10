//! What a model holds besides its name: render meshes with their mesh-group and LOD masks,
//! mesh groups, skins, LOD switch distances, physics, game data and builder metadata.
//! `CModelBuilder` fills one; a compiled `.vmdl` makes one from its meshes.

use super::physics::data::PhysicsData;
use crate::kv3::node::Node;
use crate::math::{from_v3, v3};
use crate::mesh::material_ref::MaterialRef;
use crate::mesh::render_mesh::RenderMesh;
use crate::mesh::vertex::Vertex;
use abi::structs::{BBox, Vector3};
use std::sync::Arc;

/// LOD mask meaning "every LOD" (ModelBuilder.Mesh.cs).
pub const ALL_LODS: u8 = 0xFF;

pub struct ModelMesh {
    pub mesh: Arc<RenderMesh>,
    pub group_mask: u64,
    pub lod_mask: u8,
}

/// A skin (`CBuilderMaterialGroup`).
#[derive(Clone, Default)]
pub struct MaterialGroup {
    pub name: String,
    pub materials: Vec<Arc<MaterialRef>>,
}

/// A triangle soup managed gave for traces (`SetTraceMesh`), in model space.
#[derive(Debug, Clone, Default)]
pub struct TraceMesh {
    pub vertices: Vec<Vector3>,
    pub indices: Vec<u32>,
}

pub struct Content {
    pub meshes: Vec<ModelMesh>,
    /// Mesh-group names; a group's index is its bit in the mesh-group masks.
    pub groups: Vec<String>,
    pub default_group_mask: u64,
    pub material_groups: Vec<MaterialGroup>,
    pub lod_distances: Vec<f32>,
    pub physics: Option<Arc<PhysicsData>>,
    pub game_data: Option<Arc<Node>>,
    pub trace: Option<TraceMesh>,
    pub mass: f32,
    /// Zero bounds from managed mean "work it out" (ModelBuilder.Metadata.cs).
    pub hull_bounds: Option<BBox>,
    pub view_bounds: Option<BBox>,
    pub eye_position: Vector3,
    pub max_eye_deflection: f32,
}

impl Default for Content {
    fn default() -> Self {
        Self {
            meshes: Vec::new(),
            groups: Vec::new(),
            default_group_mask: u64::MAX,
            material_groups: Vec::new(),
            lod_distances: Vec::new(),
            physics: None,
            game_data: None,
            trace: None,
            mass: 0.0,
            hull_bounds: None,
            view_bounds: None,
            eye_position: Vector3::default(),
            max_eye_deflection: 0.0,
        }
    }
}

impl Content {
    /// A compiled model's meshes, each in every mesh group and LOD.
    pub fn from_meshes(meshes: &[assets::model::Mesh]) -> Self {
        let meshes = meshes
            .iter()
            .map(|m| {
                let vertices: Vec<Vertex> = m.vertices.iter().map(Vertex::from).collect();
                ModelMesh { mesh: RenderMesh::from_vertices(&m.name, &vertices, &m.indices), group_mask: u64::MAX, lod_mask: ALL_LODS }
            })
            .collect();
        Self { meshes, ..Self::default() }
    }

    pub fn mesh(&self, i: i32) -> Option<&ModelMesh> {
        usize::try_from(i).ok().and_then(|i| self.meshes.get(i))
    }

    /// The union of the meshes' bounds; zero without meshes.
    pub fn mesh_bounds(&self) -> BBox {
        let mut it = self.meshes.iter().map(|m| m.mesh.bounds());
        let Some(first) = it.next() else { return BBox::default() };
        it.fold(first, union)
    }

    /// Bounds managed set on the builder, else the meshes'.
    pub fn render_bounds(&self) -> BBox {
        self.view_bounds.unwrap_or_else(|| self.mesh_bounds())
    }
}

pub fn union(a: BBox, b: BBox) -> BBox {
    BBox { mins: from_v3(v3(a.mins).min(v3(b.mins))), maxs: from_v3(v3(a.maxs).max(v3(b.maxs))) }
}

/// `None` for the all-zero box managed sends when nothing was set.
pub fn set_bounds(mins: Vector3, maxs: Vector3) -> Option<BBox> {
    (v3(mins) != glam::Vec3::ZERO || v3(maxs) != glam::Vec3::ZERO).then_some(BBox { mins, maxs })
}
