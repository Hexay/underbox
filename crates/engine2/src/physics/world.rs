//! `IPhysicsWorld`: one per scene, over a [`physics::World`].

use super::{body, events, joint};
use crate::handles::{Handle, Kind};
use crate::math::{from_vec, to_rot, to_vec};
use abi::slots::{g_p_physics_system as system, i_physics_world as w};
use abi::structs::PhysicsBodySettings;
use abi::{HandleId, NativeRef, Registry, bind, class};
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::IPhysicsWorld>;

static LIVE: AtomicI32 = AtomicI32::new(0);

#[derive(Default)]
pub struct PhysicsWorld {
    pub handle: Handle,
    pub sim: Mutex<physics::World>,
    pub bodies: Mutex<Vec<body::Ref>>,
    pub joints: Mutex<Vec<joint::Ref>>,
    reference: Mutex<body::Ref>,
    debug_scene: Mutex<NativeRef<class::ISceneWorld>>,
    simulation: AtomicI64,
}

pub fn get(r: Ref) -> Option<Arc<PhysicsWorld>> {
    objects::get::<_, PhysicsWorld>(r)
}

/// Run `f` on a world's simulation. Never call managed inside `f`: it may re-enter this world.
pub fn with_sim<R: Default>(r: Ref, f: impl FnOnce(&mut physics::World) -> R) -> R {
    get(r).map(|w| f(&mut w.sim.lock().unwrap())).unwrap_or_default()
}

fn create() -> HandleId<class::IPhysicsWorld> {
    let world = Arc::new(PhysicsWorld::default());
    let r: Ref = objects::create_shared(world.clone());
    LIVE.fetch_add(1, Ordering::Relaxed);
    world.handle.register(r, Kind::PhysicsWorld)
}

fn destroy(r: Ref) {
    let Some(world) = get(r) else { return };
    let joints = std::mem::take(&mut *world.joints.lock().unwrap());
    for j in joints {
        joint::delete(j);
    }
    let bodies = std::mem::take(&mut *world.bodies.lock().unwrap());
    for b in bodies {
        body::delete(b);
    }
    world.handle.free();
    objects::destroy(r);
    LIVE.fetch_sub(1, Ordering::Relaxed);
}

/// The settings managed's parameterless `AddBody` implies: a static body at the origin with
/// gravity and auto sleep at 2 u/s, as real's `PhysicsWorld.CreateBody()` reports.
fn default_settings() -> PhysicsBodySettings {
    PhysicsBodySettings {
        gravity_scale: 1.0,
        inertia_scale: 1.0,
        sleep_threshold: physics::DEFAULT_SLEEP_THRESHOLD,
        flags: body::GRAVITY | body::AUTO_SLEEP | body::AWAKE,
        rotation: crate::math::identity().rotation,
        ..Default::default()
    }
}

fn add_body(r: Ref, settings: PhysicsBodySettings) -> HandleId<class::IPhysicsBody> {
    let kind = physics::BodyType::from_raw(settings.body_type.into()).unwrap_or(physics::BodyType::Static);
    let pose = physics::Pose::from_parts(to_vec(settings.position), to_rot(settings.rotation));
    body::create(r, kind, pose, &settings)
}

pub fn register(r: &mut Registry) {
    bind!(r, system::CREATE_WORLD, create);
    bind!(r, system::DESTROY_WORLD, destroy);
    bind!(r, system::NUM_WORLDS, || LIVE.load(Ordering::Relaxed));

    bind!(r, w::ADD_BODY, |r| add_body(r, default_settings()));
    bind!(r, w::ADD_BODY_1, add_body);
    bind!(r, w::REMOVE_BODY, |_, b| body::delete(b));
    bind!(r, w::GET_WORLD_REFERENCE_BODY, |r| {
        let reference = get(r).map(|w| *w.reference.lock().unwrap()).unwrap_or_default();
        body::get(reference).map_or(HandleId::new(-1), |b| b.handle.id())
    });
    bind!(r, w::SET_WORLD_REFERENCE_BODY, |r, b| {
        if let Some(w) = get(r) {
            *w.reference.lock().unwrap() = b;
        }
    });
    bind!(r, w::MANAGED_OBJECT, |r| get(r).map_or(HandleId::new(-1), |w| w.handle.id()));

    bind!(r, w::SET_GRAVITY, |r, g| with_sim(r, |s| s.set_gravity(to_vec(g))));
    bind!(r, w::GET_GRAVITY, |r| with_sim(r, |s| from_vec(s.gravity())));
    bind!(r, w::SET_SIMULATION, |r, mode| get(r).into_iter().for_each(|w| w.simulation.store(mode, Ordering::Relaxed)));
    bind!(r, w::GET_SIMULATION, |r| get(r).map_or(0, |w| w.simulation.load(Ordering::Relaxed)));
    bind!(r, w::ENABLE_SLEEPING, |r| with_sim(r, |s| s.set_sleeping_enabled(true)));
    bind!(r, w::DISABLE_SLEEPING, |r| with_sim(r, |s| s.set_sleeping_enabled(false)));
    bind!(r, w::IS_SLEEPING_ENABLED, |r| with_sim(r, |s| s.sleeping_enabled()));
    bind!(r, w::SET_MAXIMUM_LINEAR_SPEED, |r, v| with_sim(r, |s| s.set_max_speed(v)));
    bind!(r, w::SET_COLLISION_RULES_FROM_JSON, |r, json| {
        if let Some(rules) = json.and_then(|j| j.to_str().ok()).and_then(super::rules::parse) {
            with_sim(r, |s| s.set_collision_rules(rules));
        }
    });
    bind!(r, w::STEP_SIMULATION, |r, dt, substeps| events::step(r, dt, substeps.max(1) as u32));
    bind!(r, w::PROCESS_INTERSECTIONS, events::process_intersections);
    bind!(r, w::SET_DEBUG_SCENE, |r, scene| get(r).into_iter().for_each(|w| *w.debug_scene.lock().unwrap() = scene));
    bind!(r, w::GET_DEBUG_SCENE, |r| {
        let scene = get(r).map(|w| *w.debug_scene.lock().unwrap()).unwrap_or_default();
        crate::scene::world::get(scene).map_or(HandleId::new(-1), |s| s.handle.id())
    });
    bind!(r, w::DRAW, |_, _| {});
}
