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
pub(super) const GRAVITY: u32 = 1 << 0;
pub(super) const AUTO_SLEEP: u32 = 1 << 1;
pub(super) const AWAKE: u32 = 1 << 2;
const BULLET: u32 = 1 << 3;
const OVERRIDE_MASS_CENTER: u32 = 1 << 4;

/// What native tracks per body that Rapier doesn't (mass overrides live in the simulation).
#[derive(Debug, Clone)]
pub struct State {
    pub gravity_scale: f32,
    pub gravity_enabled: bool,
    pub auto_sleep: bool,
    pub sleep_threshold: f32,
    pub override_mass_center: bool,
    pub local_mass_center: physics::Vector,
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

/// The body a Rapier body belongs to: `create` stores its object's address in `user_data`.
pub fn from_user_data(data: u128) -> Ref {
    Ref::from_raw(data as usize as *mut _)
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

/// Push native's gravity switch, sleep and mass-centre settings into the simulation.
pub fn apply_state(r: Ref) {
    let s = with_state(r, |s| Some(s.clone()));
    let Some(s) = s else { return };
    with_sim(r, |sim, id| {
        let scale = if s.gravity_enabled { s.gravity_scale } else { 0.0 };
        if let Some(rb) = sim.body_mut(id) {
            rb.set_gravity_scale(scale, true);
        }
        sim.set_auto_sleep(id, s.auto_sleep && sim.sleeping_enabled(), s.sleep_threshold);
        let center = s.override_mass_center.then_some(s.local_mass_center);
        if sim.mass_overrides(id).center != center {
            sim.set_mass_overrides(id, |m| m.center = center);
        }
    });
}

/// managed's motion-lock bits: X, Y, Z, pitch (about Y), yaw (about Z), roll (about X).
pub fn set_motion_locks(rb: &mut RigidBody, locks: [bool; 6]) {
    let [x, y, z, pitch, yaw, roll] = locks;
    rb.set_enabled_translations(!x, !y, !z, true);
    rb.set_enabled_rotations(!roll, !pitch, !yaw, true);
}

pub fn create(world_ref: world::Ref, kind: BodyType, pose: Pose, settings: &PhysicsBodySettings) -> HandleId<class::IPhysicsBody> {
    spawn(world_ref, kind, pose, settings).and_then(get).map_or(HandleId::new(-1), |b| b.handle.id())
}

/// As [`create`], answering the body itself.
pub fn spawn(world_ref: world::Ref, kind: BodyType, pose: Pose, settings: &PhysicsBodySettings) -> Option<Ref> {
    let world = world::get(world_ref)?;
    let id = {
        let mut sim = world.sim.lock().unwrap();
        let id = sim.add_body(kind, pose);
        if let Some(rb) = sim.body_mut(id) {
            rb.set_linvel(to_vec(settings.velocity), false);
            rb.set_angvel(to_vec(settings.angular_velocity), false);
            rb.set_linear_damping(settings.linear_damping);
            rb.set_angular_damping(settings.angular_damping);
            rb.enable_ccd(settings.flags & BULLET != 0);
            set_motion_locks(rb, std::array::from_fn(|i| settings.motion_locks & (1 << i) != 0));
            if settings.flags & AWAKE == 0 && kind == BodyType::Dynamic {
                rb.sleep();
            }
        }
        let scale = if settings.inertia_scale > 0.0 { settings.inertia_scale } else { 1.0 };
        sim.set_mass_overrides(id, |m| {
            m.mass = (settings.mass > 0.0).then_some(settings.mass);
            m.inertia_scale = scale;
        });
        id
    };
    let state = State {
        gravity_scale: settings.gravity_scale,
        gravity_enabled: settings.flags & GRAVITY != 0,
        auto_sleep: settings.flags & AUTO_SLEEP != 0,
        sleep_threshold: settings.sleep_threshold,
        override_mass_center: settings.flags & OVERRIDE_MASS_CENTER != 0,
        local_mass_center: to_vec(settings.mass_center),
    };
    let body = Arc::new(Body { world: world_ref, id, handle: Handle::default(), shapes: Mutex::default(), state: Mutex::new(state) });
    let r: Ref = objects::create_shared(body.clone());
    world.bodies.lock().unwrap().push(r);
    with_rb(r, |rb| rb.user_data = r.addr() as u128);
    apply_state(r);
    // Registered with the world unlocked: managed's RegisterHandle may call straight back in.
    body.handle.register::<_, class::IPhysicsBody>(r, Kind::PhysicsBody);
    Some(r)
}

/// Remove a body, its shapes, its joints and its managed handles.
pub fn delete(r: Ref) {
    let Some(body) = get(r) else { return };
    let shapes = std::mem::take(&mut *body.shapes.lock().unwrap());
    for s in shapes {
        shape::forget(s);
    }
    super::joint::forget_attached(body.world, r);
    if let Some(world) = world::get(body.world) {
        world.sim.lock().unwrap().remove_body(body.id);
        world.bodies.lock().unwrap().retain(|b| *b != r);
    }
    body.handle.free();
    objects::destroy(r);
}
