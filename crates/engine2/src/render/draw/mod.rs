//! Turning meshes into GPU draws: [`gpu_mesh`] uploads `MeshGlue` buffers (re-uploading when
//! their version moves), [`model`] expands a model into per-draw-call [`Draw`]s, [`immediate`]
//! holds the RenderTools / IRenderContext draw slots. Draws with a material shade through
//! [`shaded`]; the rest, and any that fail to bind, use the placeholder.

pub mod caster;
pub mod draw_call;
pub mod dynamic;
pub mod gpu_mesh;
pub mod immediate;
pub mod model;
pub mod gizmo;
pub mod morph;
pub mod screen_rect;
pub mod shaded;

use crate::render::lighting::Lighting;
use crate::render::shading::view::View;
use crate::render::{device, texture};
use abi::structs::{NativeRect, RenderViewport};
use render::pass::{self, DrawUniforms, Load, MeshDraw, Shaded, Targets};
use render::wgpu::{IndexFormat, PrimitiveTopology};
use render::{PipelineKey, Placeholder, RasterState, VertexBufferKey};
use std::ops::Range;
use std::sync::{Arc, OnceLock};

/// One draw with owned buffers, ready for [`execute`].
pub struct Draw {
    pub vertices: Arc<render::Buffer>,
    pub vertex_offset: u64,
    pub vertex: Vec<VertexBufferKey>,
    pub indices: Option<(Arc<render::Buffer>, IndexFormat)>,
    pub range: Range<u32>,
    pub base_vertex: i32,
    pub instances: u32,
    pub uniforms: DrawUniforms,
    pub state: RasterState,
    /// Drawn with its material's program when that binds; `vertex` and `uniforms` otherwise.
    pub shaded: Option<shaded::Shaded>,
    /// A shadow caster's instance stream of world matrices: drawn by the caster program through
    /// `uniforms.mvp` (caster.rs).
    pub transforms: Option<Arc<render::Buffer>>,
}

/// Where draws go: textures (null for none), viewport (zero size = whole target), scissor.
#[derive(Clone, Copy, Default)]
pub struct Target {
    pub color: texture::Ref,
    pub depth: texture::Ref,
    /// The depth target's array layer: a cube face for `BindDepthTarget`'s slice.
    pub depth_slice: u32,
    pub viewport: RenderViewport,
    pub scissor: Option<NativeRect>,
    /// Native's sRGB write (`RENDER_SRGB` output, `BindColorAndDepthTarget`'s flag): an 8-bit
    /// colour target stores what draws write sRGB-encoded.
    pub srgb_write: bool,
}

pub fn placeholder() -> Option<&'static Placeholder> {
    static PLACEHOLDER: OnceLock<Placeholder> = OnceLock::new();
    let gpu = device::gpu()?;
    Some(PLACEHOLDER.get_or_init(|| Placeholder::new(gpu)))
}

fn blitter() -> Option<&'static render::Blitter> {
    static BLITTER: OnceLock<render::Blitter> = OnceLock::new();
    let gpu = device::gpu()?;
    Some(BLITTER.get_or_init(|| render::Blitter::new(gpu)))
}

/// A viewport's `[x, y, w, h]` on `t`; an empty viewport covers the rest of `t` from its corner.
pub fn viewport_rect(t: &render::Texture, v: &abi::structs::RenderViewport) -> [u32; 4] {
    let (x, y) = (v.m_n_top_left_x.max(0) as u32, v.m_n_top_left_y.max(0) as u32);
    if v.m_n_width > 0 && v.m_n_height > 0 {
        [x, y, v.m_n_width as u32, v.m_n_height as u32]
    } else {
        [x, y, t.desc.width.saturating_sub(x), t.desc.height.saturating_sub(y)]
    }
}

/// Whether writing to `f` through an sRGB view would encode: an 8-bit format without its own
/// sRGB format (we never create views that write sRGB, so blits apply the encode themselves).
pub fn srgb_write_encodes(f: render::wgpu::TextureFormat) -> bool {
    !f.is_srgb() && f.target_component_alignment() == Some(1)
}

/// The net transfer of a native blit: with `srgb_views` (`RENDER_SRGB`: source and target formats)
/// an 8-bit source is read decoded and an 8-bit target written encoded, and the shader's
/// `D_SRGB_TRANSFER` (1 encode, 2 decode) applies in between. Steps that cancel drop out.
pub fn blit_transfer(srgb_views: Option<(render::wgpu::TextureFormat, render::wgpu::TextureFormat)>, combo: i32) -> render::Transfer {
    let (read, write) = srgb_views.map_or((0, 0), |(src, dst)| (-i32::from(srgb_write_encodes(src)), i32::from(srgb_write_encodes(dst))));
    let shader = match combo {
        1 => 1,
        2 => -1,
        _ => 0,
    };
    match read + shader + write {
        1 => render::Transfer::Encode,
        -1 => render::Transfer::Decode,
        // TODO: a double encode or decode (no pipeline asks for one).
        _ => render::Transfer::None,
    }
}

/// `src_rect` of `src` into `dst_rect` of `dst` (`[x, y, w, h]`): a copy when sizes and formats
/// match and no transfer applies, else a draw that scales and converts.
pub fn blit(src: &render::Texture, src_rect: [u32; 4], dst: &render::Texture, dst_rect: [u32; 4], transfer: render::Transfer) -> bool {
    let Some(gpu) = device::gpu() else { return false };
    let copy = src_rect[2..] == dst_rect[2..] && transfer == render::Transfer::None;
    (copy && src.copy_to(gpu, dst, src_rect, (dst_rect[0], dst_rect[1]))) || blitter().is_some_and(|b| b.blit(gpu, src, src_rect, dst, dst_rect, transfer))
}

/// `RenderPrimitiveType` to a wgpu topology; patches and adjacency draw as triangle lists.
pub fn topology(prim: i64) -> PrimitiveTopology {
    match prim {
        0 => PrimitiveTopology::PointList,
        1 | 2 => PrimitiveTopology::LineList,
        3 | 4 => PrimitiveTopology::LineStrip,
        7 | 8 => PrimitiveTopology::TriangleStrip,
        _ => PrimitiveTopology::TriangleList,
    }
}

/// Run `draws` (possibly none, to only clear) into `target` as one pass, shaded draws lit by
/// `lighting`.
pub fn execute(target: &Target, lighting: &Lighting, load: Load, draws: &[Draw]) {
    let (Some(gpu), Some(placeholder)) = (device::gpu(), placeholder()) else { return };
    let (color, depth) = (texture::resolve(target.color), texture::resolve(target.depth));
    let mut targets = Targets { color: color.as_ref().and_then(|t| t.gpu.as_ref()), depth: depth.as_ref().and_then(|t| t.gpu.as_ref()), depth_layer: target.depth_slice, srgb_write: target.srgb_write, ..Default::default() };
    if let (Some(c), Some(d)) = (targets.color, targets.depth)
        && (c.desc.width, c.desc.height) != (d.desc.width, d.desc.height)
    {
        targets.depth = None;
    }
    targets.depth = targets.depth.filter(|d| d.desc.format.is_depth_stencil_format());
    let v = target.viewport;
    if v.m_n_width > 0 && v.m_n_height > 0 {
        let far = if v.max_z > v.min_z { v.max_z } else { 1.0 };
        targets.viewport = Some([v.m_n_top_left_x as f32, v.m_n_top_left_y as f32, v.m_n_width as f32, v.m_n_height as f32, v.min_z.max(0.0), far]);
    }
    targets.scissor = target.scissor.filter(|s| s.w > 0 && s.h > 0).map(|s| [s.x.max(0) as u32, s.y.max(0) as u32, s.w as u32, s.h as u32]);
    let state = |d: &Draw| {
        let s = d.shaded.as_ref().map_or(d.state, |s| s.raster_state(d.state));
        RasterState { depth_write: s.depth_write && targets.depth.is_some(), ..s }
    };
    let view = View { tone_map: lighting.tone_map.unwrap_or(1.0), ..view_of(&targets) };
    let key = |d: &Draw, vertex: &[VertexBufferKey]| PipelineKey { color: targets.color_format(), depth: targets.depth.map(|t| t.desc.format), vertex: vertex.to_vec(), state: state(d) };
    let bound: Vec<_> = draws.iter().map(|d| d.shaded.as_ref().and_then(|s| shaded::bind(gpu, s, &view.with_view_proj(s.view_proj), lighting, &key(d, &s.vertex), &[target.color, target.depth]))).collect();
    let caster_groups: Vec<_> = draws.iter().map(|d| casting(d).map(|(c, _)| [c.bind_group(gpu, d.uniforms.mvp)])).collect();
    let (defaults, ids) = shaded::streams(gpu);
    let draws: Vec<_> = draws
        .iter()
        .zip(&bound)
        .zip(&caster_groups)
        .map(|((d, groups), caster_group)| {
            let as_caster = casting(d).zip(caster_group.as_ref()).map(|((c, t), g)| Shaded { program: &c.program, bind_groups: g, buffers: vec![(&t.buffer, 0)] });
            let material = d.shaded.as_ref().zip(groups.as_ref());
            MeshDraw {
                vertices: (&d.vertices.buffer, d.vertex_offset),
                vertex: material.map_or_else(|| d.vertex.clone(), |(s, _)| s.vertex.clone()),
                indices: d.indices.as_ref().map(|(b, f)| (&b.buffer, *f)),
                range: d.range.clone(),
                base_vertex: d.base_vertex,
                instances: d.instances,
                uniforms: d.uniforms,
                state: state(d),
                shaded: material.map(|(s, groups)| {
                    let ids = s.context.instance_ids.as_ref().map_or((&ids.buffer, 0), |(b, offset)| (&b.buffer, *offset));
                    Shaded { program: &s.shading.program, bind_groups: groups, buffers: vec![(&defaults.buffer, 0), ids] }
                }).or(as_caster),
            }
        })
        .collect();
    let load = Load { color: load.color.filter(|_| targets.color.is_some()), depth: load.depth.filter(|_| targets.depth.is_some()) };
    pass::execute(gpu, placeholder, &targets, load, &draws);
}

/// The caster program and instance stream of a shadow caster draw (caster.rs).
fn casting(d: &Draw) -> Option<(&'static render::Caster, &Arc<render::Buffer>)> {
    Some((caster::program()?, d.transforms.as_ref().filter(|_| d.shaded.is_none())?))
}

/// The per-view constants' viewport and target size for a pass (its view-projection is per draw).
fn view_of(targets: &Targets) -> View {
    let (w, h) = targets.size().unwrap_or((1, 1));
    let ([x, y, vw, vh], depth_range) = targets.viewport.map_or(([0.0, 0.0, w as f32, h as f32], [0.0, 1.0]), |[x, y, vw, vh, min, max]| ([x, y, vw, vh], [min, max]));
    View { viewport: [x, y, vw, vh], target: [w as f32, h as f32], depth_range, ..Default::default() }
}
