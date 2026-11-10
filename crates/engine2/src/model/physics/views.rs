//! The native objects managed reads compiled physics through: `VPhysXBodyPart_t` per part, and
//! per shape `Rn*Desc_t` plus the `RnHull_t`/`RnMesh_t` behind hull and mesh descs. Each is made
//! once and freed with its owner (see `PhysicsData`'s `Drop`).

use super::data::{Part, PhysicsData};
use abi::{NativeClass, NativeRef, class};
use std::ffi::c_void;
use std::sync::{Arc, OnceLock};

/// Objects made on first use and destroyed with their owner.
pub struct Refs<C>(OnceLock<Vec<NativeRef<C>>>);

impl<C> Default for Refs<C> {
    fn default() -> Self {
        Self(OnceLock::new())
    }
}

impl<C> Refs<C> {
    pub fn get(&self, i: usize, make: impl FnOnce() -> Vec<NativeRef<C>>) -> NativeRef<C> {
        self.0.get_or_init(make).get(i).copied().unwrap_or(NativeRef::NULL)
    }
}

impl<C> Drop for Refs<C> {
    fn drop(&mut self) {
        self.0.get().into_iter().flatten().for_each(|r| {
            objects::destroy(*r);
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Sphere,
    Capsule,
    Hull,
    Mesh,
}

/// A `VPhysXBodyPart_t`.
pub struct PartView {
    pub part: Arc<Part>,
    spheres: Refs<class::RnSphereDesc_t>,
    capsules: Refs<class::RnCapsuleDesc_t>,
    hulls: Refs<class::RnHullDesc_t>,
    meshes: Refs<class::RnMeshDesc_t>,
}

/// An `Rn*Desc_t`, or the `RnHull_t`/`RnMesh_t` inside one.
pub struct ShapeView {
    pub part: Arc<Part>,
    pub kind: Kind,
    pub index: usize,
    hull: Refs<class::RnHull_t>,
    mesh: Refs<class::RnMesh_t>,
}

fn views<C: NativeClass>(part: &Arc<Part>, kind: Kind, n: usize) -> Vec<NativeRef<C>> {
    (0..n).map(|index| objects::create(ShapeView { part: part.clone(), kind, index, hull: Refs::default(), mesh: Refs::default() })).collect()
}

impl PartView {
    pub fn sphere(&self, i: usize) -> NativeRef<class::RnSphereDesc_t> {
        self.spheres.get(i, || views(&self.part, Kind::Sphere, self.part.shapes.spheres.len()))
    }
    pub fn capsule(&self, i: usize) -> NativeRef<class::RnCapsuleDesc_t> {
        self.capsules.get(i, || views(&self.part, Kind::Capsule, self.part.shapes.capsules.len()))
    }
    pub fn hull(&self, i: usize) -> NativeRef<class::RnHullDesc_t> {
        self.hulls.get(i, || views(&self.part, Kind::Hull, self.part.shapes.hulls.len()))
    }
    pub fn mesh(&self, i: usize) -> NativeRef<class::RnMeshDesc_t> {
        self.meshes.get(i, || views(&self.part, Kind::Mesh, self.part.shapes.meshes.len()))
    }
}

impl ShapeView {
    /// The `RnHull_t` of a hull desc.
    pub fn hull_ref(&self) -> NativeRef<class::RnHull_t> {
        self.hull.get(0, || vec![objects::create(ShapeView { part: self.part.clone(), kind: Kind::Hull, index: self.index, hull: Refs::default(), mesh: Refs::default() })])
    }
    pub fn mesh_ref(&self) -> NativeRef<class::RnMesh_t> {
        self.mesh.get(0, || vec![objects::create(ShapeView { part: self.part.clone(), kind: Kind::Mesh, index: self.index, hull: Refs::default(), mesh: Refs::default() })])
    }
    pub fn ids(&self) -> super::data::Indices {
        let s = &self.part.shapes;
        let ids = match self.kind {
            Kind::Sphere => s.spheres.get(self.index).map(|x| x.ids),
            Kind::Capsule => s.capsules.get(self.index).map(|x| x.ids),
            Kind::Hull => s.hulls.get(self.index).map(|x| x.ids),
            Kind::Mesh => s.meshes.get(self.index).map(|x| x.ids),
        };
        ids.unwrap_or_default()
    }
}

impl PhysicsData {
    pub fn part_ref(&self, i: usize) -> NativeRef<class::VPhysXBodyPart_t> {
        let refs = self.part_refs.get_or_init(|| {
            let view = |p: &Arc<Part>| PartView { part: p.clone(), spheres: Refs::default(), capsules: Refs::default(), hulls: Refs::default(), meshes: Refs::default() };
            self.parts.iter().map(|p| objects::create(view(p))).collect()
        });
        refs.get(i).copied().unwrap_or_default()
    }

    pub fn joint_ref(&self, i: usize) -> NativeRef<class::VPhysXJoint_t> {
        let refs = self.joint_refs.get_or_init(|| self.joints.iter().map(|j| objects::create_shared(j.clone())).collect());
        refs.get(i).copied().unwrap_or_default()
    }
}

pub fn part<R: Default>(p: *mut c_void, f: impl FnOnce(&PartView) -> R) -> R {
    objects::get::<_, PartView>(NativeRef::<class::VPhysXBodyPart_t>::from_raw(p)).map(|v| f(&v)).unwrap_or_default()
}

pub fn shape<R: Default>(p: *mut c_void, f: impl FnOnce(&ShapeView) -> R) -> R {
    objects::get::<_, ShapeView>(NativeRef::<class::RnHull_t>::from_raw(p)).map(|v| f(&v)).unwrap_or_default()
}
