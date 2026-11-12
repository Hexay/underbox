//! Which of a view's objects a render layer draws, and how: opaque layers take the draws whose
//! material doesn't blend, translucent layers the ones that do (back to front, blended, no depth
//! writes), and the depth prepass the opaque ones with colour writes off.

use super::execute::sky;
use super::layer::{self, flags, layer_type};
use super::view::SceneView;
use crate::handles::Kind as ObjectKind;
use crate::render::draw::shaded::ContextBindings;
use crate::render::draw::{Draw, model as model_draw};
use crate::scene::object::{self, IS_TRANSLUCENT};
use abi::StringToken;
use glam::Vec3;

/// What a layer draws of the objects that pass its masks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pass {
    Opaque,
    Translucent,
    /// Opaque draws, depth only: later layers' `GreaterEqual` tests see the same depth.
    DepthPrepass,
}

pub struct Filter {
    pub required: i64,
    pub excluded: i64,
    pub match_id: u32,
    pub ignore_match: bool,
    pub pass: Pass,
}

impl Filter {
    pub fn of(l: layer::Ref, required: i64, excluded: i64, match_id: u32) -> Filter {
        let lt = layer::layer_type(l);
        let pass = match lt {
            layer_type::DEPTH_PREPASS => Pass::DepthPrepass,
            _ if lt == layer_type::TRANSLUCENT || required & IS_TRANSLUCENT != 0 => Pass::Translucent,
            _ => Pass::Opaque,
        };
        Filter { required, excluded, match_id, ignore_match: layer::layer_flags(l) & flags::IGNORE_LAYER_MATCH_ID != 0, pass }
    }
}

/// The draws for the view's objects this layer matches, and the managed objects to call back
/// with their world transforms.
pub fn collect(view: &SceneView, f: &Filter) -> (Vec<Draw>, Vec<(abi::HandleId<abi::class::CManagedSceneObject>, glam::Mat4)>) {
    let (mut draws, mut managed) = (Vec::new(), Vec::new());
    let mut sorted: Vec<(f32, Vec<Draw>)> = Vec::new();
    let sky_layer = f.match_id == StringToken::of("SkyBox").0;
    let eye = view.camera.origin;
    let worlds = view.state.lock().unwrap().worlds.clone();
    for (r, obj) in object::refs_in_worlds(&worlds) {
        let deform = model_draw::Deform::of(r);
        let s = obj.state.lock().unwrap();
        if !s.rendering || !tags_pass(view, &s.tags) {
            continue;
        }
        if obj.kind == ObjectKind::SceneSkyBoxObject {
            if sky_layer {
                let material = s.material_override.as_ref().and_then(|m| m.material());
                draws.extend(sky(model_draw::tint(s.tint), material, view.view_proj, ContextBindings::default()));
            }
            continue;
        }
        if !super::pvs::visible(eye, &obj, &s) {
            continue;
        }
        let matched = if f.match_id != 0 { s.layer_match == f.match_id } else { s.layer_match == 0 || f.ignore_match };
        let flags = s.effective_flags();
        if !matched || flags & f.required != f.required || flags & f.excluded != 0 {
            continue;
        }
        if obj.kind == ObjectKind::ManagedSceneObject {
            if f.pass != Pass::DepthPrepass {
                managed.push((obj.handle.id(), model_draw::matrix(&s.transform)));
            }
            continue;
        }
        super::trace::object(&s);
        let mine = model_draw::object(&s, &deform, view.view_proj).into_iter().filter(|d| model_draw::translucent(d) == (f.pass == Pass::Translucent));
        match f.pass {
            Pass::Opaque => draws.extend(mine),
            Pass::DepthPrepass => draws.extend(mine.map(depth_prepass)),
            Pass::Translucent => {
                let p = s.transform.position.vec;
                sorted.push((eye.distance_squared(Vec3::new(p.x, p.y, p.z)), mine.map(model_draw::as_translucent).collect()));
            }
        }
    }
    sorted.sort_by(|a, b| b.0.total_cmp(&a.0));
    draws.extend(sorted.into_iter().flat_map(|(_, d)| d));
    (draws, managed)
}

fn depth_prepass(d: Draw) -> Draw {
    Draw { state: render::RasterState { write_mask: render::wgpu::ColorWrites::empty(), ..d.state }, ..d }
}

fn tags_pass(view: &SceneView, tags: &[u32]) -> bool {
    let wanted = view.render_tags.is_empty() || tags.iter().any(|t| view.render_tags.contains(t));
    wanted && !tags.iter().any(|t| view.exclude_tags.contains(t))
}
