//! The rest of `RenderTools`, `IRenderContext` and `g_pRenderDevice`: copies between targets, the
//! sky, per-object draw lists managed's scene renderer reads (`Sandbox.SceneRenderer`), sampler
//! and bindless ids, and the GPU features the placeholder renderer doesn't have (compute, ray
//! tracing, skinning, morphs, lightmaps), which answer "none" so managed skips them.

use crate::render::draw::{immediate, model as model_draw};
use crate::render::{context, device, texture};
use crate::scene::object;
use abi::slots::{g_p_render_device as dev, i_render_context as ctx, render_tools as tools};
use abi::structs::{NativeRect, Vector3, Vector4};
use abi::{NativeRef, PtrHandle, RawPtr, Registry, bind, class};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{LazyLock, Mutex};

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

/// `GetSceneObjectDraw`: the `index`th draw call over the object's model meshes (LOD 0, enabled
/// groups), as (mesh, draw call, flags, material). A mesh of -1 ends managed's loop.
fn scene_object_draw(o: object::Ref, index: i32, mesh: RawPtr<c_void>, call: RawPtr<c_void>, flags: RawPtr<c_void>) -> NativeRef<class::IMaterial2> {
    const VISIBLE: i32 = 1;
    let Some(obj) = object::get(o) else { return NativeRef::NULL };
    let (m, groups) = {
        let s = obj.state.lock().unwrap();
        (s.model, s.mesh_groups)
    };
    let Some(model) = crate::model::get(m) else { return NativeRef::NULL };
    let mut n = 0;
    for (mi, mm) in model.content.meshes.iter().enumerate() {
        if mm.lod_mask & 1 == 0 || (mm.group_mask != 0 && mm.group_mask & groups == 0) {
            continue;
        }
        for (ci, dc) in mm.mesh.draw_calls().iter().enumerate() {
            if n == index {
                // SAFETY: managed passes three ints on its stack (RenderContext.ReadSceneObjectDraws).
                unsafe {
                    *mesh.cast::<i32>().as_ptr() = mi as i32;
                    *call.cast::<i32>().as_ptr() = ci as i32;
                    *flags.cast::<i32>().as_ptr() = VISIBLE;
                }
                return crate::mesh::material_ref::handle(dc.material.as_ref());
            }
            n += 1;
        }
    }
    NativeRef::NULL
}

/// `DrawSkyBox`: the placeholder sky, behind everything already drawn.
fn draw_sky_box(c: context::Ref, tint: Vector4) -> bool {
    let Some(d) = crate::render::scene::execute::sky(model_draw::tint(tint)) else { return false };
    immediate::submit(c, &[d]);
    true
}

/// Allocations managed asked for with `CreateConstantBuffer`, by address, with their layouts.
static CONSTANT_BUFFERS: LazyLock<Mutex<HashMap<usize, std::alloc::Layout>>> = LazyLock::new(Default::default);

fn create_constant_buffer(size: i32) -> RawPtr<c_void> {
    let Ok(layout) = std::alloc::Layout::from_size_align(size.max(1) as usize, 16) else { return RawPtr::NULL };
    // SAFETY: layout has a nonzero size.
    let p = unsafe { std::alloc::alloc_zeroed(layout) };
    if !p.is_null() {
        CONSTANT_BUFFERS.lock().unwrap().insert(p as usize, layout);
    }
    RawPtr::from_raw(p.cast())
}

fn destroy_constant_buffer(p: RawPtr<c_void>) {
    if let Some(layout) = CONSTANT_BUFFERS.lock().unwrap().remove(&(p.as_ptr() as usize)) {
        // SAFETY: allocated by create_constant_buffer with this layout.
        unsafe { std::alloc::dealloc(p.as_ptr().cast(), layout) };
    }
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

/// Distinct sampler descriptions get consecutive ids; the index is the id less one.
fn sampler(desc: abi::structs::CSamplerStateDesc) -> PtrHandle<abi::handle::SamplerStateHandle_t> {
    static SAMPLERS: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    // SAFETY: CSamplerStateDesc is a plain repr(C) struct of scalars.
    let bytes = unsafe { std::slice::from_raw_parts((&raw const desc).cast::<u8>(), size_of_val(&desc)) }.to_vec();
    let mut s = SAMPLERS.lock().unwrap();
    let i = s.iter().position(|d| *d == bytes).unwrap_or_else(|| {
        s.push(bytes);
        s.len() - 1
    });
    PtrHandle::new(i + 1)
}

/// Bindless texture index: stable per texture for its lifetime.
fn texture_view_index(t: texture::Ref) -> i32 {
    static INDICES: LazyLock<Mutex<HashMap<usize, i32>>> = LazyLock::new(Default::default);
    if t.is_null() {
        return -1;
    }
    let mut m = INDICES.lock().unwrap();
    let n = m.len() as i32;
    *m.entry(t.addr()).or_insert(n)
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
    // TODO: a scaled blit (and the sRGB transfer) needs a draw; same-size copies only for now.
    bind!(r, tools::BLIT_TEXTURE, |c, src, x, y, w, h, _srgb| {
        let t = context::target(c);
        copy_texture(src, t.color, NativeRect { x: 0, y: 0, w, h }, x, y);
        true
    });
    bind!(r, tools::COPY_TEXTURE, |_, src, dst, r, dx, dy, _, _, _, _| copy_texture(src, dst, r, dx, dy));
    bind!(r, tools::DRAW_SKY_BOX, |c, _attrs, _mode, _xf, tint, _fog, _, _, _, _| draw_sky_box(c, tint));
    bind!(r, tools::DRAW_DYNAMIC_SCENE_OBJECT, |c, o, _mode| {
        let t = object::with(o, |s| Some(s.transform)).unwrap_or_else(crate::math::identity);
        immediate::draw_scene_object(c, o, t, crate::math::vec4(1.0, 1.0, 1.0, 1.0));
        true
    });
    bind!(r, tools::GET_SCENE_OBJECT_DRAW, scene_object_draw);
    bind!(r, tools::GET_MODEL_DRAW_CALL_TINT, |_, _, _, _| false);
    bind!(r, tools::GET_SCENE_OBJECT_LIGHTMAP_KEY, |_| 0);
    bind!(r, tools::SET_SCENE_OBJECT_LIGHTMAP_ATTRIBUTES, |_, _| false);
    bind!(r, tools::GET_SCENE_WORLD_WIND, |_, _, _| {});
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
    bind!(r, tools::CREATE_CONSTANT_BUFFER, create_constant_buffer);
    bind!(r, tools::DESTROY_CONSTANT_BUFFER, destroy_constant_buffer);
    // TODO: constant buffers, shaders and textures bind once materials run translated shaders.
    bind!(r, tools::SET_CONSTANT_BUFFER_DATA, |_, _, _, _, _, _| {});
    bind!(r, tools::SET_DYNAMIC_CONSTANT_BUFFER_DATA, |_, _, _, _, _| {});
    bind!(r, ctx::SET_DYNAMIC_CONSTANT_BUFFER_DATA, |_, _, _, _, _| {});
    bind!(r, ctx::BIND_VERTEX_SHADER, |_, _, _| {});
    bind!(r, ctx::BIND_PIXEL_SHADER, |_, _| {});
    bind!(r, ctx::BIND_TEXTURE, |_, _, _| {});
    // Model draw calls through managed's material system; without it these draw nothing.
    bind!(r, tools::BIND_MODEL_DRAW_CALL, |_, _, _, _, _| false);
    bind!(r, tools::BIND_MODEL_DRAW_CALL_VERTEX_IDS, |_, _, _, _, _| false);
    bind!(r, tools::SET_RENDER_STATE_FOR_MODEL_DRAW_CALL, |_, _, _, _, _, _, _, _, _, _, _| false);
    bind!(r, tools::DRAW_MODEL_DRAW_CALL, |_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _| false);
    bind!(r, tools::DRAW_MODEL_DRAW_CALL_INDIRECT, |_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _| false);
    bind!(r, tools::DRAW_MODEL_DRAW_CALL_MULTI_INDIRECT, |_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _| false);
    // No skinning, morphs, compute, ray tracing or auto exposure on the placeholder renderer.
    bind!(r, tools::GET_MODEL_MESH_SKINNING, |_, _, _, _| -1);
    bind!(r, tools::GET_MODEL_MESH_SKINNING_BONES, |_, _, _, _, _| false);
    bind!(r, tools::ALLOCATE_VERTEX_CACHE, |_| -1);
    bind!(r, tools::GET_VERTEX_CACHE_BUFFER, PtrHandle::default);
    bind!(r, tools::DISPATCH_MODEL_MESH_SKINNING, |_, _, _, _, _, _, _, _, _, _, _, _, _, _, _| false);
    bind!(r, tools::MODEL_MESH_HAS_MORPHS, |_, _| false);
    bind!(r, tools::QUEUE_MODEL_MESH_MORPH, |_, _, _| -1);
    bind!(r, tools::GENERATE_MORPHS, |_| {});
    bind!(r, tools::GET_MORPH_SUBRECT, |_, _| {});
    bind!(r, tools::COMPUTE, |_, _, _, _, _, _| {});
    bind!(r, tools::COMPUTE_INDIRECT, |_, _, _, _, _| {});
    bind!(r, tools::TRACE_RAYS, |_, _, _, _, _, _| {});
    bind!(r, tools::TRACE_RAYS_INDIRECT, |_, _, _, _, _| {});
    bind!(r, tools::COPY_GPU_BUFFER_HIDDEN_STRUCTURE_COUNT, |_, _, _, _| {});
    bind!(r, tools::SET_GPU_BUFFER_HIDDEN_STRUCTURE_COUNT, |_, _, _| {});
    bind!(r, tools::TONEMAP_FRAME_UPDATE, |_, _| 1.0);
    bind!(r, tools::RENDER_AUTO_EXPOSURE, |_, _, _, _, _, _, _| {});
    // Contexts execute eagerly on one queue: barriers and semaphores have nothing to order.
    bind!(r, ctx::SEMAPHORE_SIGNAL_AT_END, |_| PtrHandle::default());
    bind!(r, ctx::SEMAPHORE_WAIT_AT_BEGIN, |_, _| {});
    bind!(r, ctx::TEXTURE_BARRIER_TRANSITION, |_, _, _, _, _, _, _, _| {});
    bind!(r, ctx::BUFFER_BARRIER_TRANSITION, |_, _, _, _, _, _| {});
    // TODO: mip generation and uint clears need compute or a blit pass.
    bind!(r, ctx::GENERATE_MIP_MAPS, |_, _| {});
    bind!(r, ctx::CLEAR_TEXTURE_UINT, |_, _, _| {});
    bind!(r, dev::FIND_OR_CREATE_SAMPLER_STATE, sampler);
    bind!(r, dev::GET_SAMPLER_INDEX, |h| h.value as i32 - 1);
    bind!(r, dev::GET_TEXTURE_VIEW_INDEX, |t, _, _| texture_view_index(t));
    bind!(r, dev::GET_SHEET_META_DATA_TEXTURE, || NativeRef::NULL);
    bind!(r, dev::GET_SHEET_INFO, |_| Vector4::default());
    bind!(r, dev::GET_SEQUENCE, |_, _| NativeRef::NULL);
    bind!(r, dev::GET_TEXTURE_RESIDENCY_INFO, |_, _, _| {});
    // Native API objects (VkImage, ID3D12Resource) aren't exposed through wgpu.
    bind!(r, dev::GET_DEVICE_SPECIFIC_INFO, |_| RawPtr::NULL);
    bind!(r, dev::GET_GRAPHICS_API_SPECIFIC_TEXTURE_HANDLE, |_| RawPtr::NULL);
    bind!(r, dev::GET_DEVICE_SPECIFIC_TEXTURE, |_| RawPtr::NULL);
}
