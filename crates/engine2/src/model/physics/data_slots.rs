//! `CPhysicsData` handles and queries (PhysicsGroupDescription.cs), and the slots that hand
//! physics data out: `CModel::GetPhysicsContainer` and `MeshGlue.BuildAggregateData`.

use super::data::PhysicsData;
use super::desc;
use crate::math::{quat, v3};
use crate::mesh::render_mesh::bounds_of;
use crate::model;
use crate::resource::{self, Resource};
use abi::slots::{c_model as cm, c_physics_data as pd, mesh_glue as g};
use abi::structs::BBox;
use abi::{NativeRef, Registry, StrRet, StringToken, bind, class};
use glam::Vec3;
use std::sync::Arc;

pub type Ref = NativeRef<class::CPhysicsData>;

impl Resource for PhysicsData {}

pub fn get(r: Ref) -> Option<Arc<PhysicsData>> {
    resource::resolve::<_, PhysicsData>(r)
}

pub fn handle(d: Option<&Arc<PhysicsData>>) -> Ref {
    d.map_or(Ref::NULL, |d| resource::new_handle(d.clone()))
}

fn on<R: Default>(r: Ref, f: impl FnOnce(&PhysicsData) -> R) -> R {
    get(r).map(|d| f(&d)).unwrap_or_default()
}

fn idx(i: i32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

/// Model-space bounds of every shape at the parts' bind poses; zero without shapes.
pub fn bounds(d: &PhysicsData) -> BBox {
    let mut points = Vec::new();
    for p in &d.parts {
        let (q, origin) = (quat(p.bind.rotation), v3(p.bind.position));
        let at = |x: Vec3| q * x + origin;
        let s = &p.shapes;
        let ball = |c: Vec3, r: f32| [c - Vec3::splat(r), c + Vec3::splat(r)];
        points.extend(s.spheres.iter().flat_map(|x| ball(x.center, x.radius)).map(at));
        points.extend(s.capsules.iter().flat_map(|x| ball(x.a, x.radius).into_iter().chain(ball(x.b, x.radius))).map(at));
        points.extend(s.hulls.iter().flat_map(|x| x.hull.vertices.iter().copied()).map(at));
        points.extend(s.meshes.iter().flat_map(|x| x.vertices.iter().copied()).map(at));
    }
    bounds_of(points.into_iter())
}

fn checksum(d: &PhysicsData) -> u32 {
    let joints: Vec<_> = (0..d.joints.len()).filter_map(|i| d.joint(i)).collect();
    StringToken::of(&format!("{:?}{:?}{:?}", d.parts, joints, d.surfaces)).0
}

fn physics_of(m: model::Ref) -> Option<Arc<PhysicsData>> {
    model::get(m).and_then(|m| m.content.physics.clone())
}

pub fn register(r: &mut Registry) {
    crate::bind_resource_handle!(r, abi::slots::c_physics_data, PhysicsData);
    bind!(r, pd::GET_RESOURCE_NAME, |r| StrRet::from(get(r).map(|d| d.name.clone())));
    bind!(r, pd::GET_CHECKSUM, |r| on(r, checksum));
    bind!(r, pd::GET_BONE_COUNT, |r| on(r, |d| if d.has_bones() { d.parts.len() as i32 } else { 0 }));
    bind!(r, pd::GET_BONE_NAME, |r, i| StrRet::from(on(r, |d| d.parts.get(idx(i)).map(|p| p.bone.clone()))));
    bind!(r, pd::GET_BONE_HASH, |r, i| on(r, |d| d.parts.get(idx(i)).map_or(0, |p| StringToken::of(&p.bone).0)));
    bind!(r, pd::GET_BIND_POSE, |r, i| on(r, |d| d.parts.get(idx(i)).map_or_else(super::data::identity, |p| p.bind)));
    bind!(r, pd::GET_PART_COUNT, |r| on(r, |d| d.parts.len() as i32));
    bind!(r, pd::GET_PART, |r, i| on(r, |d| d.part_ref(idx(i))));
    bind!(r, pd::GET_JOINT_COUNT, |r| on(r, |d| d.joints.len() as i32));
    bind!(r, pd::GET_JOINT, |r, i| on(r, |d| d.joint_ref(idx(i))));
    bind!(r, pd::GET_SURFACE_PROPERTIES_COUNT, |r| on(r, |d| d.surfaces.len() as i32));
    // TODO: surface objects need g_pPhysicsSystem's surface property controller; managed reads
    // a null one as "default".
    bind!(r, pd::GET_SURFACE_PROPERTIES, |_, _| NativeRef::NULL);
    bind!(r, pd::GET_COLLISION_ATTRIBUTE_COUNT, |r| on(r, |d| d.attributes.len() as i32));
    bind!(r, pd::GET_TAG_COUNT, |r, a| on(r, |d| d.attributes.get(idx(a)).map_or(0, |t| t.len() as i32)));
    bind!(r, pd::GET_TAG, |r, a, i| StrRet::from(on(r, |d| d.attributes.get(idx(a)).and_then(|t| t.get(idx(i)).cloned()))));

    bind!(r, cm::GET_PHYSICS_CONTAINER, |m| handle(physics_of(m).as_ref()));
    bind!(r, cm::HAS_PHYSICS, |m| physics_of(m).is_some());
    bind!(r, cm::GET_PHYSICS_BOUNDS, |m| physics_of(m).map(|d| bounds(&d)).unwrap_or_default());
    bind!(r, g::BUILD_AGGREGATE_DATA, |a| handle(desc::build("", a).as_ref()));
}
