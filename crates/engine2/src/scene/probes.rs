//! Environment maps, light probe volumes and sky boxes: their settings are scene-object state or
//! generic fields (`bind_fields`). The GPU work native does on them (cubemap filtering, SH
//! projection, probe constant buffers) has no consumer here yet: the renderer's env map and
//! probe clusters are empty (render/lighting/blocks.rs), so those calls are accepted and dropped.

use super::object;
use abi::slots::{c_env_map_scene_object as env, c_scene_light_probe_volume_object as probe, c_scene_sky_box_object as sky, g_p_scene_system as system};
use abi::{Registry, bind};

pub fn register(r: &mut Registry) {
    bind!(r, env::CALCULATE_BOUNDS, |_| {});
    bind!(r, env::CALCULATE_NORMALIZATION_SH, |_| {});
    bind!(r, env::CALCULATE_RADIANCE_SH, |_| {});
    bind!(r, env::CALCULATE_RADIANCE_SH_1, |_, _| {});
    bind!(r, env::SET_WORLD_POSITION, |e, p| object::with(e.cast(), |st| st.transform.position = p));
    bind!(r, env::GET_WORLD_POSITION, |e| object::with(e.cast(), |st| st.transform.position));
    bind!(r, probe::CREATE_CONSTANTS, |_| {});
    bind!(r, system::MARK_ENVIRONMENT_MAP_OBJECT_UPDATED, |_| {});
    bind!(r, system::MARK_LIGHT_PROBE_VOLUME_OBJECT_UPDATED, |_| {});
    bind!(r, sky::SET_SKY_TINT, |s, tint| {
        let v = tint.vec;
        object::with(s.cast(), |st| st.tint = crate::math::vec4(v.x, v.y, v.z, 1.0))
    });
}
