//! Running a view: its layers in order, each into its own (or the view's) targets. Render
//! layers draw the matching objects of the view's worlds (models through the placeholder, managed
//! objects through `SceneCustomObject.RenderObject`); procedural layers call managed back.

use super::layer::{self, Kind, flags, layer_type};
use super::view::{self, SceneView};
use crate::handles::Kind as ObjectKind;
use crate::mesh::layout::{Field, Layout};
use crate::render::draw::shaded::{self, ContextBindings};
use crate::render::draw::{self, Draw, Target, immediate, model as model_draw, morph};
use crate::render::material::Material;
use crate::render::shading::bind::Instance;
use crate::render::lighting::{Lighting, local_shadow, shadow};
use crate::render::{context, device, texture};
use crate::scene::object;
use abi::callbacks::sandbox_scene_custom_object_render as custom;
use abi::structs::{ISceneLayer, ISceneView, IRenderContext, ManagedRenderSetup_t, SceneSystemPerFrameStats_t};
use abi::StringToken;
use glam::{Mat4, Vec3};
use render::pass::{DrawUniforms, Load};
use render::{BufferKind, Placeholder, RasterState, Semantic};
use std::sync::{Arc, OnceLock};

use crate::scene::object::IS_OPAQUE;
/// Placeholder sky colour (linear; sRGB (0.45, 0.6, 0.85)), times the sky object's tint. TODO: the sky material.
const SKY: [f32; 4] = [0.170_645, 0.318_547, 0.692_071, 1.0];

/// Clear, sky and one opaque layer: what a view gets when managed adds no layers (no managed
/// pipeline, as in tests, or a pipeline that declined).
/// Like native's pipeline, they draw linear into an HDR target and resolve it to the view's
/// colour with the sRGB encode.
pub fn default_layers(v: view::Ref, clear_flags: i32, clear_color: abi::structs::Vector4) {
    let Some(view) = view::get(v) else { return };
    let vp = view.viewport;
    let hdr = hdr_target(view.color);
    let add = |name: &str, kind: Kind, required: i64| {
        let l = view::add_layer(v, layer::create(v, name.into(), vp, kind, required, 0), Default::default());
        if !hdr.is_null() {
            layer::set_output(l, hdr, view.depth);
        }
        l
    };
    let clear = add("Clear", Kind::Clear(clear_flags), 0);
    layer::with(clear, |s| s.clear_color = clear_color);
    let sky = add("Sky", Kind::Render(StringToken::of("Forward")), 0);
    layer::with(sky, |s| s.match_id = StringToken::of("SkyBox").0);
    add("Opaque", Kind::Render(StringToken::of("Forward")), IS_OPAQUE);
    if !hdr.is_null() {
        let resolve = Kind::Blit { src: hdr, src_viewport: vp, srgb: true };
        let l = view::add_layer(v, layer::create(v, "ResolveHDRToFinalSDR".into(), vp, resolve, 0, 0), Default::default());
        layer::set_output(l, view.color, texture::Ref::default());
    }
}

/// An `RGBA16161616F` target the size of `color`, one per colour target (views on other threads
/// render concurrently) and dropped with it; null without a GPU colour target.
fn hdr_target(color: texture::Ref) -> texture::Ref {
    type Entry = (texture::Ref, (u32, u32), texture::Ref);
    static TARGETS: std::sync::Mutex<Vec<Entry>> = std::sync::Mutex::new(Vec::new());
    let size_of = |t: texture::Ref| texture::resolve(t).and_then(|t| t.gpu.as_ref().map(|g| (g.desc.width, g.desc.height)));
    let Some(size) = size_of(color) else { return texture::Ref::default() };
    let mut targets = TARGETS.lock().unwrap();
    targets.retain(|(c, s, hdr)| {
        let keep = size_of(*c) == Some(*s);
        if !keep {
            crate::resource::destroy_handle(*hdr);
        }
        keep
    });
    if let Some((_, _, t)) = targets.iter().find(|(c, _, _)| *c == color) {
        return *t;
    }
    const RGBA16161616F: i32 = 24;
    let t = texture::create_target("underbox_hdr_color", size.0, size.1, RGBA16161616F);
    targets.push((color, size, t));
    t
}

pub fn run(v: view::Ref) {
    let Some(view) = view::get(v) else { return };
    let ctx = context::create(0);
    let (layers, worlds) = {
        let s = view.state.lock().unwrap();
        (s.layers.clone(), s.worlds.clone())
    };
    let mut lighting = Lighting::collect(&worlds, view.ambient);
    shadow::render(&mut lighting, &worlds, view.view_proj);
    local_shadow::render(&mut lighting, &worlds);
    immediate::set_view(ctx, view.view_proj, Arc::new(lighting));
    for l in layers {
        run_layer(&view, v, ctx, l);
    }
    context::release(ctx);
}

fn target(view: &SceneView, l: layer::Ref, s: &layer::LayerState) -> Target {
    let (color, depth) = if s.output_set { (s.color, s.depth) } else { (view.color, view.depth) };
    let vp = layer::viewport(l);
    let viewport = if vp.m_n_width > 0 && vp.m_n_height > 0 { vp } else { view.viewport };
    Target { color, depth, viewport, ..Default::default() }
}

fn setup(ctx: context::Ref, v: view::Ref, l: layer::Ref, color: texture::Ref) -> ManagedRenderSetup_t {
    ManagedRenderSetup_t {
        render_context: IRenderContext { self_: ctx.addr() },
        scene_view: ISceneView { self_: v.addr() },
        scene_layer: ISceneLayer { self_: l.addr() },
        color_image_format: texture::resolve(color).map_or(0, |t| t.image_format()),
        msaa_level: 0,
        stats: SceneSystemPerFrameStats_t { self_: 0 },
    }
}

fn run_layer(view: &SceneView, v: view::Ref, ctx: context::Ref, l: layer::Ref) {
    let Some(layer) = layer::get(l) else { return };
    let (t, clear_color, required, excluded, match_id) = {
        let s = layer.state.lock().unwrap();
        (target(view, l, &s), s.clear_color.vec, s.required, s.excluded, s.match_id)
    };
    context::bind_target(ctx, &t);
    if !matches!(layer.kind, Kind::Render(_)) {
        super::trace::layer(v.addr(), &layer, &t, &[]);
    }
    match layer.kind {
        Kind::Clear(_) => {
            let f = layer::clear_flags(l);
            let color = render::wgpu::Color { r: clear_color.x.into(), g: clear_color.y.into(), b: clear_color.z.into(), a: clear_color.w.into() };
            draw::execute(&t, &immediate::lighting(ctx), Load { color: (f & 0xFF != 0).then_some(color), depth: (f & 0x100 != 0).then_some(1.0) }, &[]);
        }
        Kind::Render(_) => {
            let lt = layer_type(l);
            if layer::layer_flags(l) & flags::LIGHT_BINNER_SETUP != 0 || lt == layer::layer_type::DEPTH_PREPASS {
                return;
            }
            let filter = Filter { required, excluded, match_id, ignore_match: layer::layer_flags(l) & flags::IGNORE_LAYER_MATCH_ID != 0, translucent: lt == layer::layer_type::TRANSLUCENT };
            let (draws, managed) = collect(view, &filter);
            super::trace::layer(v.addr(), &layer, &t, &draws);
            draw::execute(&t, &immediate::lighting(ctx), Load::default(), &draws);
            if custom_objects_callable() {
                for id in managed {
                    custom::render_object(setup(ctx, v, l, t.color), id);
                }
            }
        }
        Kind::Procedural(callback) if callback != 0 => {
            // SAFETY: managed registered a `void(ManagedRenderSetup_t)` function pointer (HookLayers.cs).
            let f: extern "C" fn(ManagedRenderSetup_t) = unsafe { std::mem::transmute::<usize, extern "C" fn(ManagedRenderSetup_t)>(callback) };
            f(setup(ctx, v, l, t.color));
        }
        Kind::Blit { src, src_viewport: s, srgb } => {
            let (Some(src), Some(dst)) = (texture::resolve(src), texture::resolve(t.color)) else { return };
            let (Some(a), Some(b)) = (src.gpu.as_ref(), dst.gpu.as_ref()) else { return };
            let rect = if s.m_n_width > 0 { [s.m_n_top_left_x as u32, s.m_n_top_left_y as u32, s.m_n_width as u32, s.m_n_height as u32] } else { [0, 0, a.desc.width, a.desc.height] };
            let combo = crate::render::attributes::shader_value(layer.attributes, "D_SRGB_TRANSFER").map_or(0, |v| v.x as i32);
            let transfer = draw::blit_transfer(srgb.then_some((a.desc.format, b.desc.format)), combo);
            draw::blit(a, rect, b, draw::viewport_rect(b, &t.viewport), transfer);
        }
        Kind::Procedural(_) | Kind::Label => {}
    }
}

fn custom_objects_callable() -> bool {
    abi::rt::callbacks::is_set(47)
}

struct Filter {
    required: i64,
    excluded: i64,
    match_id: u32,
    ignore_match: bool,
    translucent: bool,
}

/// The draws for the view's objects this layer matches, and the managed objects to call back.
fn collect(view: &SceneView, f: &Filter) -> (Vec<Draw>, Vec<abi::HandleId<abi::class::CManagedSceneObject>>) {
    let (mut draws, mut managed) = (Vec::new(), Vec::new());
    let sky_layer = f.match_id == StringToken::of("SkyBox").0;
    let worlds = view.state.lock().unwrap().worlds.clone();
    for (r, obj) in object::refs_in_worlds(&worlds) {
        let morphs = morph::of(r);
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
        let matched = if f.match_id != 0 { s.layer_match == f.match_id } else { s.layer_match == 0 || f.ignore_match };
        let flags = s.effective_flags();
        if !matched || flags & f.required != f.required || flags & f.excluded != 0 {
            continue;
        }
        match obj.kind {
            ObjectKind::ManagedSceneObject => managed.push(obj.handle.id()),
            _ if f.translucent => {}
            _ => draws.extend(model_draw::object(&s, morphs.as_ref(), view.view_proj)),
        }
    }
    (draws, managed)
}

fn tags_pass(view: &SceneView, tags: &[u32]) -> bool {
    let wanted = view.render_tags.is_empty() || tags.iter().any(|t| view.render_tags.contains(t));
    wanted && !tags.iter().any(|t| view.exclude_tags.contains(t))
}

/// A full-screen triangle at the far plane, behind everything drawn with depth: the sky
/// material's cube (the sky stand-in) when it has a live `SkyTexture`, else the flat placeholder.
pub(crate) fn sky(tint: [f32; 4], material: Option<Arc<Material>>, view_proj: Mat4, context: ContextBindings) -> Option<Draw> {
    static TRIANGLE: OnceLock<Arc<render::Buffer>> = OnceLock::new();
    let gpu = device::gpu()?;
    let p: [f32; 9] = [-1.0, -1.0, 1.0, 3.0, -1.0, 1.0, -1.0, 3.0, 1.0];
    let bytes: Vec<u8> = p.iter().flat_map(|f| f.to_le_bytes()).collect();
    let vertices = TRIANGLE.get_or_init(|| Arc::new(render::Buffer::with_data(gpu, "sky", BufferKind::Vertex, &bytes))).clone();
    let shaded = material.filter(|m| has_sky_texture(m)).and_then(|m| {
        let layout = Layout { name: "sky".into(), stride: 12, fields: vec![Field { semantic: "position".into(), index: 0, format: crate::mesh::layout::format::RGB32F, offset: 0 }] };
        let mut s = shaded::resolve(m, &layout, Instance { world: Mat4::IDENTITY, tint }, view_proj, context)?;
        s.values.insert("g_vSkyTint".into(), glam::Vec4::from_array(tint));
        Some(s)
    });
    let flat = std::array::from_fn(|i| SKY[i] * tint[i]);
    Some(Draw {
        vertices,
        vertex_offset: 0,
        vertex: Placeholder::vertex_keys(12, &[(Semantic::Position, render::wgpu::VertexFormat::Float32x3, 0)]),
        indices: None,
        range: 0..3,
        base_vertex: 0,
        instances: 1,
        uniforms: DrawUniforms { mvp: Mat4::IDENTITY, model: Mat4::from_translation(Vec3::ZERO), tint: flat },
        state: RasterState { depth_write: false, ..Default::default() },
        shaded,
    })
}

fn has_sky_texture(m: &Material) -> bool {
    use crate::resource::Resource as _;
    m.shading_params().0.get("SkyTexture").and_then(|t| texture::resolve(*t)).is_some_and(|t| !t.is_error() && t.gpu.is_some())
}
