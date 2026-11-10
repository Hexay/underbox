//! Strong resource handles (`[ResourceHandle:HMaterial]` classes and the like). Managed holds each
//! handle as its own pointer; copies share one resource, and destroying a handle drops only it.

use abi::{Guid, NativeClass, NativeRef, RawPtr};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, LazyLock, Mutex};

struct Handle<T> {
    resource: Arc<T>,
}

/// A new strong handle to `resource`.
pub fn new_handle<C: NativeClass, T: Send + Sync + 'static>(resource: Arc<T>) -> NativeRef<C> {
    objects::create::<C, _>(Handle { resource })
}

/// The resource a handle holds, if the handle is alive.
pub fn resolve<C, T: Send + Sync + 'static>(handle: NativeRef<C>) -> Option<Arc<T>> {
    objects::get::<C, Handle<T>>(handle).map(|h| h.resource.clone())
}

pub fn copy_handle<C: NativeClass, T: Send + Sync + 'static>(handle: NativeRef<C>) -> NativeRef<C> {
    resolve::<C, T>(handle).map_or(NativeRef::NULL, new_handle::<C, T>)
}

pub fn destroy_handle<C>(handle: NativeRef<C>) {
    objects::destroy(handle);
}

/// Identifies the resource, shared by all its handles (native's binding pointer).
pub fn binding_ptr<C, T: Send + Sync + 'static>(handle: NativeRef<C>) -> RawPtr<c_void> {
    resolve::<C, T>(handle).map_or(RawPtr::NULL, |r| RawPtr::from_raw(Arc::as_ptr(&r) as *mut c_void))
}

/// Resources by path, so loading one path twice gives one resource (managed caches wrappers by
/// binding pointer). Entries stay resident; TODO: unload unreferenced ones.
pub struct ByName<T>(LazyLock<Mutex<HashMap<String, Arc<T>>>>);

impl<T> ByName<T> {
    pub const fn new() -> Self {
        Self(LazyLock::new(Mutex::default))
    }

    pub fn find_or_insert(&self, name: &str, make: impl FnOnce() -> T) -> Arc<T> {
        self.0.lock().unwrap().entry(name.to_ascii_lowercase()).or_insert_with(|| Arc::new(make())).clone()
    }
}

impl<T> Default for ByName<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// What every resource type answers about itself through its handles.
pub trait Resource: Send + Sync + 'static {
    fn is_error(&self) -> bool {
        false
    }
    fn is_loaded(&self) -> bool {
        true
    }
    fn guid(&self) -> Guid {
        Guid::default()
    }
}

/// Bind the handle-management slots InteropGen adds to a resource-handle class
/// (`DestroyStrongHandle`, `IsStrongHandleValid`, `IsError`, `IsStrongHandleLoaded`,
/// `CopyStrongHandle`, `GetBindingPtr`) for resource type `$ty` in slot module `$m`.
#[macro_export]
macro_rules! bind_resource_handle {
    ($r:expr, $m:path, $ty:ty) => {{
        use $crate::resource::{self, Resource};
        use $m as m;
        abi::bind!($r, m::DESTROY_STRONG_HANDLE, resource::destroy_handle);
        abi::bind!($r, m::IS_STRONG_HANDLE_VALID, |h| resource::resolve::<_, $ty>(h).is_some());
        abi::bind!($r, m::IS_ERROR, |h| resource::resolve::<_, $ty>(h).is_none_or(|r| r.is_error()));
        abi::bind!($r, m::IS_STRONG_HANDLE_LOADED, |h| resource::resolve::<_, $ty>(h).is_some_and(|r| r.is_loaded()));
        abi::bind!($r, m::COPY_STRONG_HANDLE, resource::copy_handle::<_, $ty>);
        abi::bind!($r, m::GET_BINDING_PTR, resource::binding_ptr::<_, $ty>);
        abi::bind!($r, m::GET_GUID, |h| resource::resolve::<_, $ty>(h).map(|r| r.guid()).unwrap_or_default());
    }};
}
