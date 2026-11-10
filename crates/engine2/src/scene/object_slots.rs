//! `CSceneObject`'s slots, shared by every scene-object class through `abi::INHERITS`.

use super::object::{self, get, with};
use abi::slots::{c_scene_object as o, g_p_scene_system as system};
use abi::{HandleId, Registry, bind};

pub fn register(r: &mut Registry) {
    bind!(r, system::DELETE_SCENE_OBJECT, object::delete);
    // Objects free when deleted: the empty device has no in-flight frame still drawing them.
    bind!(r, system::DELETE_SCENE_OBJECT_AT_FRAME_END, object::delete);
    bind!(r, system::PERFORM_END_OF_FRAME_DELETES, || {});

    bind!(r, o::SET_FLAGS, |s, f| with(s, |st| st.flags |= f));
    bind!(r, o::CLEAR_FLAGS, |s, f| with(s, |st| st.flags &= !f));
    bind!(r, o::CHANGE_FLAGS, |s, set, clear| with(s, |st| st.flags = (st.flags & !clear) | set));
    bind!(r, o::HAS_FLAGS, |s, f| with(s, |st| st.effective_flags() & f == f));
    bind!(r, o::GET_FLAGS, |s| with(s, |st| st.effective_flags()));
    bind!(r, o::GET_ORIGINAL_FLAGS, |s| with(s, |st| st.original_flags));
    bind!(r, o::UPDATE_FLAGS_BASED_ON_MATERIAL, |_| {});

    bind!(r, o::SET_TRANSFORM, |s, t| with(s, |st| st.transform = t));
    bind!(r, o::GET_C_TRANSFORM, |s| with(s, |st| Some(st.transform)).unwrap_or_else(crate::math::identity));
    bind!(r, o::SET_BOUNDS, |s, b| with(s, |st| st.bounds = b));
    bind!(r, o::GET_BOUNDS, |s| with(s, |st| st.bounds));
    bind!(r, o::SET_BOUNDS_INFINITE, |s| {
        let (lo, hi) = (crate::math::vec3(f32::MIN, f32::MIN, f32::MIN), crate::math::vec3(f32::MAX, f32::MAX, f32::MAX));
        with(s, |st| (st.bounds.mins, st.bounds.maxs) = (lo, hi))
    });
    bind!(r, o::GET_BOUNDING_SPHERE_RADIUS, |s| {
        with(s, |st| {
            let (a, b) = (st.bounds.mins.vec, st.bounds.maxs.vec);
            0.5 * ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt()
        })
    });
    bind!(r, o::SET_CULL_DISTANCE, |s, d| with(s, |st| st.cull_distance = d));

    bind!(r, o::SET_TINT_RGBA, |s, c| with(s, |st| st.tint = c));
    bind!(r, o::GET_TINT_RGBA, |s| with(s, |st| Some(st.tint)).unwrap_or_default());
    bind!(r, o::SET_ALPHA_FADE, |s, a| with(s, |st| st.alpha_fade = a));
    bind!(r, o::GET_ALPHA_FADE, |s| with(s, |st| st.alpha_fade));
    bind!(r, o::ENABLE_LIGHTING_CACHE, |_| {});
    bind!(r, o::SET_LIGHTING_ORIGIN, |s, p, enabled| with(s, |st| st.lighting_origin = enabled.then_some(p)));
    bind!(r, o::GET_LIGHTING_ORIGIN, |s| with(s, |st| st.lighting_origin).unwrap_or_default());
    bind!(r, o::HAS_LIGHTING_ORIGIN, |s| with(s, |st| st.lighting_origin.is_some()));

    bind!(r, o::IS_LOADED, |s| with(s, |st| st.loaded));
    bind!(r, o::SET_LOADED, |s| with(s, |st| st.loaded = true));
    bind!(r, o::CLEAR_LOADED, |s| with(s, |st| st.loaded = false));
    bind!(r, o::IS_RENDERING_ENABLED, |s| with(s, |st| st.rendering));
    bind!(r, o::ENABLE_RENDERING, |s| with(s, |st| st.rendering = true));
    bind!(r, o::DISABLE_RENDERING, |s| with(s, |st| st.rendering = false));
    bind!(r, o::SET_RENDERING_ENABLED, |s, v| with(s, |st| st.rendering = v));
    bind!(r, o::SET_BATCHABLE, |s, v| with(s, |st| st.batchable = v));
    bind!(r, o::IS_NOT_BATCHABLE, |s| with(s, |st| !st.batchable));
    bind!(r, o::SET_UNIQUE_BATCH_GROUP, |_, _| {});

    bind!(r, o::GET_PARENT, |s| {
        let parent = with(s, |st| st.parent);
        get(parent).map_or(HandleId::new(-1), |p| p.handle.id())
    });
    bind!(r, o::ADD_CHILD_OBJECT, |s, _name, child, _flags| {
        with(child, |c| c.parent = s);
        with(s, |st| st.children.push(child));
    });
    bind!(r, o::REMOVE_CHILD, |s, child| {
        with(child, |c| c.parent = object::Ref::NULL);
        with(s, |st| st.children.retain(|c| *c != child));
    });
    bind!(r, o::GET_WORLD, |s| get(s).and_then(|o| super::world::get(o.world)).map_or(HandleId::new(-1), |w| w.handle.id()));
    bind!(r, o::GET_ATTRIBUTES_PTR_FOR_MODIFY, |s| get(s).map(|o| o.attributes).unwrap_or_default());

    bind!(r, o::ADD_TAG, |s, t| with(s, |st| {
        if !st.tags.contains(&t) {
            st.tags.push(t)
        }
    }));
    bind!(r, o::REMOVE_TAG, |s, t| with(s, |st| st.tags.retain(|x| *x != t)));
    bind!(r, o::REMOVE_ALL_TAGS, |s| with(s, |st| st.tags.clear()));
    bind!(r, o::HAS_TAG, |s, t| with(s, |st| st.tags.contains(&t)));
    bind!(r, o::GET_TAG_COUNT, |s| with(s, |st| st.tags.len() as i32));
    bind!(r, o::GET_TAG_AT, |s, i| with(s, |st| usize::try_from(i).ok().and_then(|i| st.tags.get(i).copied()).unwrap_or(0)));
    bind!(r, o::SET_FORCE_LAYER_ID, |_, _| {});
    bind!(r, o::SET_LAYER_MATCH_ID, |s, id| with(s, |st| st.layer_match = id.0));
    bind!(r, o::GET_LAYER_MATCH_ID_VALUE, |s| with(s, |st| st.layer_match));

    bind!(r, o::ENABLE_MESH_GROUPS, |s, m| with(s, |st| st.mesh_groups |= m));
    bind!(r, o::DISABLE_MESH_GROUPS, |s, m| with(s, |st| st.mesh_groups &= !m));
    bind!(r, o::RESET_MESH_GROUPS, |s, m| with(s, |st| st.mesh_groups = m));
    bind!(r, o::GET_CURRENT_MESH_GROUP_MASK, |s| with(s, |st| st.mesh_groups));
    bind!(r, o::SET_LOD, |s, l| with(s, |st| st.lod = Some(l)));
    bind!(r, o::DISABLE_LOD, |s| with(s, |st| st.lod = None));
    bind!(r, o::GET_CURRENT_LOD_LEVEL, |s| with(s, |st| st.lod.unwrap_or(0)));
    bind!(r, o::GET_CURRENT_LOD_GROUP_MASK, |_| 1);
    bind!(r, o::GET_MODEL_HANDLE, |s| with(s, |st| crate::model::copy(st.model)));
    bind!(r, o::GET_INT_VALUE, |_, _, default| default);
    bind!(r, o::SET_MATERIAL_OVERRIDE_FOR_MESH_INSTANCES, |s, m| with(s, |st| st.material_override = crate::mesh::material_ref::MaterialRef::keep(m)));
    // TODO: per-attribute and per-index overrides and material groups.
    bind!(r, o::CLEAR_MATERIAL_OVERRIDE_LIST, |_| {});
    bind!(r, o::SET_MATERIAL_OVERRIDE, |_, _, _, _| {});
    bind!(r, o::SET_MATERIAL_OVERRIDE_BY_INDEX, |_, _, _| {});
    bind!(r, o::SET_MATERIAL_GROUP, |_, _| {});
    bind!(r, o::SET_BODY_GROUP, |_, _, _| {});
}
