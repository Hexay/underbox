//! `IPhysicsShape`'s slots and `IPhysicsBody.Add*Shape`.

use super::body;
use super::shape::{Type, add, add_with_slots, get, remove, set_physical, set_surface, slot_surfaces, surface_name, toggle_tag, update, update_meta, with_collider, with_meta, with_sim, with_state};
use super::surfaces::by_hash;
use crate::math::{from_vec, to_rot, to_vec};
use crate::model::physics::part_slots::{hull_of, mesh_of};
use abi::slots::{i_physics_body as b, i_physics_shape as s};
use abi::structs::{BBox, Capsule, Sphere};
use abi::{HandleId, Registry, StrRet, bind};
use physics::Shape as Geometry;

fn hull(position: abi::structs::Vector3, rotation: abi::structs::Rotation, points: &[abi::structs::Vector3]) -> Geometry {
    Geometry::Hull { pose: physics::Pose::from_parts(to_vec(position), to_rot(rotation)), points: points.iter().copied().map(to_vec).collect() }
}

/// Triangles from a flat index list; a trailing partial triangle or a negative index is dropped.
fn mesh(vertices: &[abi::structs::Vector3], indices: &[i32]) -> Geometry {
    let triangles = indices.as_chunks::<3>().0.iter().filter_map(|t| Some([t[0].try_into().ok()?, t[1].try_into().ok()?, t[2].try_into().ok()?]));
    Geometry::Mesh { vertices: vertices.iter().copied().map(to_vec).collect(), triangles: triangles.collect() }
}

/// Physics-data points (an `RnHull_t`/`RnMesh_t` view) placed by `xform`, scale included.
fn placed(points: &[glam::Vec3], xform: abi::structs::Transform) -> Vec<physics::Vector> {
    let (pos, rot, scale) = (to_vec(xform.position), to_rot(xform.rotation), to_vec(xform.scale));
    points.iter().map(|p| pos + rot * (physics::Vector::new(p.x, p.y, p.z) * scale)).collect()
}

fn hull_points(points: Vec<physics::Vector>) -> Geometry {
    Geometry::Hull { pose: physics::Pose::from_parts(physics::Vector::ZERO, physics::Rotation::IDENTITY), points }
}

fn triangle_count(g: &Geometry) -> Option<usize> {
    match g {
        Geometry::Mesh { triangles, .. } => Some(triangles.len()),
        _ => None,
    }
}

fn bbox((mins, maxs): (physics::Vector, physics::Vector)) -> BBox {
    BBox { mins: from_vec(mins), maxs: from_vec(maxs) }
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
    // Raw meshes carry no per-triangle materials: every triangle is slot 0.
    bind!(r, b::ADD_MESH_SHAPE, |body, vertices, indices, _materials, settings| {
        let slots = slot_surfaces(&settings);
        let geometry = mesh(vertices, indices);
        let triangles = if slots.is_empty() { Vec::new() } else { triangle_count(&geometry).map_or(Vec::new(), |n| vec![0; n]) };
        add_with_slots(body, Type::Mesh, geometry, settings, triangles, slots)
    });
    bind!(r, b::ADD_HULL_SHAPE_1, |body, h, xform, settings| {
        let points = hull_of(h.as_raw(), |x| placed(&x.vertices, xform));
        add(body, Type::Hull, hull_points(points), settings)
    });
    bind!(r, b::ADD_HULL_SHAPE_2, |body, m, xform, settings| {
        let points = mesh_of(m.as_raw(), |x| placed(&x.vertices, xform));
        add(body, Type::Hull, hull_points(points), settings)
    });
    // Each triangle's material slot picks one of `settings`' slot surfaces (managed `CreationSurfaces`).
    bind!(r, b::ADD_MESH_SHAPE_1, |body, m, xform, _materials, settings| {
        let (vertices, triangles, materials) = mesh_of(m.as_raw(), |x| (placed(&x.vertices, xform), x.triangles.clone(), x.materials.clone()));
        add_with_slots(body, Type::Mesh, Geometry::Mesh { vertices, triangles }, settings, materials, slot_surfaces(&settings))
    });
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
    bind!(r, s::HAS_TAG, |r, t| with_meta(r, |m| m.tags.contains(&t)));
    bind!(r, s::ADD_TAG, |r, t| toggle_tag(r, t, true));
    bind!(r, s::REMOVE_TAG, |r, t| toggle_tag(r, t, false));
    bind!(r, s::CLEAR_TAGS, |r| update_meta(r, |m| !std::mem::take(&mut m.tags).is_empty()));
    bind!(r, s::ADD_COLLISION_FUNCTION_MASK, |r, mask| update_meta(r, |m| m.functions |= mask));
    bind!(r, s::REMOVE_COLLISION_FUNCTION_MASK, |r, mask| update_meta(r, |m| m.functions &= !mask));
    bind!(r, s::GET_COLLISION_FUNCTION_MASK, |r| with_meta(r, |m| m.functions));
    bind!(r, s::SET_IGNORE_TRACES, |r, v| with_state(r, |st| st.ignore_traces = v));
    bind!(r, s::SET_LOCAL_VELOCITY, |r, v| with_state(r, |st| st.local_velocity = v));
    bind!(r, s::GET_LOCAL_VELOCITY, |r| with_state(r, |st| st.local_velocity));
    bind!(r, s::SET_MATERIAL_INDEX, |r, name: abi::StringToken| set_surface(r, by_hash(name.0).unwrap_or(-1)));
    // Managed sets a mesh's per-slot surfaces (`PhysicsShape.Surfaces`); slot 0 is also the shape's own.
    bind!(r, s::SET_SURFACE_INDEX, |r, index, slot| with_state(r, |st| {
        if slot <= 0 {
            st.surface_index = index;
        }
        if let Ok(slot) = usize::try_from(slot) {
            st.set_slot_surface(slot, index);
        }
    }));
    bind!(r, s::GET_MATERIAL_NAME, |r| StrRet::from(surface_name(r)));

    bind!(r, s::SET_TRIGGER, |r, v| with_sim(r, |sim, id| sim.set_trigger(id, v)));
    bind!(r, s::IS_TRIGGER, |r| with_collider(r, |c| c.is_sensor()));
    bind!(r, s::SET_FRICTION, |r, v| set_physical(r, false, v));
    bind!(r, s::GET_FRICTION, |r| with_collider(r, |c| c.friction()));
    bind!(r, s::SET_ELASTICITY, |r, v| set_physical(r, true, v));
    bind!(r, s::SET_ROLLING_RESISTANCE, |_, _| {});
    bind!(r, s::SET_HAS_NO_MASS, |r, v| update_meta(r, |m| m.massless = v));
    bind!(r, s::IS_TOUCHING, |r, other, triggers| {
        let Some(other) = get(other) else { return false };
        with_sim(r, |sim, id| sim.is_touching(id, other.id, triggers))
    });
    bind!(r, s::BUILD_BOUNDS, |r| with_sim(r, |sim, id| sim.shape_bounds(id).map(bbox)).unwrap_or_default());
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
