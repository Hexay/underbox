//! `CModel`'s body parts and mesh groups, skins and materials, LODs, game data and the
//! per-model answers that have no data behind them yet (flexes, visemes, weight lists).
//!
//! Body parts come from mesh-group names: `ModelBuilder` names each choice `{part}_@{choice}`
//! (ModelBuilder.Mesh.cs), so a part is every group sharing a prefix, in first-seen order.

use super::{Model, on};
use crate::kv3;
use crate::mesh::material_ref;
use abi::slots::c_model as m;
use abi::{Registry, StrRet, StringToken, bind};
use std::ffi::CStr;

/// A body part: its name and its choices (a mesh group's name and mask).
pub(super) struct Part {
    pub name: String,
    pub choices: Vec<(String, u64)>,
}

/// A file model without mesh groups has one part, "Body", whose one choice is the file's stem and
/// covers every mesh (real, probe `MODEL`).
pub(super) fn parts(m: &Model) -> Vec<Part> {
    if m.content.groups.is_empty() {
        if m.name.starts_with(super::RUNTIME_PREFIX) {
            return Vec::new();
        }
        let stem = std::path::Path::new(&m.name).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        return vec![Part { name: "Body".into(), choices: vec![(stem, u64::MAX)] }];
    }
    let mut parts: Vec<Part> = Vec::new();
    for (i, g) in m.content.groups.iter().enumerate() {
        let name = g.rsplit_once("_@").map_or(g.as_str(), |(p, _)| p);
        let choice = (g.clone(), bit(i));
        match parts.iter_mut().find(|p| p.name == name) {
            Some(p) => p.choices.push(choice),
            None => parts.push(Part { name: name.to_owned(), choices: vec![choice] }),
        }
    }
    parts
}

fn on_part<R: Default>(r: super::Ref, part: i32, f: impl FnOnce(&Model, &Part) -> R) -> R {
    on(r, |m| usize::try_from(part).ok().and_then(|i| parts(m).into_iter().nth(i)).map(|p| f(m, &p)).unwrap_or_default())
}

fn bit(group: usize) -> u64 {
    1u64.checked_shl(group as u32).unwrap_or(0)
}

fn choice(p: &Part, n: i32) -> Option<&(String, u64)> {
    usize::try_from(n).ok().and_then(|n| p.choices.get(n))
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Skins, as the model lists them: real reports none for a model without `m_materialGroups`.
pub(super) fn group_name(m: &Model, i: i32) -> Option<String> {
    usize::try_from(i).ok().and_then(|i| m.content.material_groups.get(i)).map(|g| g.name.clone())
}

/// Every distinct material, in first-use order: skins first, then draw calls.
fn materials(m: &Model) -> Vec<std::sync::Arc<material_ref::MaterialRef>> {
    let skins = m.content.material_groups.iter().flat_map(|g| g.materials.iter().cloned());
    let calls = m.content.meshes.iter().flat_map(|x| x.mesh.draw_calls()).filter_map(|d| d.material);
    let mut out: Vec<std::sync::Arc<material_ref::MaterialRef>> = Vec::new();
    for mat in skins.chain(calls) {
        if !out.iter().any(|o| std::sync::Arc::ptr_eq(o, &mat)) {
            out.push(mat);
        }
    }
    out
}

/// Highest LOD any mesh is limited to, or -1 when every mesh draws at every LOD.
pub(super) fn max_lod(m: &Model) -> i32 {
    let masks = m.content.meshes.iter().map(|x| x.lod_mask).filter(|&l| l != super::content::ALL_LODS);
    masks.map(|l| 7 - l.leading_zeros() as i32).max().unwrap_or(-1)
}

/// The LOD for an on-screen size: past each switch distance (a screen size) the next LOD.
pub(super) fn lod_for_screen_size(m: &Model, pixels: f32, scale: f32) -> i32 {
    let size = pixels * scale;
    let last = max_lod(m);
    let d = &m.content.lod_distances;
    (1..=last).filter(|&i| d.get(i as usize).is_some_and(|&x| x > 0.0 && size < x)).max().unwrap_or(0)
}

pub fn register(r: &mut Registry) {
    bind!(r, m::GET_NUM_MESH_GROUPS, |r| on(r, |m| m.content.groups.len() as i32));
    bind!(r, m::GET_DEFAULT_MESH_GROUP_MASK, |r| on(r, |m| m.content.default_group_mask));
    bind!(r, m::GET_MESH_GROUP_MASK_FOR_MESH, |r, i| on(r, |m| m.content.mesh(i).map_or(0, |x| x.group_mask)));
    bind!(r, m::GET_NUM_BODY_PARTS, |r| on(r, |m| parts(m).len() as i32));
    bind!(r, m::GET_BODY_PART_NAME, |r, p| StrRet::from(on_part(r, p, |_, p| Some(p.name.clone()))));
    bind!(r, m::GET_BODY_PART_FOR_NAME, |r, name| {
        let name = text(name);
        on(r, |m| parts(m).iter().position(|p| p.name.eq_ignore_ascii_case(&name)).map_or(-1, |i| i as i32))
    });
    bind!(r, m::GET_NUM_BODY_PART_MESHES, |r, p| on_part(r, p, |_, p| p.choices.len() as i32));
    bind!(r, m::GET_BODY_PART_MASK, |r, p| on_part(r, p, |_, p| p.choices.iter().fold(0, |m, c| m | c.1)));
    bind!(r, m::GET_BODY_PART_MESH_MASK, |r, p, n| on_part(r, p, |_, p| choice(p, n).map_or(0, |c| c.1)));
    bind!(r, m::GET_BODY_PART_MESH_NAME, |r, p, n| StrRet::from(on_part(r, p, |_, p| choice(p, n).map(|c| c.0.clone()))));
    bind!(r, m::FIND_MESH_INDEX_FOR_MASK, |r, p, mask| on_part(r, p, |_, p| p.choices.iter().position(|c| mask & c.1 != 0).map_or(-1, |i| i as i32)));

    bind!(r, m::GET_NUM_MATERIAL_GROUPS, |r| on(r, |m| m.content.material_groups.len() as i32));
    bind!(r, m::GET_MATERIAL_GROUP_NAME, |r, i| StrRet::from(on(r, |m| group_name(m, i))));
    bind!(r, m::GET_MATERIAL_GROUP_INDEX, |r, name| {
        on(r, |m| m.content.material_groups.iter().position(|g| StringToken::of(&g.name) == name).map_or(-1, |i| i as i32))
    });
    bind!(r, m::GET_NUM_MATERIALS_IN_GROUP, |r, g| on(r, |m| usize::try_from(g).ok().and_then(|g| m.content.material_groups.get(g)).map_or(0, |g| g.materials.len() as i32)));
    bind!(r, m::GET_MATERIAL_IN_GROUP, |r, g, i| {
        on(r, |m| material_ref::handle(usize::try_from(g).ok().and_then(|g| m.content.material_groups.get(g)).and_then(|g| g.materials.get(usize::try_from(i).ok()?))))
    });
    bind!(r, m::GET_MATERIAL_INDEX_COUNT, |r| on(r, |m| materials(m).len() as i32));
    bind!(r, m::GET_MATERIAL_BY_INDEX, |r, i| on(r, |m| material_ref::handle(usize::try_from(i).ok().and_then(|i| materials(m).get(i).cloned()).as_ref())));

    bind!(r, m::COMPUTE_MAX_LOD_LEVEL_USED_BY_MODEL, |r| on(r, max_lod));
    bind!(r, m::LOD_LEVEL_FOR_SCREEN_SIZE, |r, pixels, scale| on(r, |m| lod_for_screen_size(m, pixels, scale)));
    bind!(r, m::GET_LOD_SWITCH_DISTANCE_COUNT, |r| on(r, |m| m.content.lod_distances.len() as i32));
    bind!(r, m::GET_LOD_SWITCH_DISTANCE, |r, i| on(r, |m| usize::try_from(i).ok().and_then(|i| m.content.lod_distances.get(i).copied()).unwrap_or(0.0)));

    bind!(r, m::FIND_MODEL_SUB_KEY, |r, key| {
        let key = text(key);
        on(r, |m| kv3::node::handle_opt(m.content.game_data.as_ref().and_then(|d| d.member(&key))))
    });

    // Models never derive from another here, are never translucent as a whole, and carry no
    // visemes or weight lists (`anim` reports zero of each; flex controllers are `anim::flex`).
    bind!(r, m::GET_BASE_MODEL_NAME, |_| StrRet::from(None::<String>));
    bind!(r, m::GET_BASE_MODEL, |_| super::Ref::NULL);
    bind!(r, m::IS_TRANSLUCENT, |_| false);
    bind!(r, m::IS_TRANSLUCENT_TWO_PASS, |_| false);
    bind!(r, m::HAS_SKINNED_MESHES, |r| on(r, |m| !crate::anim::data::by_name(&m.name).anim.skeleton.is_empty()));
    bind!(r, m::GET_VISEME_INDEX, |_, _| -1);
    bind!(r, m::GET_VISEME_MORPH, |_, _, _| 0.0);
    bind!(r, m::GET_WEIGHTLIST_NAME, |_, _| StrRet::from(None::<String>));
}
