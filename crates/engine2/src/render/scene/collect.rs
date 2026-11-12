//! Which of a view's objects a render layer draws, and how: opaque layers take the draws whose
//! material doesn't blend, translucent layers the ones that do (back to front, blended, no depth
//! writes), and the depth prepass the opaque ones with colour writes off.

use super::execute::sky;
use super::layer::{self, flags, layer_type};
use super::view::SceneView;
use crate::handles::Kind as ObjectKind;
use crate::render::draw::shaded::ContextBindings;
use crate::render::depth;
use crate::render::draw::{Draw, caster, model as model_draw};
use crate::scene::object::{self, IS_TRANSLUCENT};
use abi::StringToken;
use glam::{Mat4, Vec3};

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
    /// The layer's `size_cull` (layer.rs).
    pub size_cull: f32,
    /// The layer has a colour target: a depth-normal prepass, drawn by the materials' programs.
    pub color: bool,
}

impl Filter {
    pub fn of(l: layer::Ref, required: i64, excluded: i64, match_id: u32) -> Filter {
        let lt = layer::layer_type(l);
        let pass = match lt {
            layer_type::DEPTH_PREPASS => Pass::DepthPrepass,
            _ if lt == layer_type::TRANSLUCENT || required & IS_TRANSLUCENT != 0 => Pass::Translucent,
            _ => Pass::Opaque,
        };
        let (size_cull, color) = (layer::with(l, |s| s.size_cull), !layer::targets(l).0.is_null());
        Filter { required, excluded, match_id, ignore_match: layer::layer_flags(l) & flags::IGNORE_LAYER_MATCH_ID != 0, pass, size_cull, color }
    }

    fn sized(&self, percent: f32) -> bool {
        match self.size_cull {
            t if t > 0.0 => percent >= t,
            t if t < 0.0 => percent < -t,
            _ => true,
        }
    }
}

/// How much of the screen's height `b`'s bounding sphere covers through `view_proj`, in percent;
/// past 100 when the eye is inside it. Our measure for native's size threshold: it splits real's
/// showroom prepass as real does (the floor large, the boxes small).
fn screen_percent(view_proj: Mat4, b: abi::structs::BBox) -> f32 {
    let (lo, hi) = (crate::math::v3(b.mins), crate::math::v3(b.maxs));
    let (center, radius) = ((lo + hi) * 0.5, (hi - lo).length() * 0.5);
    let w = (view_proj * center.extend(1.0)).w;
    let scale = view_proj.row(1).truncate().length();
    if w <= radius { f32::INFINITY } else { 100.0 * radius * scale / w }
}

/// A layer's draws, and the managed objects to call back with their world transforms.
pub type Collected = (Vec<Draw>, Vec<(abi::HandleId<abi::class::CManagedSceneObject>, Mat4)>);

/// What this layer draws of the view's objects it matches.
pub fn collect(view: &SceneView, f: &Filter) -> Collected {
    let (mut draws, mut managed) = (Vec::new(), Vec::new());
    let mut opaque = Vec::new();
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
        if f.size_cull != 0.0 && !f.sized(screen_percent(view.view_proj, s.world_bounds())) {
            continue;
        }
        super::trace::object(&s);
        let bounds = (flags & super::occlusion::NO_OCCLUSION_CULLING == 0).then(|| s.world_bounds()).map(|b| [crate::math::v3(b.mins), crate::math::v3(b.maxs)]);
        let mine = model_draw::object(&s, &deform, view.view_proj).into_iter().filter(|d| model_draw::translucent(d) == (f.pass == Pass::Translucent));
        let mine = mine.map(|mut d| {
            d.cull.bounds.extend(bounds);
            d
        });
        match f.pass {
            Pass::Opaque => opaque.extend(mine.map(|d| with_stencil(d, depth::stencil::opaque()))),
            Pass::DepthPrepass if f.color => opaque.extend(mine.map(|d| with_stencil(no_color(d), depth::stencil::prepass()))),
            Pass::DepthPrepass => opaque.extend(mine.map(model_draw::depth_only)),
            Pass::Translucent => {
                let p = s.transform.position.vec;
                sorted.push((eye.distance_squared(Vec3::new(p.x, p.y, p.z)), mine.map(model_draw::as_translucent).collect()));
            }
        }
    }
    if f.pass == Pass::DepthPrepass && !f.color {
        opaque = prepass(opaque, view.view_proj);
    }
    crate::render::draw::sort_opaque(&mut opaque);
    draws.extend(opaque);
    sorted.sort_by(|a, b| b.0.total_cmp(&a.0));
    draws.extend(sorted.into_iter().flat_map(|(_, d)| d));
    (draws, managed)
}

/// A depth prepass as real draws it: the depth-only program, each mesh one instanced draw, back
/// faces culled, no colour, and the prepass stencil test.
fn prepass(draws: Vec<Draw>, view_proj: Mat4) -> Vec<Draw> {
    let culled = draws.into_iter().map(|d| Draw { state: render::RasterState { cull: Some(render::wgpu::Face::Back), ..d.state }, ..d });
    caster::instanced(culled.collect(), view_proj).into_iter().map(|d| with_stencil(no_color(d), depth::stencil::prepass())).collect()
}

fn no_color(d: Draw) -> Draw {
    Draw { state: render::RasterState { write_mask: render::wgpu::ColorWrites::empty(), ..d.state }, ..d }
}

fn with_stencil(d: Draw, stencil: render::Stencil) -> Draw {
    Draw { state: render::RasterState { stencil: Some(stencil), ..d.state }, ..d }
}

fn tags_pass(view: &SceneView, tags: &[u32]) -> bool {
    let wanted = view.render_tags.is_empty() || tags.iter().any(|t| view.render_tags.contains(t));
    wanted && !tags.iter().any(|t| view.exclude_tags.contains(t))
}
