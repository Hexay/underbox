//! `IPhysicsShape`: one collider on a body, plus the tags and masks traces filter on.

use super::body;
use crate::handles::{Handle, Kind};
use crate::math::{from_vec, to_rot, to_vec};
use abi::slots::{i_physics_body as b, i_physics_shape as s};
use abi::structs::{BBox, Capsule, PhysicsShapeSettings, Sphere};
use abi::{HandleId, NativeRef, Registry, StrRet, bind, class};
use physics::{Shape as Geometry, ShapeId, Surface};
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::IPhysicsShape>;

/// managed's `PhysicsShapeType`.
#[derive(Debug, Clone, Copy)]
enum Type {
    Sphere = 0,
    Capsule = 1,
    Hull = 2,
    Mesh = 3,
}

fn hull(position: abi::structs::Vector3, rotation: abi::structs::Rotation, points: &[abi::structs::Vector3]) -> Geometry {
    Geometry::Hull { pose: physics::Pose::from_parts(to_vec(position), to_rot(rotation)), points: points.iter().copied().map(to_vec).collect() }
}

/// Triangles from a flat index list; a trailing partial triangle or a negative index is dropped.
fn mesh(vertices: &[abi::structs::Vector3], indices: &[i32]) -> Geometry {
    let triangles = indices.as_chunks::<3>().0.iter().filter_map(|t| Some([t[0].try_into().ok()?, t[1].try_into().ok()?, t[2].try_into().ok()?]));
    Geometry::Mesh { vertices: vertices.iter().copied().map(to_vec).collect(), triangles: triangles.collect() }
}

/// managed's `PhysicsShapeCreationFlags`.
const SENSOR: u32 = 1 << 0;
const IGNORE_TRACES: u32 = 1 << 1;

#[derive(Debug, Default, Clone)]
pub struct State {
    pub tags: Vec<u32>,
    pub collision_mask: u8,
    pub ignore_traces: bool,
    pub local_velocity: abi::structs::Vector3,
}

pub struct Shape {
    pub body: body::Ref,
    pub id: ShapeId,
    kind: Type,
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

fn with_state<R: Default>(r: Ref, f: impl FnOnce(&mut State) -> R) -> R {
    get(r).map(|s| f(&mut s.state.lock().unwrap())).unwrap_or_default()
}

fn with_collider<R: Default>(r: Ref, f: impl FnOnce(&mut physics::Collider) -> R) -> R {
    let Some(shape) = get(r) else { return R::default() };
    body::with_sim(shape.body, |sim, _| sim.shape_mut(shape.id).map(f).unwrap_or_default())
}

fn bbox((mins, maxs): (physics::Vector, physics::Vector)) -> BBox {
    BBox { mins: from_vec(mins), maxs: from_vec(maxs) }
}

fn add(body_ref: body::Ref, kind: Type, geometry: Geometry, settings: PhysicsShapeSettings) -> HandleId<class::IPhysicsShape> {
    let surface = Surface { friction: settings.friction, elasticity: settings.elasticity, trigger: settings.flags & SENSOR != 0 };
    let Some(id) = body::with_sim(body_ref, |sim, id| sim.add_shape(id, &geometry, surface)) else { return HandleId::new(-1) };
    let tags = settings.tags.iter().copied().take_while(|t| *t != 0).collect();
    let state = State { tags, collision_mask: settings.collision_function_mask as u8, ignore_traces: settings.flags & IGNORE_TRACES != 0, ..Default::default() };
    let shape = Arc::new(Shape { body: body_ref, id, kind, handle: Handle::default(), geometry: Mutex::new(geometry), state: Mutex::new(state) });
    let r: Ref = objects::create_shared(shape.clone());
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
    objects::destroy(r);
}

fn remove(body_ref: body::Ref, r: Ref) {
    let Some(shape) = get(r) else { return };
    body::with_sim(body_ref, |sim, _| sim.remove_shape(shape.id));
    if let Some(body) = body::get(body_ref) {
        body.shapes.lock().unwrap().retain(|x| *x != r);
    }
    forget(r);
}

fn update(r: Ref, geometry: Geometry) {
    let Some(shape) = get(r) else { return };
    body::with_sim(shape.body, |sim, _| sim.update_shape(shape.id, &geometry));
    *shape.geometry.lock().unwrap() = geometry;
}

/// Set or clear `tag`; whether that changed anything.
fn toggle_tag(r: Ref, tag: u32, on: bool) -> bool {
    with_state(r, |st| match (on, st.tags.iter().position(|t| *t == tag)) {
        (true, None) => {
            st.tags.push(tag);
            true
        }
        (false, Some(i)) => {
            st.tags.remove(i);
            true
        }
        _ => false,
    })
}

pub fn register(r: &mut Registry) {
    bind!(r, b::ADD_SPHERE_SHAPE, |body, center, radius, settings| {
        add(body, Type::Sphere, Geometry::Sphere { center: to_vec(center), radius }, settings)
    });
    bind!(r, b::ADD_CAPSULE_SHAPE, |body, a, c, radius, settings| {
        add(body, Type::Capsule, Geometry::Capsule { a: to_vec(a), b: to_vec(c), radius }, settings)
    });
    bind!(r, b::ADD_BOX_SHAPE, |body, center, rotation, half, settings| {
        add(body, Type::Hull, Geometry::Box { center: to_vec(center), rotation: to_rot(rotation), half_extents: to_vec(half) }, settings)
    });
    bind!(r, b::ADD_HULL_SHAPE, |body, position, rotation, points, settings| add(body, Type::Hull, hull(position, rotation, points), settings));
    // TODO: per-triangle surfaces (material count + surface indices in settings).
    bind!(r, b::ADD_MESH_SHAPE, |body, vertices, indices, _materials, settings| add(body, Type::Mesh, mesh(vertices, indices), settings));
    bind!(r, b::REMOVE_SHAPE, remove);
    bind!(r, b::PURGE_SHAPES, |body| {
        let shapes = body::get(body).map(|b| b.shapes.lock().unwrap().clone()).unwrap_or_default();
        shapes.into_iter().for_each(|s| remove(body, s));
    });
    bind!(r, b::GET_SHAPE_COUNT, |body| body::get(body).map_or(0, |b| b.shapes.lock().unwrap().len() as i32));
    bind!(r, b::GET_SHAPE, |body, i| {
        let shape = body::get(body).and_then(|b| usize::try_from(i).ok().and_then(|i| b.shapes.lock().unwrap().get(i).copied()));
        shape.and_then(get).map_or(HandleId::new(-1), |s| s.handle.id())
    });

    bind!(r, s::GET_BODY, |r| get(r).and_then(|s| body::get(s.body)).map_or(HandleId::new(-1), |b| b.handle.id()));
    bind!(r, s::MANAGED_OBJECT, |r| get(r).map_or(HandleId::new(-1), |s| s.handle.id()));
    bind!(r, s::GET_TYPE, |r| get(r).map_or(0, |s| s.kind as i64));
    bind!(r, s::HAS_TAG, |r, t| with_state(r, |st| st.tags.contains(&t)));
    bind!(r, s::ADD_TAG, |r, t| toggle_tag(r, t, true));
    bind!(r, s::REMOVE_TAG, |r, t| toggle_tag(r, t, false));
    bind!(r, s::CLEAR_TAGS, |r| with_state(r, |st| !std::mem::take(&mut st.tags).is_empty()));
    bind!(r, s::ADD_COLLISION_FUNCTION_MASK, |r, m| with_state(r, |st| st.collision_mask |= m));
    bind!(r, s::REMOVE_COLLISION_FUNCTION_MASK, |r, m| with_state(r, |st| st.collision_mask &= !m));
    bind!(r, s::GET_COLLISION_FUNCTION_MASK, |r| with_state(r, |st| st.collision_mask));
    bind!(r, s::SET_IGNORE_TRACES, |r, v| with_state(r, |st| st.ignore_traces = v));
    bind!(r, s::SET_LOCAL_VELOCITY, |r, v| with_state(r, |st| st.local_velocity = v));
    bind!(r, s::GET_LOCAL_VELOCITY, |r| with_state(r, |st| st.local_velocity));
    // TODO: surface properties (materials) once the surface-property controller exists.
    bind!(r, s::SET_MATERIAL_INDEX, |_, _| {});
    bind!(r, s::SET_SURFACE_INDEX, |_, _, _| {});
    bind!(r, s::GET_MATERIAL_NAME, |_| StrRet::from("default"));

    bind!(r, s::SET_TRIGGER, |r, v| with_collider(r, |c| c.set_sensor(v)));
    bind!(r, s::IS_TRIGGER, |r| with_collider(r, |c| c.is_sensor()));
    bind!(r, s::SET_FRICTION, |r, v| with_collider(r, |c| c.set_friction(v)));
    bind!(r, s::GET_FRICTION, |r| with_collider(r, |c| c.friction()));
    bind!(r, s::SET_ELASTICITY, |r, v| with_collider(r, |c| c.set_restitution(v)));
    bind!(r, s::SET_ROLLING_RESISTANCE, |_, _| {});
    bind!(r, s::SET_HAS_NO_MASS, |r, v| with_collider(r, |c| if v { c.set_density(0.0) }));
    bind!(r, s::BUILD_BOUNDS, |r| {
        let Some(shape) = get(r) else { return BBox::default() };
        body::with_sim(shape.body, |sim, _| sim.shape_bounds(shape.id).map(bbox)).unwrap_or_default()
    });
    bind!(r, s::LOCAL_BOUNDS, |r| with_collider(r, |c| bbox({
        let aabb = c.shared_shape().compute_local_aabb();
        (aabb.mins, aabb.maxs)
    })));
    bind!(r, s::RESET_PROXY, |_| {});

    bind!(r, s::UPDATE_SPHERE_SHAPE, |r, center, radius| update(r, Geometry::Sphere { center: to_vec(center), radius }));
    bind!(r, s::UPDATE_CAPSULE_SHAPE, |r, a, c, radius| update(r, Geometry::Capsule { a: to_vec(a), b: to_vec(c), radius }));
    bind!(r, s::UPDATE_BOX_SHAPE, |r, center, rotation, half| {
        update(r, Geometry::Box { center: to_vec(center), rotation: to_rot(rotation), half_extents: to_vec(half) })
    });
    bind!(r, s::UPDATE_HULL_SHAPE, |r, position, rotation, points| update(r, hull(position, rotation, points)));
    bind!(r, s::UPDATE_MESH_SHAPE, |r, vertices, indices| update(r, mesh(vertices, indices)));
    bind!(r, s::AS_SPHERE, |r| match get(r).map(|s| s.geometry.lock().unwrap().clone()) {
        Some(Geometry::Sphere { center, radius }) => Sphere { center: from_vec(center), radius },
        _ => Sphere::default(),
    });
    bind!(r, s::AS_CAPSULE, |r| match get(r).map(|s| s.geometry.lock().unwrap().clone()) {
        Some(Geometry::Capsule { a, b, radius }) => Capsule { center_a: from_vec(a), center_b: from_vec(b), radius },
        _ => Capsule::default(),
    });
}
