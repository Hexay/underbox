//! The rest of `RenderTools`, `IRenderContext` and `g_pRenderDevice`: copies between targets, the
//! sky, per-object draw lists managed's scene renderer reads (`Sandbox.SceneRenderer`), sampler
//! and bindless ids, and the GPU features the renderer doesn't have yet (indirect compute, ray
//! tracing, lightmaps), which answer "none" so managed skips them. Skinning: render/skin.

use crate::render::draw::{self, immediate, model as model_draw};
use crate::render::{context, device, texture};
use crate::scene::object;
use abi::slots::{g_p_render_device as dev, i_render_context as ctx, render_tools as tools};
use abi::structs::{NativeRect, RenderViewport, Vector3, Vector4};
use abi::{NativeRef, PtrHandle, RawPtr, Registry, bind, class};
use std::ffi::c_void;

/// Copy `rect` of the context's colour (or depth) target into `dst` at its origin.
fn copy_from_target(c: context::Ref, dst: texture::Ref, rect: Option<[u32; 4]>, depth: bool) -> bool {
    let t = context::target(c);
    let src = if depth { t.depth } else { t.color };
    let (Some(gpu), Some(src), Some(dst)) = (device::gpu(), texture::resolve(src), texture::resolve(dst)) else { return false };
    let (Some(a), Some(b)) = (src.gpu.as_ref(), dst.gpu.as_ref()) else { return false };
    let rect = rect.filter(|r| r[2] > 0 && r[3] > 0).unwrap_or([0, 0, a.desc.width.min(b.desc.width), a.desc.height.min(b.desc.height)]);
    a.copy_to(gpu, b, rect, (0, 0))
}

fn rect(r: NativeRect) -> Option<[u32; 4]> {
    let c = |v: i32| v.max(0) as u32;
    Some([c(r.x), c(r.y), c(r.w), c(r.h)])
}

fn copy_texture(src: texture::Ref, dst: texture::Ref, r: NativeRect, dx: i32, dy: i32) {
    let (Some(gpu), Some(s), Some(d)) = (device::gpu(), texture::resolve(src), texture::resolve(dst)) else { return };
    let (Some(a), Some(b)) = (s.gpu.as_ref(), d.gpu.as_ref()) else { return };
    let r = rect(r).filter(|r| r[2] > 0 && r[3] > 0).unwrap_or([0, 0, a.desc.width, a.desc.height]);
    // TODO: mip and array slices (always 0 here).
    a.copy_to(gpu, b, r, (dx.max(0) as u32, dy.max(0) as u32));
}

/// `BlitTexture`: all of `src` into `viewport` of the bound colour target, scaled and converted
/// (managed's HDR-to-swap-chain copy).
/// `nSrgbTransfer` 1 encodes and 2 decodes; on top, a target bound with sRGB write that stores
/// 8-bit values (no sRGB view of its own) gets the encode its view would apply.
fn blit_texture(c: context::Ref, src: texture::Ref, viewport: RenderViewport, srgb_transfer: i32) -> bool {
    use render::Transfer;
    let (Some(s), Some(d)) = (texture::resolve(src), texture::resolve(context::target(c).color)) else { return false };
    let (Some(a), Some(b)) = (s.gpu.as_ref(), d.gpu.as_ref()) else { return false };
    let f = b.desc.format;
    let view_encodes = context::srgb_write(c) && draw::srgb_write_encodes(f);
    let transfer = match (srgb_transfer, view_encodes) {
        (2, true) => Transfer::None,
        (2, false) => Transfer::Decode,
        (1, _) | (_, true) => Transfer::Encode,
        _ => Transfer::None,
    };
    draw::blit(a, [0, 0, a.desc.width, a.desc.height], b, draw::viewport_rect(b, &viewport), transfer)
}

/// `GetSceneObjectDraw`: the `index`th draw call over the object's model meshes (LOD 0, enabled
/// groups), as (mesh, draw call, flags, material). A mesh of -1 ends managed's loop.
fn scene_object_draw(o: object::Ref, index: i32, mesh: RawPtr<c_void>, call: RawPtr<c_void>, flags: RawPtr<c_void>) -> NativeRef<class::IMaterial2> {
    const VISIBLE: i32 = 1;
    const ALPHA_BLENDED: i32 = 2;
    let Some(obj) = object::get(o) else { return NativeRef::NULL };
    let (m, groups, material_override) = {
        let s = obj.state.lock().unwrap();
        (s.model, s.mesh_groups, s.material_override.clone())
    };
    let Some(model) = crate::model::get(m) else { return NativeRef::NULL };
    let mut n = 0;
    for (mi, mm) in model.content.meshes.iter().enumerate() {
        if mm.lod_mask & 1 == 0 || (mm.group_mask != 0 && mm.group_mask & groups == 0) {
            continue;
        }
        for (ci, dc) in mm.mesh.draw_calls().iter().enumerate() {
            if n == index {
                // Native's draw states carry a whole-object override (managed's SceneMirror.ApplyMesh).
                let material = material_override.as_ref().or(dc.material.as_ref());
                let blends = material.and_then(|m| m.material()).is_some_and(|m| m.blend().is_some());
                // SAFETY: managed passes three ints on its stack (RenderContext.ReadSceneObjectDraws).
                unsafe {
                    *mesh.cast::<i32>().as_ptr() = mi as i32;
                    *call.cast::<i32>().as_ptr() = ci as i32;
                    *flags.cast::<i32>().as_ptr() = VISIBLE | if blends { ALPHA_BLENDED } else { 0 };
                }
                return crate::mesh::material_ref::handle(material);
            }
            n += 1;
        }
    }
    NativeRef::NULL
}

/// `DrawSkyBox`: the sky material's stand-in (or the placeholder), behind everything already drawn.
fn draw_sky_box(c: context::Ref, attrs: crate::render::attributes::Ref, mode: crate::render::material_mode::Ref, tint: Vector4) -> bool {
    let ctx_attrs = context::get(c).map(|x| x.attributes).unwrap_or_default();
    let context = draw::shaded::ContextBindings { attributes: vec![attrs, ctx_attrs], ..Default::default() };
    let Some(d) = crate::render::scene::execute::sky(model_draw::tint(tint), crate::render::material_mode::material(mode), immediate::view_proj(c), context) else { return false };
    immediate::submit(c, &[d]);
    true
}

/// Managed's material-bind block (zeroed by managed): full and similar-material bind counts.
const MATERIAL_BIND_SIZE: i32 = 16;

fn material_bind_counts(bind: RawPtr<c_void>, full: RawPtr<c_void>, similar: RawPtr<c_void>) {
    if bind.is_null() {
        return;
    }
    // SAFETY: bind is a MATERIAL_BIND_SIZE block, full and similar point at ints (RenderContext.MaterialBinds).
    unsafe {
        let counts = bind.cast::<[i32; 2]>().as_ptr().read();
        *full.cast::<i32>().as_ptr() = counts[0];
        *similar.cast::<i32>().as_ptr() = counts[1];
    }
}

fn light_probe_volume(mins: RawPtr<c_void>, maxs: RawPtr<c_void>, priority: RawPtr<c_void>) -> i32 {
    // SAFETY: managed passes two Vector3s and an int it hasn't initialised (ReadLightProbeVolume).
    unsafe {
        *mins.cast::<Vector3>().as_ptr() = Vector3::default();
        *maxs.cast::<Vector3>().as_ptr() = Vector3::default();
        *priority.cast::<i32>().as_ptr() = 0;
    }
    0
}

/// Every world blows native's default wind ([`crate::render::fog::WIND`]).
fn world_wind(strength: RawPtr<c_void>, direction: RawPtr<c_void>) {
    let (s, d) = crate::render::fog::WIND;
    // SAFETY: managed passes two Vector4s it reads back (RenderContext.ReadWorldWind).
    unsafe {
        *strength.cast::<[f32; 4]>().as_ptr() = s.to_array();
        *direction.cast::<[f32; 4]>().as_ptr() = d.to_array();
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, tools::RESOLVE_FRAME_BUFFER, |c, t, vp| {
        copy_from_target(c, t, rect(vp), false);
    });
    bind!(r, tools::RESOLVE_DEPTH_BUFFER, |c, t, vp| {
        copy_from_target(c, t, rect(vp), true);
    });
    bind!(r, tools::MAKE_FRAME_BUFFER_COPY, |c, t, x, y, w, h| {
        copy_from_target(c, t, Some([x.max(0) as u32, y.max(0) as u32, w.max(0) as u32, h.max(0) as u32]), false);
    });
    // TODO: the sRGB transfer flag; the blit draw writes linear values as they are.
    bind!(r, tools::BLIT_TEXTURE, |c, src, x, y, w, h, srgb| {
        crate::render::submits::log(c, &format!("blit from {}", crate::render::scene::trace::describe(src)));
        blit_texture(c, src, RenderViewport { m_n_top_left_x: x, m_n_top_left_y: y, m_n_width: w, m_n_height: h, ..Default::default() }, srgb)
    });
    bind!(r, tools::COPY_TEXTURE, |c, src, dst, r, dx, dy, _, _, _, _| {
        crate::render::submits::log_texture(c, &format!("copy from {}", crate::render::scene::trace::describe(src)), dst);
        copy_texture(src, dst, r, dx, dy)
    });
    bind!(r, tools::DRAW_SKY_BOX, |c, attrs, mode, _xf, tint, _fog, _, _, _, _| draw_sky_box(c, attrs, mode, tint));
    bind!(r, tools::DRAW_DYNAMIC_SCENE_OBJECT, |c, o, _mode| {
        let t = object::with(o, |s| Some(s.transform)).unwrap_or_else(crate::math::identity);
        immediate::draw_scene_object(c, o, t, crate::math::vec4(1.0, 1.0, 1.0, 1.0));
        true
    });
    bind!(r, tools::GET_SCENE_OBJECT_DRAW, scene_object_draw);
    bind!(r, tools::GET_MODEL_DRAW_CALL_TINT, |_, _, _, _| false);
    bind!(r, tools::GET_SCENE_OBJECT_LIGHTMAP_KEY, |_| 0);
    bind!(r, tools::SET_SCENE_OBJECT_LIGHTMAP_ATTRIBUTES, |_, _| false);
    bind!(r, tools::GET_SCENE_WORLD_WIND, |_, s, d| world_wind(s, d));
    bind!(r, tools::PACK_SCENE_LIGHT, |_, _, _| -1);
    bind!(r, tools::PACK_SCENE_ENV_MAP, |_, _| false);
    bind!(r, tools::GET_LIGHT_PROBE_VOLUME, |_, mins, maxs, priority, _, _| light_probe_volume(mins, maxs, priority));
    bind!(r, tools::SET_LIGHT_PROBE_VOLUME_ATTRIBUTES, |_, _| false);
    bind!(r, tools::GET_AGGREGATE_MODEL, |_, _| NativeRef::NULL);
    bind!(r, tools::GET_AGGREGATE_FRAGMENTS, |_, _, _| 0);
    bind!(r, tools::GET_MATERIAL_BIND_SIZE, || MATERIAL_BIND_SIZE);
    bind!(r, tools::RESET_MATERIAL_BIND, |_| {});
    bind!(r, tools::GET_MATERIAL_BIND_COUNTS, material_bind_counts);
    bind!(r, tools::GET_MATERIAL_SIMILARITY_KEY, |m| m.addr() as u64);
    bind!(r, tools::MODEL_MESH_HAS_MORPHS, |m, mesh| {
        let model = crate::model::get(m);
        usize::try_from(mesh).ok().and_then(|i| model?.content.meshes.get(i).map(|e| !e.mesh.state().morphs.is_empty())).unwrap_or(false)
    });
    // No ray tracing.
    bind!(r, tools::TRACE_RAYS, |_, _, _, _, _, _| {});
    bind!(r, tools::TRACE_RAYS_INDIRECT, |_, _, _, _, _| {});
    bind!(r, tools::COPY_GPU_BUFFER_HIDDEN_STRUCTURE_COUNT, |_, _, _, _| {});
    bind!(r, tools::SET_GPU_BUFFER_HIDDEN_STRUCTURE_COUNT, |_, _, _| {});
    bind!(r, tools::TONEMAP_FRAME_UPDATE, crate::render::exposure::frame_update);
    bind!(r, tools::RENDER_AUTO_EXPOSURE, |_, t, color, x, y, w, h| crate::render::exposure::record(t, color, NativeRect { x, y, w, h }));
    // Contexts execute eagerly on one queue: barriers and semaphores have nothing to order.
    bind!(r, ctx::SEMAPHORE_SIGNAL_AT_END, |_| PtrHandle::default());
    bind!(r, ctx::SEMAPHORE_WAIT_AT_BEGIN, |_, _| {});
    bind!(r, ctx::TEXTURE_BARRIER_TRANSITION, |_, _, _, _, _, _, _, _| {});
    bind!(r, ctx::BUFFER_BARRIER_TRANSITION, |_, _, _, _, _, _| {});
    // TODO: mip generation and uint clears need compute or a blit pass.
    bind!(r, ctx::GENERATE_MIP_MAPS, |_, _| {});
    bind!(r, ctx::CLEAR_TEXTURE_UINT, |_, _, _| {});
    bind!(r, dev::FIND_OR_CREATE_SAMPLER_STATE, |d| PtrHandle::<abi::handle::SamplerStateHandle_t>::new(crate::render::samplers::find_or_create(d)));
    bind!(r, dev::GET_SAMPLER_INDEX, |h| h.value as i32 - 1);
    bind!(r, dev::GET_TEXTURE_VIEW_INDEX, |t, color_space, _| crate::render::bindless::index(t, color_space == crate::render::bindless::SRGB));
    bind!(r, dev::GET_SHEET_META_DATA_TEXTURE, || NativeRef::NULL);
    bind!(r, dev::GET_SHEET_INFO, |_| Vector4::default());
    bind!(r, dev::GET_TEXTURE_RESIDENCY_INFO, |_, _, _| {});
    // Native API objects (VkImage, ID3D12Resource) aren't exposed through wgpu.
    bind!(r, dev::GET_DEVICE_SPECIFIC_INFO, |_| RawPtr::NULL);
    bind!(r, dev::GET_GRAPHICS_API_SPECIFIC_TEXTURE_HANDLE, |_| RawPtr::NULL);
    bind!(r, dev::GET_DEVICE_SPECIFIC_TEXTURE, |_| RawPtr::NULL);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::material_ref::MaterialRef;
    use crate::render::material;
    use crate::scene::{object::State, world};

    /// What managed's SceneMirror reads: (mesh, call, flags, material's shader).
    fn draw(o: object::Ref) -> (i32, i32, i32, String) {
        let (mut mesh, mut call, mut flags) = (-1i32, 0i32, 0i32);
        let ptr = |v: &mut i32| RawPtr::from_raw((v as *mut i32).cast::<c_void>());
        let m = scene_object_draw(o, 0, ptr(&mut mesh), ptr(&mut call), ptr(&mut flags));
        let name = material::resolve(m).map(|m| m.shader().to_owned()).unwrap_or_default();
        crate::resource::destroy_handle(m);
        (mesh, call, flags, name)
    }

    #[test]
    fn draws_carry_a_translucent_override_and_its_blend() {
        crate::render::scene::tests::fake_managed();
        let w: world::Ref = objects::create_shared(std::sync::Arc::new(world::SceneWorld::default()));
        let mat = material::create_raw(Some(c"materials/test/glass.vmat"), Some(c"shaders/complex.shader"), true);
        material::set_vector(mat, "F_TRANSLUCENT", crate::math::vec4(1.0, 0.0, 0.0, 0.0));
        let model = crate::model::create_runtime("models/test/glass.vmdl", Default::default());
        let state = State { model, material_override: MaterialRef::keep(mat), ..State::default() };
        let (o, _) = object::create_native::<class::CSceneObject, class::CSceneObject>(w, crate::handles::Kind::SceneObject, state, None);
        assert_eq!(draw(o), (0, 0, 3, "shaders/complex.shader".to_owned()), "visible and alpha blended, drawn with the override");
        object::with(o, |s| s.material_override = None);
        assert_eq!(draw(o).2, 1, "the model's own draw is opaque");
    }
}
