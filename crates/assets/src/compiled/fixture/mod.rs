//! Compiled resources with known contents, for tests and the mapworld corpus: a crate model
//! (two LODs, two draw calls, a skin, a bone, a hull in its PHYS block) using a compiled material
//! whose colour texture is a BC1 checker with mips, and map physics (a ground mesh and a hull).

use super::material::Material;
use super::mesh::{DrawCall, Mesh, TRIANGLES};
use super::model::{Bone, MaterialGroup, Model, ModelMesh};
use super::physics::{Aggregate, IDENTITY, Ids, Part, Shapes, TriMesh};
use super::texture::{Format, Texture};
use super::vbib::Packing;
use super::vertex::Vertex;
use crate::bitmap::bc;

mod rigged;
pub use rigged::*;

pub const MODEL: &str = "models/compiled/crate.vmdl";
pub const MATERIAL: &str = "materials/compiled/crate.vmat";
pub const SKIN_MATERIAL: &str = "materials/compiled/crate_dark.vmat";
pub const TEXTURE: &str = "materials/compiled/crate_color.vtex";
/// The crate's half width; it stands on z = 0 and is `2 * HALF` tall.
pub const HALF: f32 = 16.0;
pub const CHECKER: [[u8; 4]; 2] = [[232, 140, 40, 255], [90, 50, 20, 255]];
/// The top face's tint.
pub const TOP_TINT: [f32; 4] = [0.6, 0.9, 0.6, 1.0];
pub const SIZE: usize = 16;

/// The colour texture's mip 0: 4x4 checker cells.
pub fn checker(size: usize) -> Vec<[u8; 4]> {
    (0..size * size).map(|i| CHECKER[(i % size / 4 + i / size / 4) % 2]).collect()
}

fn half_size(texels: &[[u8; 4]], size: usize) -> Vec<[u8; 4]> {
    let n = size / 2;
    let at = |x: usize, y: usize| texels[y * size + x];
    (0..n * n)
        .map(|i| {
            let (x, y) = (i % n * 2, i / n * 2);
            std::array::from_fn(|c| ((u32::from(at(x, y)[c]) + u32::from(at(x + 1, y)[c]) + u32::from(at(x, y + 1)[c]) + u32::from(at(x + 1, y + 1)[c]) + 2) / 4) as u8)
        })
        .collect()
}

pub fn texture() -> Texture {
    let (mut texels, mut size, mut levels) = (checker(SIZE), SIZE, Vec::new());
    loop {
        levels.push(bc::blocks(&texels, size, size, |b| bc::bc1(b).to_vec()));
        if size == 1 {
            break;
        }
        texels = half_size(&texels, size);
        size /= 2;
    }
    Texture { width: SIZE as u16, height: SIZE as u16, depth: 1, format: Format::DXT1, reflectivity: [0.5, 0.35, 0.15, 0.0], levels, ..Texture::default() }
}

pub fn material(name: &str, tint: [f32; 4]) -> Material {
    Material {
        name: name.to_owned(),
        shader: "shaders/complex.shader".into(),
        ints: vec![("F_SPECULAR".into(), 0)],
        floats: vec![("g_flModelTintAmount".into(), 1.0)],
        vectors: vec![("g_vColorTint".into(), tint)],
        textures: vec![("g_tColor".into(), TEXTURE.into())],
        ..Material::default()
    }
}

/// A box of half width `h` and height `2h` standing on z = 0: sides and bottom first, top last.
fn cube(h: f32) -> Mesh {
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    let mut mesh = Mesh::default();
    for (n, u, v) in faces {
        let base = mesh.vertices.len() as u32;
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p: [f32; 3] = std::array::from_fn(|i| n[i] * h + u[i] * a * h + v[i] * b * h + if i == 2 { h } else { 0.0 });
            mesh.vertices.push(Vertex { position: p, normal: n, tangent: [u[0], u[1], u[2], 1.0], uv: [[(a + 1.0) / 2.0, (1.0 - b) / 2.0], [0.0; 2]], ..Vertex::default() });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| base + i));
    }
    let draw = |start, count, tint| DrawCall { material: MATERIAL.into(), primitive: TRIANGLES, start_index: start, index_count: count, tint };
    mesh.draws = vec![draw(0, 30, [1.0; 4]), draw(30, 6, TOP_TINT)];
    mesh.bounds = Some(([-h, -h, 0.0], [h, h, 2.0 * h]));
    mesh
}

/// Eight corners of an axis-aligned box.
pub fn corners(lo: [f32; 3], hi: [f32; 3]) -> Vec<[f32; 3]> {
    (0..8).map(|i| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]).collect()
}

fn part(shapes: Shapes, mass: f32) -> Part {
    Part { bone: None, bind: IDENTITY, flags: 0, mass, inertia_scale: 1.0, linear_damping: 0.0, angular_damping: 0.0, shapes }
}

pub fn model() -> Model {
    let lod1 = cube(HALF * 0.5);
    let hull = Shapes { hulls: vec![(corners([-HALF, -HALF, 0.0], [HALF, HALF, 2.0 * HALF]), Ids::default())], ..Shapes::default() };
    Model {
        name: MODEL.into(),
        meshes: vec![
            ModelMesh { name: "crate".into(), mesh: cube(HALF), group_mask: 1, lod_mask: 1, ..ModelMesh::default() },
            ModelMesh { name: "crate_lod1".into(), mesh: lod1, group_mask: 1, lod_mask: 2, ..ModelMesh::default() },
        ],
        mesh_groups: vec!["body".into()],
        default_group_mask: 1,
        lod_distances: vec![0.0, 400.0],
        material_groups: vec![MaterialGroup { name: "default".into(), materials: vec![MATERIAL.into()] }, MaterialGroup { name: "dark".into(), materials: vec![SKIN_MATERIAL.into()] }],
        bones: vec![Bone { name: "root".into(), parent: None, position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0] }],
        physics: Some(Aggregate { parts: vec![part(hull, 20.0)], surfaces: vec![0], attributes: vec![vec!["solid".into()]], ..Aggregate::default() }),
        key_values: String::new(),
        ..Model::default()
    }
}

/// A map's `world_physics`: a ground quad of half size `ground` at z = 0, plus box hulls.
pub fn world_physics(ground: f32, boxes: &[([f32; 3], [f32; 3])]) -> Aggregate {
    let g = ground;
    let quad = TriMesh { vertices: vec![[-g, -g, 0.0], [g, -g, 0.0], [g, g, 0.0], [-g, g, 0.0]], triangles: vec![[0, 1, 2], [0, 2, 3]], materials: vec![], ids: Ids::default() };
    let hulls = boxes.iter().map(|(lo, hi)| (corners(*lo, *hi), Ids::default())).collect();
    let attributes = vec![vec!["solid".into(), "world".into()]];
    Aggregate { parts: vec![part(Shapes { meshes: vec![quad], hulls, ..Shapes::default() }, 0.0)], surfaces: vec![0], attributes, ..Aggregate::default() }
}

/// (compiled path, bytes) of the crate model, its materials and texture.
pub fn crate_files() -> Vec<(String, Vec<u8>)> {
    vec![
        (format!("{MODEL}_c"), super::model::write(&model(), Packing::MeshoptZstd)),
        (format!("{MATERIAL}_c"), super::material::write(&material(MATERIAL, [1.0, 1.0, 1.0, 0.0]))),
        (format!("{SKIN_MATERIAL}_c"), super::material::write(&material(SKIN_MATERIAL, [0.4, 0.4, 0.4, 0.0]))),
        (format!("{TEXTURE}_c"), super::texture::write(&texture(), true)),
    ]
}
