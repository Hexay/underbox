//! Scene objects: one Rust type behind every `CSceneObject` class (lights, sky boxes, decals,
//! models...). Subclass slots inherit the base bindings (`abi::INHERITS`); state a subclass adds
//! lives in `extra`. Nothing draws on the empty device, but managed reads all of it back.

use super::world::{self, SceneWorld};
use crate::handles::{Handle, Kind};
use crate::render::attributes;
use abi::structs::{BBox, Transform, Vector3, Vector4};
use abi::{HandleId, NativeClass, NativeRef, class};
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::CSceneObject>;

#[derive(Clone)]
pub struct State {
    pub flags: i64,
    pub original_flags: i64,
    pub transform: Transform,
    pub bounds: BBox,
    pub tint: Vector4,
    pub alpha_fade: f32,
    pub cull_distance: f32,
    pub lighting_origin: Option<Vector3>,
    pub rendering: bool,
    pub loaded: bool,
    pub batchable: bool,
    pub tags: Vec<u32>,
    pub parent: Ref,
    pub children: Vec<Ref>,
    pub mesh_groups: u64,
    pub lod: Option<i32>,
    pub layer_match: u32,
    pub model: NativeRef<class::CModel>,
    /// Every mesh draws with this instead of its own material (`SetMaterialOverrideForMeshInstances`).
    pub material_override: Option<Arc<crate::mesh::material_ref::MaterialRef>>,
}

/// `SceneObjectFlags.IsOpaque`.
pub const IS_OPAQUE: i64 = 0x1;
/// `SceneObjectFlags.IsTranslucent`.
pub const IS_TRANSLUCENT: i64 = 0x2;

impl State {
    /// The flags managed reads: a model object also reports `IsOpaque` and/or `IsTranslucent`,
    /// which native derives from its draws' materials (`render::draw::model::surface_flags`).
    pub fn effective_flags(&self) -> i64 {
        if self.model.is_null() {
            return self.flags;
        }
        self.flags | crate::render::draw::model::surface_flags(self.model, self.mesh_groups, self.material_override.as_ref())
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            flags: 0,
            original_flags: 0,
            transform: crate::math::identity(),
            bounds: BBox::default(),
            tint: crate::math::vec4(1.0, 1.0, 1.0, 1.0),
            alpha_fade: 1.0,
            cull_distance: 0.0,
            lighting_origin: None,
            rendering: true,
            loaded: true,
            batchable: true,
            tags: Vec::new(),
            parent: Ref::NULL,
            children: Vec::new(),
            mesh_groups: !0,
            lod: None,
            layer_match: 0,
            model: NativeRef::NULL,
            material_override: None,
        }
    }
}

pub struct SceneObject {
    pub kind: Kind,
    pub world: world::Ref,
    pub handle: Handle,
    pub attributes: attributes::Ref,
    pub state: Mutex<State>,
    /// Lights' own state (light classes only).
    pub light: Option<Mutex<super::light::Light>>,
}

impl Drop for SceneObject {
    fn drop(&mut self) {
        objects::destroy(self.attributes);
        crate::resource::destroy_handle(self.state.get_mut().unwrap().model);
    }
}

pub fn get(r: Ref) -> Option<Arc<SceneObject>> {
    objects::get::<_, SceneObject>(r)
}

/// Run `f` on a live object's state; the default when it's gone.
pub fn with<R: Default>(r: Ref, f: impl FnOnce(&mut State) -> R) -> R {
    get(r).map(|o| f(&mut o.state.lock().unwrap())).unwrap_or_default()
}

/// Make an object of native class `C` in `world`, add it there, and register it with managed.
/// `D` is the class the creating slot returns it as (a base of `C`, or `C`).
pub fn create<C: NativeClass, D>(world: world::Ref, kind: Kind, state: State) -> HandleId<D> {
    create_with::<C, D>(world, kind, state, None)
}

/// As [`create`], for a light.
pub fn create_with<C: NativeClass, D>(world: world::Ref, kind: Kind, state: State, light: Option<super::light::Light>) -> HandleId<D> {
    create_native::<C, D>(world, kind, state, light).1
}

/// As [`create_with`], also returning the object's native reference (for native owners such as map worlds).
pub fn create_native<C: NativeClass, D>(world: world::Ref, kind: Kind, state: State, light: Option<super::light::Light>) -> (Ref, HandleId<D>) {
    let light = light.map(Mutex::new);
    let object = Arc::new(SceneObject { kind, world, handle: Handle::default(), attributes: attributes::create(), state: Mutex::new(state), light });
    let r: NativeRef<C> = objects::create_shared(object.clone());
    if let Some(w) = world::get(world) {
        w.objects.lock().unwrap().push(r.addr());
    }
    (NativeRef::from_raw(r.as_raw().cast()), object.handle.register(r, kind))
}

/// The live objects of `worlds`, each world's in creation order.
pub fn in_worlds(worlds: &[world::Ref]) -> Vec<Arc<SceneObject>> {
    refs_in_worlds(worlds).into_iter().map(|(_, o)| o).collect()
}

/// [`in_worlds`] with each object's reference.
pub fn refs_in_worlds(worlds: &[world::Ref]) -> Vec<(Ref, Arc<SceneObject>)> {
    let addrs = worlds.iter().filter_map(|w| world::get(*w)).flat_map(|w| w.objects.lock().unwrap().clone());
    addrs.filter_map(|a| get(NativeRef::from_raw(a as *mut _)).map(|o| (NativeRef::from_raw(a as *mut _), o))).collect()
}

/// Remove an object from its world and parent, tell managed, and free it.
pub fn delete(r: Ref) {
    let Some(object) = get(r) else { return };
    if let Some(w) = world::get(object.world) {
        remove_from(&w, r);
    }
    let parent = object.state.lock().unwrap().parent;
    with(parent, |p| p.children.retain(|c| *c != r));
    object.handle.free();
    objects::destroy(r);
}

fn remove_from(world: &SceneWorld, r: Ref) {
    world.objects.lock().unwrap().retain(|a| *a != r.addr());
}
