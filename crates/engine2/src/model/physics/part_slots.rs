//! Slots on the part and shape views: `VPhysXBodyPart_t`, `Rn*Desc_t`, `RnHull_t`, `RnMesh_t`.
//! Compiled physics is read-only here: field setters are accepted and ignored.

use super::data::{CapsuleDesc, MeshDesc, Part, SphereDesc};
use super::hull::Hull;
use super::views::{Kind, ShapeView, part, shape};
use crate::containers;
use crate::math::from_v3 as out;
use crate::mesh::render_mesh::bounds_of;
use abi::slots::{rn_capsule_desc_t as cd, rn_hull_desc_t as hd, rn_hull_t as h, rn_mesh_desc_t as md, rn_mesh_t as m, rn_sphere_desc_t as sd, v_phys_x_body_part_t as bp};
use abi::structs::{BBox, Capsule, Sphere, Vector3};
use abi::{NativeRef, Registry, bind, class};
use glam::Vec3;
use std::f32::consts::TAU;

fn idx(i: i32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

fn sphere_of(s: &ShapeView) -> Option<&SphereDesc> {
    (s.kind == Kind::Sphere).then(|| s.part.shapes.spheres.get(s.index)).flatten()
}

fn capsule_of(s: &ShapeView) -> Option<&CapsuleDesc> {
    (s.kind == Kind::Capsule).then(|| s.part.shapes.capsules.get(s.index)).flatten()
}

fn hull_of<R: Default>(p: *mut std::ffi::c_void, f: impl FnOnce(&Hull) -> R) -> R {
    shape(p, |s| (s.kind == Kind::Hull).then(|| s.part.shapes.hulls.get(s.index).map(|x| f(&x.hull))).flatten().unwrap_or_default())
}

fn mesh_of<R: Default>(p: *mut std::ffi::c_void, f: impl FnOnce(&MeshDesc) -> R) -> R {
    shape(p, |s| (s.kind == Kind::Mesh).then(|| s.part.shapes.meshes.get(s.index).map(f)).flatten().unwrap_or_default())
}

fn bbox(points: &[Vec3]) -> BBox {
    bounds_of(points.iter().copied())
}

/// Three great circles around `center` (one per axis plane) as a line list.
fn outline(centers: &[Vec3], radius: f32) -> (Vec<Vec3>, Vec<u32>) {
    const SEGMENTS: u32 = 16;
    let (mut v, mut i) = (Vec::new(), Vec::new());
    for c in centers {
        for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z)] {
            let base = v.len() as u32;
            v.extend((0..SEGMENTS).map(|k| {
                let t = k as f32 / SEGMENTS as f32 * TAU;
                *c + (a * t.cos() + b * t.sin()) * radius
            }));
            i.extend((0..SEGMENTS).flat_map(|k| [base + k, base + (k + 1) % SEGMENTS]));
        }
    }
    if let [a, b] = centers {
        let base = v.len() as u32;
        for d in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y] {
            v.extend([*a + d * radius, *b + d * radius]);
        }
        i.extend((0..4).flat_map(|k| [base + k * 2, base + k * 2 + 1]));
    }
    (v, i)
}

fn write_outline(r: NativeRef<class::CUtlVector_Vector_>, ri: NativeRef<class::CUtlVector_uint32_>, (v, i): (Vec<Vec3>, Vec<u32>)) {
    let base = containers::with(r, |o: &mut Vec<Vector3>| {
        let base = o.len() as u32;
        o.extend(v.into_iter().map(out));
        base
    });
    containers::with(ri, |o: &mut Vec<u32>| o.extend(i.into_iter().map(|x| x + base)));
}

macro_rules! part_fields {
    ($r:expr, $($get:ident, $set:ident => $f:expr;)*) => {
        $(
            bind!($r, bp::$get, |p| part(p, |v| { let p: &Part = &v.part; $f(p) }));
            bind!($r, bp::$set, |_, _| {});
        )*
    };
}

macro_rules! shape_ids {
    ($r:expr, $($m:ident),*) => {
        $(
            bind!($r, $m::GET_M_N_COLLISION_ATTRIBUTE_INDEX, |p| shape(p, |s| s.ids().attribute));
            bind!($r, $m::SET_M_N_COLLISION_ATTRIBUTE_INDEX, |_, _| {});
            bind!($r, $m::GET_M_N_SURFACE_PROPERTY_INDEX, |p| shape(p, |s| s.ids().surface));
            bind!($r, $m::SET_M_N_SURFACE_PROPERTY_INDEX, |_, _| {});
        )*
    };
}

pub fn register(r: &mut Registry) {
    part_fields! { r,
        GET_M_N_FLAGS, SET_M_N_FLAGS => |p: &Part| p.flags;
        GET_M_FL_MASS, SET_M_FL_MASS => |p: &Part| p.mass;
        GET_M_N_COLLISION_ATTRIBUTE_INDEX, SET_M_N_COLLISION_ATTRIBUTE_INDEX => |p: &Part| p.collision_attribute;
        GET_M_FL_INERTIA_SCALE, SET_M_FL_INERTIA_SCALE => |p: &Part| p.inertia_scale;
        GET_M_FL_LINEAR_DAMPING, SET_M_FL_LINEAR_DAMPING => |p: &Part| p.linear_damping;
        GET_M_FL_ANGULAR_DAMPING, SET_M_FL_ANGULAR_DAMPING => |p: &Part| p.angular_damping;
        GET_M_FL_GRAVITY_SCALE, SET_M_FL_GRAVITY_SCALE => |p: &Part| p.gravity_scale;
        GET_M_B_OVERRIDE_MASS_CENTER, SET_M_B_OVERRIDE_MASS_CENTER => |p: &Part| p.override_mass_center;
        GET_M_V_MASS_CENTER_OVERRIDE, SET_M_V_MASS_CENTER_OVERRIDE => |p: &Part| p.mass_center;
    }
    bind!(r, bp::GET_SPHERE_COUNT, |v| part(v.as_raw(), |v| v.part.shapes.spheres.len() as i32));
    bind!(r, bp::GET_CAPSULE_COUNT, |v| part(v.as_raw(), |v| v.part.shapes.capsules.len() as i32));
    bind!(r, bp::GET_HULL_COUNT, |v| part(v.as_raw(), |v| v.part.shapes.hulls.len() as i32));
    bind!(r, bp::GET_MESH_COUNT, |v| part(v.as_raw(), |v| v.part.shapes.meshes.len() as i32));
    bind!(r, bp::GET_SPHERE, |v, i| part(v.as_raw(), |v| v.sphere(idx(i))));
    bind!(r, bp::GET_CAPSULE, |v, i| part(v.as_raw(), |v| v.capsule(idx(i))));
    bind!(r, bp::GET_HULL, |v, i| part(v.as_raw(), |v| v.hull(idx(i))));
    bind!(r, bp::GET_MESH, |v, i| part(v.as_raw(), |v| v.mesh(idx(i))));
    // Every shape uses its part's one attribute.
    bind!(r, bp::GET_COLLISION_ATTRIBUTE_COUNT, |v| part(v.as_raw(), |_| 1));
    bind!(r, bp::GET_COLLISION_ATTRIBUTE_INDEX, |v, _| part(v.as_raw(), |v| v.part.collision_attribute));

    shape_ids!(r, sd, cd, hd, md);
    bind!(r, sd::GET_M_SPHERE, |p| shape(p, |s| sphere_of(s).map(|x| Sphere { center: out(x.center), radius: x.radius }).unwrap_or_default()));
    bind!(r, sd::SET_M_SPHERE, |_, _| {});
    bind!(r, cd::GET_M_CAPSULE, |p| shape(p, |s| capsule_of(s).map(|x| Capsule { center_a: out(x.a), center_b: out(x.b), radius: x.radius }).unwrap_or_default()));
    bind!(r, cd::SET_M_CAPSULE, |_, _| {});
    bind!(r, sd::GET_OUTLINE, |s, vs, is| write_outline(vs, is, shape(s.as_raw(), |s| sphere_of(s).map(|x| outline(&[x.center], x.radius)).unwrap_or_default())));
    bind!(r, cd::GET_OUTLINE, |s, vs, is| write_outline(vs, is, shape(s.as_raw(), |s| capsule_of(s).map(|x| outline(&[x.a, x.b], x.radius)).unwrap_or_default())));
    bind!(r, hd::GET_HULL, |s| shape(s.as_raw(), ShapeView::hull_ref));
    bind!(r, md::GET_MESH, |s| shape(s.as_raw(), ShapeView::mesh_ref));

    bind!(r, h::GET_VERTEX_COUNT, |x| hull_of(x.as_raw(), |h| h.vertices.len() as i32));
    bind!(r, h::GET_EDGE_COUNT, |x| hull_of(x.as_raw(), |h| h.edges.len() as i32));
    bind!(r, h::GET_VOLUME, |x| hull_of(x.as_raw(), Hull::volume));
    bind!(r, h::GET_MASS_CENTER, |x| hull_of(x.as_raw(), |h| out(h.mass_center())));
    bind!(r, h::GET_CENTROID, |x| hull_of(x.as_raw(), |h| out(h.centroid())));
    bind!(r, h::GET_SURFACE_AREA, |x| hull_of(x.as_raw(), Hull::surface_area));
    bind!(r, h::GET_MEMORY, |x| hull_of(x.as_raw(), |h| (h.vertices.len() * 12 + h.triangles.len() * 12 + h.edges.len() * 8) as i32));
    bind!(r, h::GET_BBOX, |x| hull_of(x.as_raw(), |h| bbox(&h.vertices)));
    bind!(r, h::GET_VERTEX, |x, i| hull_of(x.as_raw(), |h| h.vertices.get(idx(i)).map_or(Vector3::default(), |p| out(*p))));
    bind!(r, h::GET_EDGE_VERTEX, |x, i, a, b| {
        (*a, *b) = hull_of(x.as_raw(), |h| h.edges.get(idx(i)).map(|e| (out(h.vertices[e[0] as usize]), out(h.vertices[e[1] as usize]))).unwrap_or_default());
    });

    bind!(r, m::GET_TRIANGLE_COUNT, |x| mesh_of(x.as_raw(), |m| m.triangles.len() as i32));
    bind!(r, m::GET_VERTEX_COUNT, |x| mesh_of(x.as_raw(), |m| m.vertices.len() as i32));
    bind!(r, m::GET_VERTEX, |x, i| mesh_of(x.as_raw(), |m| m.vertices.get(idx(i)).map_or(Vector3::default(), |p| out(*p))));
    // Depth of the mesh's BVH in native; ours has none.
    bind!(r, m::GET_HEIGHT, |_| 0);
    bind!(r, m::GET_MEMORY, |x| mesh_of(x.as_raw(), |m| (m.vertices.len() * 12 + m.triangles.len() * 13) as i32));
    bind!(r, m::GET_BBOX, |x| mesh_of(x.as_raw(), |m| bbox(&m.vertices)));
    bind!(r, m::GET_MATERIAL_COUNT, |x| mesh_of(x.as_raw(), |m| m.materials.len() as i32));
    bind!(r, m::GET_TRIANGLE_MATERIAL, |x, t| mesh_of(x.as_raw(), |m| m.materials.get(idx(t)).map_or(0, |&s| i32::from(s))));
    bind!(r, m::GET_TRIANGLE_INDICES, |x, t, a, b, c| {
        let tri = mesh_of(x.as_raw(), |m| m.triangles.get(idx(t)).copied().unwrap_or_default());
        (*a, *b, *c) = (tri[0] as i32, tri[1] as i32, tri[2] as i32);
    });
    bind!(r, m::GET_TRIANGLE, |x, t, a, b, c| {
        let p = mesh_of(x.as_raw(), |m| m.triangles.get(idx(t)).map(|tri| tri.map(|i| out(m.vertices.get(i as usize).copied().unwrap_or_default()))).unwrap_or_default());
        (*a, *b, *c) = (p[0], p[1], p[2]);
    });
}
