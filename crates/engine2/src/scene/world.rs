//! `ISceneWorld`: a set of scene objects managed creates per scene (`g_pSceneSystem.CreateWorld`).

use crate::handles::{Handle, Kind};
use abi::slots::{g_p_scene_system as system, i_scene_world as w};
use abi::{HandleId, NativeRef, Registry, StrRet, bind, class};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

pub type Ref = NativeRef<class::ISceneWorld>;

#[derive(Default)]
pub struct SceneWorld {
    name: String,
    pub handle: Handle,
    /// Object addresses, in creation order.
    pub objects: Mutex<Vec<usize>>,
    delete_at_end_of_frame: AtomicBool,
}

pub fn get(r: Ref) -> Option<std::sync::Arc<SceneWorld>> {
    objects::get::<_, SceneWorld>(r)
}

fn create(name: Option<&std::ffi::CStr>) -> HandleId<class::ISceneWorld> {
    let name = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let world = std::sync::Arc::new(SceneWorld { name, ..Default::default() });
    let r: Ref = objects::create_shared(world.clone());
    world.handle.register(r, Kind::SceneWorld)
}

/// Delete every object, then the world itself.
fn destroy(r: Ref) {
    delete_all(r);
    super::world_queries::forget(r);
    if let Some(world) = get(r) {
        world.handle.free();
    }
    objects::destroy(r);
}

fn delete_all(r: Ref) {
    let Some(world) = get(r) else { return };
    let all = std::mem::take(&mut *world.objects.lock().unwrap());
    for addr in all {
        super::object::delete(NativeRef::from_raw(addr as *mut _));
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, system::CREATE_WORLD, create);
    bind!(r, system::DESTROY_WORLD, destroy);
    bind!(r, w::RELEASE, destroy);
    bind!(r, w::DELETE_ALL_OBJECTS, delete_all);
    // Objects free as soon as they're deleted (see object_slots), so none wait for frame end.
    bind!(r, w::DELETE_END_OF_FRAME_OBJECTS, |_| {});
    bind!(r, w::GET_SCENE_OBJECT_COUNT, |r| get(r).map_or(0, |w| w.objects.lock().unwrap().len() as i32));
    bind!(r, w::IS_EMPTY, |r| get(r).is_none_or(|w| w.objects.lock().unwrap().is_empty()));
    bind!(r, w::GET_WORLD_DEBUG_NAME, |r| StrRet::from(get(r).map(|w| w.name.clone())));
    bind!(r, w::SET_DELETE_AT_END_OF_FRAME, |r, v| {
        if let Some(w) = get(r) {
            w.delete_at_end_of_frame.store(v, Ordering::Relaxed);
        }
    });
    bind!(r, w::GET_DELETE_AT_END_OF_FRAME, |r| get(r).is_some_and(|w| w.delete_at_end_of_frame.load(Ordering::Relaxed)));
}
