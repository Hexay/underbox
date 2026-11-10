//! `CPhysBodyDescArray` / `CPhysBodyDesc`: the bodies and joints managed describes for a model's
//! physics (CPhysBodyDescArray.Create.cs), snapshotted into [`PhysicsData`] by
//! `SetPhysics_Runtime` or `MeshGlue.BuildAggregateData`.

use super::data::{CapsuleDesc, HullDesc, JointDesc, MeshDesc, Part, PhysicsData, SharedJoint, SphereDesc};
use super::hull::Hull;
use abi::slots::{c_phys_body_desc as bd, c_phys_body_desc_array as arr};
use abi::structs::{Capsule, Sphere, Transform, Vector3};
use abi::{NativeRef, RawPtr, Registry, bind, class};
use crate::math::{quat, v3 as v};
use glam::Vec3;
use std::ffi::{CStr, c_void};
use std::sync::{Arc, Mutex};

pub type ArrayRef = NativeRef<class::CPhysBodyDescArray>;
type BodyRef = NativeRef<class::CPhysBodyDesc>;
pub type JointRef = NativeRef<class::VPhysXJoint_t>;

pub struct DescArray {
    bodies: Vec<BodyRef>,
    joints: Vec<JointRef>,
    surfaces: Mutex<Vec<i32>>,
}

/// `transform` as a point mapping: scale (a zero component counts as 1), rotate, translate.
fn mapping(t: Transform) -> impl Fn(Vec3) -> Vec3 {
    let s = v(t.scale);
    let s = Vec3::select(s.cmpeq(Vec3::ZERO), Vec3::ONE, s);
    let q = quat(t.rotation);
    let p = v(t.position);
    move |x| q * (x * s) + p
}

fn create(count: i32, joints: i32) -> ArrayRef {
    let bodies = (0..count.max(0)).map(|_| objects::create(Mutex::new(Part::default()))).collect();
    let joints = (0..joints.max(0)).map(|_| objects::create_shared(Arc::new(Mutex::new(JointDesc::default())))).collect();
    objects::create(DescArray { bodies, joints, surfaces: Mutex::default() })
}

fn delete(a: ArrayRef) {
    if let Some(d) = objects::get::<_, DescArray>(a) {
        d.bodies.iter().for_each(|b| {
            objects::destroy(*b);
        });
        d.joints.iter().for_each(|j| {
            objects::destroy(*j);
        });
    }
    objects::destroy(a);
}

fn nth<C: Copy + Default>(list: &[C], i: i32) -> C {
    usize::try_from(i).ok().and_then(|i| list.get(i).copied()).unwrap_or_default()
}

pub fn with_body<R: Default>(b: *mut c_void, f: impl FnOnce(&mut Part) -> R) -> R {
    objects::get::<_, Mutex<Part>>(BodyRef::from_raw(b)).map(|p| f(&mut p.lock().unwrap())).unwrap_or_default()
}

pub fn joint(j: JointRef) -> Option<SharedJoint> {
    objects::get::<_, Mutex<JointDesc>>(j)
}

/// The physics a desc array describes, deep-copied.
pub fn build(name: &str, a: ArrayRef) -> Option<Arc<PhysicsData>> {
    let d = objects::get::<_, DescArray>(a)?;
    let parts = d.bodies.iter().map(|b| with_body(b.as_raw(), |p| Some(p.clone())).unwrap_or_default()).collect();
    let joints = d.joints.iter().map(|j| joint(*j).map(|j| *j.lock().unwrap()).unwrap_or_default()).collect();
    Some(PhysicsData::new(name, parts, joints, d.surfaces.lock().unwrap().clone()))
}

fn points(ptr: RawPtr<c_void>, n: usize) -> Vec<Vec3> {
    // SAFETY: managed passes a pinned Vector3[n] (CPhysBodyDescArray.Create.cs).
    unsafe { ptr.cast::<Vector3>().as_slice(n) }.iter().copied().map(v).collect()
}

fn add_hull(b: *mut c_void, pts: RawPtr<c_void>, n: i32, t: Transform) {
    let Some(hull) = Hull::from_points(&points(pts, n.max(0) as usize)) else { return };
    let hull = hull.transformed(mapping(t));
    with_body(b, |p| p.shapes.hulls.push(HullDesc { hull, ids: Default::default() }));
}

fn add_box(b: *mut c_void, half: Vector3, t: Transform) {
    let h = v(half).abs();
    let corners: Vec<Vec3> = (0..8).map(|i| Vec3::new(if i & 1 != 0 { h.x } else { -h.x }, if i & 2 != 0 { h.y } else { -h.y }, if i & 4 != 0 { h.z } else { -h.z })).collect();
    let Some(hull) = Hull::from_points(&corners) else { return };
    let hull = hull.transformed(mapping(t));
    with_body(b, |p| p.shapes.hulls.push(HullDesc { hull, ids: Default::default() }));
}

fn add_mesh(b: *mut c_void, vertices: RawPtr<c_void>, nv: u32, indices: RawPtr<c_void>, ni: u32, materials: RawPtr<c_void>) {
    let vertices = points(vertices, nv as usize);
    // SAFETY: managed passes pinned uint[ni] and, unless null, byte[ni / 3].
    let (indices, materials) = unsafe { (indices.cast::<[u32; 3]>().as_slice(ni as usize / 3), materials.cast::<u8>().as_slice(ni as usize / 3)) };
    let triangles: Vec<[u32; 3]> = indices.iter().filter(|t| t.iter().all(|&i| i < nv)).copied().collect();
    let materials = if triangles.len() == indices.len() { materials.to_vec() } else { Vec::new() };
    with_body(b, |p| p.shapes.meshes.push(MeshDesc { vertices, triangles, materials, ids: Default::default() }));
}

pub fn register(r: &mut Registry) {
    bind!(r, arr::CREATE, create);
    bind!(r, arr::DELETE_THIS, delete);
    bind!(r, arr::GET, |a, i| objects::get::<_, DescArray>(a).map(|d| nth(&d.bodies, i)).unwrap_or_default());
    bind!(r, arr::GET_JOINT, |a, i| objects::get::<_, DescArray>(a).map(|d| nth(&d.joints, i)).unwrap_or_default());
    bind!(r, arr::ADD_SURFACE, |a, s| objects::get::<_, DescArray>(a).into_iter().for_each(|d| d.surfaces.lock().unwrap().push(s)));

    bind!(r, bd::GET_M_FL_MASS, |b| with_body(b, |p| p.mass));
    bind!(r, bd::SET_M_FL_MASS, |b, x| with_body(b, |p| p.mass = x));
    bind!(r, bd::GET_M_FL_INERTIA_SCALE, |b| with_body(b, |p| p.inertia_scale));
    bind!(r, bd::SET_M_FL_INERTIA_SCALE, |b, x| with_body(b, |p| p.inertia_scale = x));
    bind!(r, bd::GET_M_FL_LINEAR_DAMPING, |b| with_body(b, |p| p.linear_damping));
    bind!(r, bd::SET_M_FL_LINEAR_DAMPING, |b, x| with_body(b, |p| p.linear_damping = x));
    bind!(r, bd::GET_M_FL_ANGULAR_DAMPING, |b| with_body(b, |p| p.angular_damping));
    bind!(r, bd::SET_M_FL_ANGULAR_DAMPING, |b, x| with_body(b, |p| p.angular_damping = x));
    bind!(r, bd::SET_BONE_NAME, |b: BodyRef, name: Option<&CStr>| with_body(b.as_raw(), |p| p.bone = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    bind!(r, bd::SET_SURFACE, |b: BodyRef, s| with_body(b.as_raw(), |p| p.surface = Some(s.0)));
    bind!(r, bd::SET_BIND_POSE, |b: BodyRef, t| with_body(b.as_raw(), |p| p.bind = t));
    bind!(r, bd::ADD_SPHERE, |b: BodyRef, s: Sphere| with_body(b.as_raw(), |p| p.shapes.spheres.push(SphereDesc { center: v(s.center), radius: s.radius, ids: Default::default() })));
    bind!(r, bd::ADD_CAPSULE, |b: BodyRef, c: Capsule| {
        with_body(b.as_raw(), |p| p.shapes.capsules.push(CapsuleDesc { a: v(c.center_a), b: v(c.center_b), radius: c.radius, ids: Default::default() }))
    });
    bind!(r, bd::ADD_BOX, |b: BodyRef, half, t| add_box(b.as_raw(), half, t));
    bind!(r, bd::ADD_HULL, |b: BodyRef, pts, n, t| add_hull(b.as_raw(), pts, n, t));
    // TODO: hull simplification (precision, face/edge/vertex caps); the exact hull is kept.
    bind!(r, bd::ADD_HULL_1, |b: BodyRef, pts, n, t, _, _, _, _, _, _| add_hull(b.as_raw(), pts, n, t));
    bind!(r, bd::ADD_MESH, |b: BodyRef, vs, nv, is, ni, mats| add_mesh(b.as_raw(), vs, nv, is, ni, mats));
}
