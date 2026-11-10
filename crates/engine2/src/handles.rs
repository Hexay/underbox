//! Managed handles for `[Handle:...]` classes (scene worlds and objects, physics worlds, bodies,
//! shapes, joints). When native makes one it calls managed's `RegisterHandle`, which adopts the
//! wrapper waiting in `HandleIndex.nextObject` or builds one, and returns the int id slots hand
//! back; destroying it calls `FreeHandle`. Without this every wrapper reports `IsValid` false.

use abi::callbacks::sandbox_handle_index as index;
use abi::{HandleId, NativeRef, RawPtr, StringToken};
use std::sync::atomic::{AtomicI32, Ordering};

/// The type names managed's `HandleIndex.RegisterHandle` dispatches on (Core/Interop/Handle.cs, `Types`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    SceneWorld,
    SceneObject,
    SceneLightObject,
    SceneSkyBoxObject,
    SceneAnimatableObject,
    ManagedSceneObject,
    EnvMapSceneObject,
    SceneLightProbeVolumeObject,
    DecalSceneObject,
    PhysicsWorld,
    PhysicsBody,
    PhysicsShape,
    PhysicsAggregate,
    PhysicsJoint,
}

impl Kind {
    fn token(self) -> u32 {
        let name = match self {
            Kind::PhysicsAggregate => "PhysAggregate",
            other => return StringToken::of(&format!("{other:?}")).0,
        };
        StringToken::of(name).0
    }
}

/// The managed id of one native object; unset (-1) until registered.
#[derive(Debug)]
pub struct Handle(AtomicI32);

impl Default for Handle {
    fn default() -> Self {
        Self(AtomicI32::new(-1))
    }
}

impl Handle {
    /// Register `ptr` with managed as `kind`, remembering the id it gives back.
    pub fn register<C, D>(&self, ptr: NativeRef<C>, kind: Kind) -> HandleId<D> {
        let id = index::register_handle(RawPtr::from_raw(ptr.as_raw()), kind.token());
        self.0.store(id, Ordering::Relaxed);
        HandleId::new(id)
    }

    pub fn id<C>(&self) -> HandleId<C> {
        HandleId::new(self.0.load(Ordering::Relaxed))
    }

    /// Tell managed the object is gone; idempotent.
    pub fn free(&self) {
        let id = self.0.swap(-1, Ordering::Relaxed);
        if id >= 0 {
            index::free_handle(id);
        }
    }
}
