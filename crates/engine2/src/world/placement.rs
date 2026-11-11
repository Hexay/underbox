//! What a world-node scene object places: one model at its transform, or, for an aggregate with
//! fragments, one object per fragment over a one-draw copy of the aggregate's model (so each
//! carries its own transform, tint and LOD groups; research/world.md "Engine side").

use abi::structs::Vector4;
use glam::{Affine3A, Mat3, Vec3};
use world::node::{Fragment, Matrix, SceneObject};

pub struct Placement {
    pub model: crate::model::Ref,
    pub at: Affine3A,
    pub tint: Vector4,
    pub fade_end: f32,
    pub lod: Option<i32>,
}

pub fn local(m: &Matrix) -> Affine3A {
    let col = |j: usize| Vec3::new(m[0][j], m[1][j], m[2][j]);
    Affine3A::from_mat3_translation(Mat3::from_cols(col(0), col(1), col(2)), col(3))
}

/// The aggregate model drawing only `f`'s draw call of its first mesh, in `f`'s LOD groups with
/// its LOD setup's switch distances (every LOD without a mask).
fn fragment_model(so: &SceneObject, f: &Fragment) -> Option<crate::model::Ref> {
    let mut m = crate::model::compiled::read(&so.model)?;
    let mut mesh = m.meshes.first()?.clone();
    mesh.mesh.draws = vec![mesh.mesh.draws.get(f.draw_call)?.clone()];
    mesh.lod_mask = if f.lod_mask == 0 { u64::MAX } else { u64::from(f.lod_mask) };
    m.meshes = vec![mesh];
    if let Some(setup) = usize::try_from(f.lod_setup).ok().and_then(|i| so.lod_setups.get(i)) {
        m.lod_distances = setup.switch_distances.clone();
    }
    let name = format!("{}#draw{}", so.model, f.draw_call);
    Some(crate::model::create_runtime(&name, crate::model::compiled::content(&m)))
}

fn load(model: &str) -> crate::model::Ref {
    let name = std::ffi::CString::new(model).unwrap_or_default();
    crate::model::load(Some(&name), abi::Guid::default())
}

pub fn of(so: &SceneObject) -> Vec<Placement> {
    let [r, g, b, a] = so.tint;
    let whole = || Placement {
        model: load(&so.model),
        at: local(&so.transform),
        tint: crate::math::vec4(r, g, b, a),
        fade_end: so.fade[1],
        lod: (so.lod_override >= 0).then_some(so.lod_override),
    };
    let fragments: Option<Vec<Placement>> = so
        .fragments
        .iter()
        .map(|f| {
            let [r, g, b] = f.tint;
            let at = f.transform.as_ref().map_or(Affine3A::IDENTITY, local);
            Some(Placement { model: fragment_model(so, f)?, at, tint: crate::math::vec4(r, g, b, 1.0), fade_end: 0.0, lod: None })
        })
        .collect();
    match fragments {
        Some(f) if !f.is_empty() => f,
        // No fragments, or a model we can't split (no compiled file, bad draw index): draw it whole.
        _ => vec![whole()],
    }
}
