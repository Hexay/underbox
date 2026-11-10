//! Where scene objects come from: the scene system's sky/decal/probe factories, the mesh
//! system's model objects, and managed's own scene object classes (lights are in light.rs).

use super::object::{self, State};
use crate::handles::Kind;
use abi::slots::{c_dynamic_scene_object as dynamic, c_managed_scene_object as managed, g_p_mesh_system as mesh, g_p_scene_system as system};
use abi::{Registry, bind, class};

pub fn register(r: &mut Registry) {
    use class::{CDecalSceneObject as Decal, CEnvMapSceneObject as EnvMap};
    bind!(r, system::CREATE_ENV_MAP, |w, _projection| object::create::<EnvMap, _>(w, Kind::EnvMapSceneObject, State::default()));
    bind!(r, system::CREATE_LIGHT_PROBE_VOLUME, |w| {
        object::create::<class::CSceneLightProbeVolumeObject, _>(w, Kind::SceneLightProbeVolumeObject, State::default())
    });
    bind!(r, system::CREATE_SKY_BOX, |_material, w| object::create::<class::CSceneSkyBoxObject, _>(w, Kind::SceneSkyBoxObject, State::default()));
    bind!(r, system::CREATE_DECAL, |w| object::create::<Decal, _>(w, Kind::DecalSceneObject, State::default()));
    bind!(r, managed::CREATE, |w| object::create::<class::CManagedSceneObject, _>(w, Kind::ManagedSceneObject, State::default()));
    bind!(r, dynamic::CREATE, |w| object::create::<class::CDynamicSceneObject, _>(w, Kind::SceneObject, State::default()));

    bind!(r, mesh::CREATE_SCENE_OBJECT, |model, transform, desc, flags, _type_flags, w, _unused| {
        // Animatable objects (SceneModel) ask by descriptor name; plain ones pass none.
        let animatable = desc.is_some_and(|d| d.to_bytes() == b"AnimatableSceneObjectDesc");
        let state = State { model: crate::model::copy(model), transform, flags, original_flags: flags, bounds: crate::model::bounds(model), ..State::default() };
        match animatable {
            true => object::create::<class::CSceneAnimatableObject, _>(w, Kind::SceneAnimatableObject, state),
            false => object::create::<class::CSceneObject, _>(w, Kind::SceneObject, state),
        }
    });
    bind!(r, mesh::CHANGE_MODEL, |s, model| {
        let Some(obj) = object::get(s) else { return };
        let old = std::mem::replace(&mut obj.state.lock().unwrap().model, crate::model::copy(model));
        crate::resource::destroy_handle(old);
    });
}
