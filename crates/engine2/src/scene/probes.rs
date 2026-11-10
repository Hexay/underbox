//! Environment maps and sky boxes: their settings are scene-object state or generic fields
//! (`bind_fields`); what's left here is GPU work (cubemap filtering, SH projection), which the
//! empty device has nothing to do for. TODO: the renderer computes these (M2).

use super::object;
use abi::slots::{c_env_map_scene_object as env, c_scene_sky_box_object as sky, g_p_scene_system as system};
use abi::{Registry, bind};

pub fn register(r: &mut Registry) {
    bind!(r, env::CALCULATE_BOUNDS, |_| {});
    bind!(r, env::CALCULATE_NORMALIZATION_SH, |_| {});
    bind!(r, env::CALCULATE_RADIANCE_SH, |_| {});
    bind!(r, env::SET_WORLD_POSITION, |e, p| object::with(e.cast(), |st| st.transform.position = p));
    bind!(r, system::MARK_ENVIRONMENT_MAP_OBJECT_UPDATED, |_| {});
    bind!(r, system::MARK_LIGHT_PROBE_VOLUME_OBJECT_UPDATED, |_| {});
    bind!(r, sky::SET_SKY_TINT, |s, tint| {
        let v = tint.vec;
        object::with(s.cast(), |st| st.tint = crate::math::vec4(v.x, v.y, v.z, 1.0))
    });
}
