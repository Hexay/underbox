//! Shadow casters batched as native's shadow views draw them: every placeholder-class draw of the
//! same mesh range becomes one instanced draw of the caster program (`render::Caster`), its world
//! matrices on an instance stream. Draws that bind their own program (skinned vertex-cache draws)
//! stay as they are, after the instanced ones.

use super::{Draw, device};
use glam::Mat4;
use render::Caster;
use std::sync::{Arc, OnceLock};

pub fn program() -> Option<&'static Caster> {
    static CASTER: OnceLock<Caster> = OnceLock::new();
    let gpu = device::gpu()?;
    Some(CASTER.get_or_init(|| Caster::new(gpu)))
}

/// What makes two draws one mesh range: buffers by identity, offsets, range, layout and state.
fn same_mesh(a: &Draw, b: &Draw) -> bool {
    let indices = |d: &Draw| d.indices.as_ref().map(|(b, f)| (Arc::as_ptr(b), *f));
    Arc::ptr_eq(&a.vertices, &b.vertices)
        && a.vertex_offset == b.vertex_offset
        && indices(a) == indices(b)
        && a.range == b.range
        && a.base_vertex == b.base_vertex
        && a.state == b.state
        && a.vertex.first() == b.vertex.first()
}

/// `draws` seen through `view_proj`, same-mesh placeholder draws merged into instanced caster
/// draws in order of first appearance; each instance is a draw's `uniforms.model`.
pub fn instanced(draws: Vec<Draw>, view_proj: Mat4) -> Vec<Draw> {
    let Some(gpu) = device::gpu() else { return draws };
    let mut groups: Vec<(Draw, Vec<Mat4>)> = Vec::new();
    let mut bound = Vec::new();
    for d in draws {
        if d.shaded.is_some() || d.vertex.is_empty() {
            bound.push(d);
            continue;
        }
        match groups.iter_mut().find(|(g, _)| same_mesh(g, &d)) {
            Some((_, worlds)) => worlds.push(d.uniforms.model),
            None => {
                let world = d.uniforms.model;
                groups.push((d, vec![world]));
            }
        }
    }
    let mut out: Vec<Draw> = groups
        .into_iter()
        .map(|(d, worlds)| {
            let stream = Arc::new(Caster::instances(gpu, &worlds));
            let uniforms = render::pass::DrawUniforms { mvp: view_proj, model: Mat4::IDENTITY, ..d.uniforms };
            Draw { vertex: Caster::vertex_keys(&d.vertex[0]), instances: worlds.len() as u32, uniforms, transforms: Some(stream), ..d }
        })
        .collect();
    out.extend(bound);
    out
}
