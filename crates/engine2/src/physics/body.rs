//! `IPhysicsBody`: a rigid body in a world's simulation, plus the state native keeps beside it.

use super::{shape, world};
use crate::handles::{Handle, Kind};
use crate::math::to_vec;
use abi::structs::PhysicsBodySettings;
use abi::{HandleId, NativeRef, class};
use physics::{BodyId, BodyType, Pose, RigidBody};
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::IPhysicsBody>;

/// managed's `PhysicsBodyCreationFlags`.
const GRAVITY: u32 = 1 << 0;
const AUTO_SLEEP: u32 = 1 << 1;
const BULLET: u32 = 1 << 3;
const OVERRIDE_MASS_CENTER: u32 = 1 << 4;

/// What native tracks per body that Rapier doesn't.
#[derive(Debug, Clone)]
pub struct State {
    pub gravity_scale: f32,
    pub gravity_enabled: bool,
    pub auto_sleep: bool,
    pub sleep_threshold: f32,
    pub touch_events: bool,
    pub override_mass_center: bool,
    pub local_mass_center: physics::Vector,
    pub mass_updates_deferred: bool,
}

pub struct Body {
    pub world: world::Ref,
    pub id: BodyId,
    pub handle: Handle,
    pub shapes: Mutex<Vec<shape::Ref>>,
    pub state: Mutex<State>,
}

pub fn get(r: Ref) -> Option<Arc<Body>> {
    objects::get::<_, Body>(r)
}

/// Run `f` on a live body in its simulation (the world locked). Never call managed inside `f`.
pub fn with_sim<R: Default>(r: Ref, f: impl FnOnce(&mut physics::World, BodyId) -> R) -> R {
    let Some(body) = get(r) else { return R::default() };
    world::with_sim(body.world, |s| f(s, body.id))
}

/// As [`with_sim`], for just the Rapier body.
pub fn with_rb<R: Default>(r: Ref, f: impl FnOnce(&mut RigidBody) -> R) -> R {
    with_sim(r, |s, id| s.body_mut(id).map(f).unwrap_or_default())
}

pub fn with_state<R: Default>(r: Ref, f: impl FnOnce(&mut State) -> R) -> R {
    get(r).map(|b| f(&mut b.state.lock().unwrap())).unwrap_or_default()
}

/// Rapier's gravity scale from native's scale and on/off switch.
pub fn apply_gravity(r: Ref) {
    let scale = with_state(r, |s| if s.gravity_enabled { s.gravity_scale } else { 0.0 });
    with_rb(r, |rb| rb.set_gravity_scale(scale, true));
}

pub fn create(world_ref: world::Ref, kind: BodyType, pose: Pose, settings: &PhysicsBodySettings) -> HandleId<class::IPhysicsBody> {
    let Some(world) = world::get(world_ref) else { return HandleId::new(-1) };
    let id = {
        let mut sim = world.sim.lock().unwrap();
        let id = sim.add_body(kind, pose);
        if let Some(rb) = sim.body_mut(id) {
            rb.set_linvel(to_vec(settings.velocity), false);
            rb.set_angvel(to_vec(settings.angular_velocity), false);
            rb.set_linear_damping(settings.linear_damping);
            rb.set_angular_damping(settings.angular_damping);
            rb.enable_ccd(settings.flags & BULLET != 0);
        }
        id
    };
    let state = State {
        gravity_scale: settings.gravity_scale,
        gravity_enabled: settings.flags & GRAVITY != 0,
        auto_sleep: settings.flags & AUTO_SLEEP != 0,
        sleep_threshold: settings.sleep_threshold,
        touch_events: false,
        override_mass_center: settings.flags & OVERRIDE_MASS_CENTER != 0,
        local_mass_center: to_vec(settings.mass_center),
        mass_updates_deferred: false,
    };
    let body = Arc::new(Body { world: world_ref, id, handle: Handle::default(), shapes: Mutex::default(), state: Mutex::new(state) });
    let r: Ref = objects::create_shared(body.clone());
    world.bodies.lock().unwrap().push(r);
    apply_gravity(r);
    // Registered with the world unlocked: managed's RegisterHandle may call straight back in.
    body.handle.register(r, Kind::PhysicsBody)
}

/// Remove a body, its shapes and its managed handles.
pub fn delete(r: Ref) {
    let Some(body) = get(r) else { return };
    let shapes = std::mem::take(&mut *body.shapes.lock().unwrap());
    for s in shapes {
        shape::forget(s);
    }
    if let Some(world) = world::get(body.world) {
        world.sim.lock().unwrap().remove_body(body.id);
        world.bodies.lock().unwrap().retain(|b| *b != r);
    }
    body.handle.free();
    objects::destroy(r);
}
