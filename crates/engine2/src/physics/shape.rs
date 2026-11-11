//! `IPhysicsShape`: one collider on a body. Tags and collision switches live in the simulation's
//! [`physics::ShapeMeta`] (the contact filter reads them); trace-only state lives here.

use super::body;
use crate::handles::{Handle, Kind};
use abi::structs::PhysicsShapeSettings;
use abi::{HandleId, NativeRef, StringToken, class};
use physics::{Shape as Geometry, ShapeId, ShapeMeta, Surface};
use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex};

pub type Ref = NativeRef<class::IPhysicsShape>;

/// managed's `PhysicsShapeType`.
#[derive(Debug, Clone, Copy)]
pub enum Type {
    Sphere = 0,
    Capsule = 1,
    Hull = 2,
    Mesh = 3,
    HeightField = 4,
}

/// managed's `PhysicsShapeCreationFlags`.
const SENSOR: u32 = 1 << 0;
const IGNORE_TRACES: u32 = 1 << 1;
const NO_MASS: u32 = 1 << 2;
const DEFAULT_SOLID_TAG: u32 = 1 << 5;

#[derive(Debug, Default, Clone)]
pub struct State {
    pub ignore_traces: bool,
    pub local_velocity: abi::structs::Vector3,
    pub surface_index: i32,
    /// Mesh shapes: each triangle's material slot, and each slot's surface (`SetSurfaceIndex`).
    pub triangle_slots: Vec<u8>,
    pub slot_surfaces: Vec<i32>,
    /// Friction and elasticity came from the surface (settings below zero), so follow its reloads.
    pub inherits: (bool, bool),
}

impl State {
    /// The surface at `triangle` (a mesh hit's), else the shape's own.
    pub fn surface_at(&self, triangle: Option<u32>) -> i32 {
        let slot = triangle.and_then(|t| self.triangle_slots.get(t as usize));
        slot.and_then(|&s| self.slot_surfaces.get(usize::from(s))).copied().filter(|&s| s >= 0).unwrap_or(self.surface_index)
    }

    pub fn set_slot_surface(&mut self, slot: usize, surface: i32) {
        if self.slot_surfaces.len() <= slot {
            self.slot_surfaces.resize(slot + 1, -1);
        }
        self.slot_surfaces[slot] = surface;
    }
}

pub struct Shape {
    pub body: body::Ref,
    pub id: ShapeId,
    pub kind: Type,
    pub handle: Handle,
    pub geometry: Mutex<Geometry>,
    pub state: Mutex<State>,
}

pub fn get(r: Ref) -> Option<Arc<Shape>> {
    objects::get::<_, Shape>(r)
}

/// The shape a collider belongs to: `add` stores its object's address in `user_data`.
pub fn from_user_data(data: u128) -> Ref {
    Ref::from_raw(data as usize as *mut _)
}

pub fn with_state<R: Default>(r: Ref, f: impl FnOnce(&mut State) -> R) -> R {
    get(r).map(|s| f(&mut s.state.lock().unwrap())).unwrap_or_default()
}

/// Run `f` on the shape's simulation with its collider id (the world locked).
pub fn with_sim<R: Default>(r: Ref, f: impl FnOnce(&mut physics::World, ShapeId) -> R) -> R {
    let Some(shape) = get(r) else { return R::default() };
    body::with_sim(shape.body, |sim, _| f(sim, shape.id))
}

pub fn with_collider<R: Default>(r: Ref, f: impl FnOnce(&mut physics::Collider) -> R) -> R {
    with_sim(r, |sim, id| sim.shape_mut(id).map(f).unwrap_or_default())
}

/// Read the shape's tags and switches.
pub fn with_meta<R: Default>(r: Ref, f: impl FnOnce(&ShapeMeta) -> R) -> R {
    with_sim(r, |sim, id| sim.shape_meta(id).map(f).unwrap_or_default())
}

/// Change the shape's tags and switches.
pub fn update_meta<R: Default>(r: Ref, f: impl FnOnce(&mut ShapeMeta) -> R) -> R {
    with_sim(r, |sim, id| {
        let mut out = R::default();
        sim.update_shape_meta(id, |m| out = f(m));
        out
    })
}

/// The surface a shape's physics come from: `surface_index` if it names one, else `default` (0).
fn resolved(surface_index: i32) -> i32 {
    if super::surfaces::physical(surface_index).is_some() { surface_index } else { 0 }
}

/// Settings values below zero mean "the surface's" (see [`resolved`]).
fn surface(settings: &PhysicsShapeSettings) -> Surface {
    let fallback = super::surfaces::physical(resolved(settings.surface_index)).unwrap_or_default();
    let pick = |v: f32, d: f32| if v >= 0.0 { v } else { d };
    Surface { friction: pick(settings.friction, fallback.friction), elasticity: pick(settings.elasticity, fallback.elasticity), trigger: settings.flags & SENSOR != 0 }
}

/// Each material slot's surface (`SurfaceIndices`, -1 for the shape's own), as managed passes them.
pub fn slot_surfaces(settings: &PhysicsShapeSettings) -> Vec<i32> {
    let len = usize::try_from(settings.surface_count).unwrap_or(0);
    let p = abi::RawPtr::<i32>::from_raw(settings.surface_indices as *mut i32);
    // SAFETY: managed's `SurfaceIndices` points at `SurfaceCount` ints, pinned for the call.
    unsafe { p.as_slice(len) }.to_vec()
}

fn meta(settings: &PhysicsShapeSettings) -> ShapeMeta {
    let mut tags: Vec<u32> = settings.tags.iter().copied().take_while(|t| *t != 0).collect();
    if tags.is_empty() && settings.flags & DEFAULT_SOLID_TAG != 0 {
        tags.push(StringToken::of("solid").0);
    }
    ShapeMeta { tags, functions: settings.collision_function_mask as u8, massless: settings.flags & NO_MASS != 0, ..Default::default() }
}

pub fn add(body_ref: body::Ref, kind: Type, geometry: Geometry, settings: PhysicsShapeSettings) -> HandleId<class::IPhysicsShape> {
    add_with_slots(body_ref, kind, geometry, settings, Vec::new(), Vec::new())
}

/// As [`add`], for a mesh whose triangles use material slots mapped to surfaces (see [`State`]).
pub fn add_with_slots(
    body_ref: body::Ref,
    kind: Type,
    geometry: Geometry,
    settings: PhysicsShapeSettings,
    triangle_slots: Vec<u8>,
    slot_surfaces: Vec<i32>,
) -> HandleId<class::IPhysicsShape> {
    let id = body::with_sim(body_ref, |sim, id| {
        let shape = sim.add_shape(id, &geometry, surface(&settings))?;
        sim.update_shape_meta(shape, |m| *m = meta(&settings));
        Some(shape)
    });
    let Some(id) = id else { return HandleId::new(-1) };
    let state = State {
        ignore_traces: settings.flags & IGNORE_TRACES != 0,
        local_velocity: settings.surface_velocity,
        surface_index: settings.surface_index,
        triangle_slots,
        slot_surfaces,
        inherits: (settings.friction < 0.0, settings.elasticity < 0.0),
    };
    let shape = Arc::new(Shape { body: body_ref, id, kind, handle: Handle::default(), geometry: Mutex::new(geometry), state: Mutex::new(state) });
    let r: Ref = objects::create_shared(shape.clone());
    LIVE.lock().unwrap().insert(r.addr());
    body::with_sim(body_ref, |sim, _| sim.shape_mut(id).map(|c| c.user_data = r.addr() as u128));
    if let Some(body) = body::get(body_ref) {
        body.shapes.lock().unwrap().push(r);
    }
    shape.handle.register(r, Kind::PhysicsShape)
}

/// Drop a shape's native object and managed handle; its collider goes with its body.
pub fn forget(r: Ref) {
    if let Some(shape) = get(r) {
        shape.handle.free();
    }
    LIVE.lock().unwrap().remove(&r.addr());
    objects::destroy(r);
}

/// Every live shape, for surface reloads.
static LIVE: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(Default::default);

/// Give shapes that inherit surface `index`'s friction or elasticity its current values.
pub fn reapply_surface(index: i32) {
    let live: Vec<Ref> = LIVE.lock().unwrap().iter().map(|&a| Ref::from_raw(a as *mut _)).collect();
    for r in live.into_iter().filter(|&r| with_state(r, |st| resolved(st.surface_index) == index)) {
        apply_inherited(r);
    }
}

/// Copy the shape's surface friction and elasticity into its collider where it inherits them.
fn apply_inherited(r: Ref) {
    let (inherits, own) = with_state(r, |st| (st.inherits, resolved(st.surface_index)));
    let p = super::surfaces::physical(own).unwrap_or_default();
    with_collider(r, |c| {
        if inherits.0 {
            c.set_friction(p.friction);
        }
        if inherits.1 {
            c.set_restitution(p.elasticity);
        }
    });
}

/// `SetMaterialIndex`: the shape now uses surface `index` (unknown: `default`).
pub fn set_surface(r: Ref, index: i32) {
    with_state(r, |st| st.surface_index = index);
    apply_inherited(r);
}

/// `SetFriction`/`SetElasticity` (`elasticity`); below zero reverts to the surface's value.
pub fn set_physical(r: Ref, elasticity: bool, v: f32) {
    let inherit = v < 0.0;
    with_state(r, |st| if elasticity { st.inherits.1 = inherit } else { st.inherits.0 = inherit });
    if inherit {
        apply_inherited(r);
    } else {
        with_collider(r, |c| if elasticity { c.set_restitution(v) } else { c.set_friction(v) });
    }
}

/// The name of the surface the shape uses.
pub fn surface_name(r: Ref) -> String {
    super::surfaces::name(with_state(r, |st| resolved(st.surface_index))).unwrap_or_default()
}

pub fn remove(body_ref: body::Ref, r: Ref) {
    let Some(shape) = get(r) else { return };
    body::with_sim(body_ref, |sim, _| sim.remove_shape(shape.id));
    if let Some(body) = body::get(body_ref) {
        body.shapes.lock().unwrap().retain(|x| *x != r);
    }
    forget(r);
}

pub fn update(r: Ref, geometry: Geometry) {
    let Some(shape) = get(r) else { return };
    body::with_sim(shape.body, |sim, _| sim.update_shape(shape.id, &geometry));
    *shape.geometry.lock().unwrap() = geometry;
}

/// Set or clear `tag`; whether that changed anything.
pub fn toggle_tag(r: Ref, tag: u32, on: bool) -> bool {
    update_meta(r, |m| match (on, m.tags.iter().position(|t| *t == tag)) {
        (true, None) => {
            m.tags.push(tag);
            true
        }
        (false, Some(i)) => {
            m.tags.remove(i);
            true
        }
        _ => false,
    })
}
