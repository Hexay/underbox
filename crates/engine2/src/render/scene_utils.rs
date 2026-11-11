//! `g_pSceneUtils`' per-view systems and the ray tracing world. The tonemap system and the
//! volumetric fog are real objects keeping what managed sets (fog isn't null-guarded: managed
//! calls `SetParams` straight on what `CreateVolumetricFog` returns). Exposure is render/exposure.rs;
//! fog is drawn by render/fog_volume.rs. Ray tracing isn't supported (`IsRayTracingSupported` is false),
//! so building its acceleration structures fails and managed skips ray traced effects.

use abi::slots::{g_p_material_system2_utils as utils, g_p_scene_utils as su, i_ray_trace_scene_world as rt, i_tonemap_system as tonemap, i_volumetric_fog as fog};
use abi::structs::{SceneTonemapParameters_t, SceneVolumetricFogParameters};
use abi::{NativeRef, Registry, bind, class};
use std::sync::Mutex;

use super::exposure::Tonemap;

#[derive(Default)]
pub struct Fog(pub Mutex<Option<(SceneVolumetricFogParameters, NativeRef<class::CTextureBase>)>>);

pub fn register(r: &mut Registry) {
    bind!(r, su::CREATE_TONEMAP_SYSTEM, || objects::create(Tonemap::default()));
    bind!(r, su::DESTROY_TONEMAP_SYSTEM, |t: NativeRef<class::ITonemapSystem>| {
        objects::destroy(t);
    });
    bind!(r, su::CREATE_VOLUMETRIC_FOG, || objects::create(Fog::default()));
    bind!(r, su::DESTROY_VOLUMETRIC_FOG, |f: NativeRef<class::IVolumetricFog>| {
        objects::destroy(f);
    });
    bind!(r, tonemap::SET_TONEMAP_PARAMETERS, |t, p: &mut SceneTonemapParameters_t| {
        if let Some(t) = objects::get::<_, Tonemap>(t) {
            t.0.lock().unwrap().params = Some(*p);
        }
    });
    bind!(r, tonemap::RESET_TONEMAP_PARAMETERS, |t| {
        if let Some(t) = objects::get::<_, Tonemap>(t) {
            *t.0.lock().unwrap() = Default::default();
        }
    });
    // Native's signature returns nothing; managed ignores it.
    bind!(r, fog::IS_FOGGING_ENABLED, |_| {});
    bind!(r, fog::SET_PARAMS, |f, p, t| {
        if let Some(f) = objects::get::<_, Fog>(f) {
            *f.0.lock().unwrap() = Some((p, t));
        }
    });
    bind!(r, rt::BEGIN_BUILD, |_| {});
    bind!(r, rt::ADD_SCENE_WORLD_TO_BUILD, |_, _, _| {});
    bind!(r, rt::END_BUILD, |_, _, _| false);
    bind!(r, rt::BUILD_TLAS_FOR_WORLD, |_, _, _| false);
    bind!(r, utils::DRAW_SCREEN_SPACE_RECTANGLE, |c, m, a, x, y, w, h, u0, v0, u1, v1, tw, th| {
        super::draw::screen_rect::draw(c, m, a, (x, y, w, h), (u0, v0, u1, v1), (tw, th));
    });
}
