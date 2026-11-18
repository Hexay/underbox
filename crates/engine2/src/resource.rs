//! Strong resource handles (`[ResourceHandle:HMaterial]` classes and the like). Managed holds each
//! handle as its own pointer; copies share one resource, and destroying a handle drops only it.

use abi::{Guid, NativeClass, NativeRef, RawPtr};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
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

struct Empty;

/// A strong handle to nothing, which native getters return for a missing resource: managed's
/// `FromNative` wrappers call `IsStrongHandleValid` first, which throws on a null pointer.
pub fn empty_handle<C: NativeClass>() -> NativeRef<C> {
    objects::create::<C, _>(Empty)
}

/// A copy for managed to own, which is an empty handle where `handle` holds nothing.
pub fn handed_out<C: NativeClass, T: Send + Sync + 'static>(handle: Option<NativeRef<C>>) -> NativeRef<C> {
    handle.and_then(resolve::<C, T>).map_or_else(empty_handle, new_handle::<C, T>)
}

pub fn destroy_handle<C>(handle: NativeRef<C>) {
    objects::destroy(handle);
}

/// Identifies the resource, shared by all its handles (native's binding pointer).
pub fn binding_ptr<C, T: Send + Sync + 'static>(handle: NativeRef<C>) -> RawPtr<c_void> {
    resolve::<C, T>(handle).map_or(RawPtr::NULL, |r| RawPtr::from_raw(Arc::as_ptr(&r) as *mut c_void))
}

/// Resources by path, so loading one path twice gives one resource (managed caches wrappers by
/// binding pointer). As real's resource system, a resource nothing references any more (no
/// strong handle, no engine object holding it) is deleted on the next frame update
/// ([`unload_unreferenced`]); loading its path again reads it afresh. Entries the engine looks up
/// by name instead of holding ([`ByName::find_or_insert_resident`]) are never unloaded.
pub struct ByName<T: Send + Sync + 'static> {
    map: LazyLock<Mutex<HashMap<String, Entry<T>>>>,
    swept: AtomicBool,
}

struct Entry<T> {
    resource: Arc<T>,
    resident: bool,
}

trait Sweep: Sync {
    fn sweep(&self) -> usize;
}

static CACHES: Mutex<Vec<&'static dyn Sweep>> = Mutex::new(Vec::new());

impl<T: Send + Sync + 'static> ByName<T> {
    pub const fn new() -> Self {
        Self { map: LazyLock::new(Mutex::default), swept: AtomicBool::new(false) }
    }

    fn map(&'static self) -> std::sync::MutexGuard<'static, HashMap<String, Entry<T>>> {
        if !self.swept.swap(true, Ordering::Relaxed) {
            CACHES.lock().unwrap().push(self);
        }
        self.map.lock().unwrap()
    }

    fn entry(&'static self, name: &str, resident: bool, make: impl FnOnce() -> T) -> Arc<T> {
        let mut map = self.map();
        let entry = map.entry(name.to_ascii_lowercase()).or_insert_with(|| Entry { resource: Arc::new(make()), resident });
        entry.resident |= resident;
        entry.resource.clone()
    }

    pub fn find_or_insert(&'static self, name: &str, make: impl FnOnce() -> T) -> Arc<T> {
        self.entry(name, false, make)
    }

    /// As [`Self::find_or_insert`], and keeps the resource loaded from now on: for callers that
    /// look it up by name each time and key their own caches on its identity.
    pub fn find_or_insert_resident(&'static self, name: &str, make: impl FnOnce() -> T) -> Arc<T> {
        self.entry(name, true, make)
    }

    /// Make `resource` the one `name` finds from now on.
    pub fn insert(&'static self, name: &str, resource: Arc<T>) {
        self.map().insert(name.to_ascii_lowercase(), Entry { resource, resident: false });
    }
}

impl<T: Send + Sync + 'static> Default for ByName<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + Sync + 'static> Sweep for ByName<T> {
    fn sweep(&self) -> usize {
        // Dropped unlocked: a resource's drop may release others of its type.
        let unreferenced = |_: &String, e: &mut Entry<T>| !e.resident && Arc::strong_count(&e.resource) == 1;
        let gone: Vec<Entry<T>> = self.map.lock().unwrap().extract_if(unreferenced).map(|(_, e)| e).collect();
        gone.len()
    }
}

/// Drop every cached resource only its cache still holds; how many went.
pub fn unload_unreferenced() -> usize {
    let caches = CACHES.lock().unwrap().clone();
    caches.iter().map(|c| c.sweep()).sum()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static LOADED: ByName<usize> = ByName::new();
    static LOADS: AtomicUsize = AtomicUsize::new(0);

    fn load() -> NativeRef<abi::class::CModel> {
        new_handle(LOADED.find_or_insert("Res", || LOADS.fetch_add(1, Ordering::Relaxed)))
    }

    /// Resident while any handle holds it; the frame after the last one goes, a load reads it afresh.
    #[test]
    fn unreferenced_resources_unload() {
        let (a, b) = (load(), load());
        assert_eq!(LOADS.load(Ordering::Relaxed), 1);
        assert_eq!(binding_ptr::<_, usize>(a).as_ptr(), binding_ptr::<_, usize>(b).as_ptr());
        destroy_handle(a);
        assert_eq!(LOADED.sweep(), 0, "b still holds it");
        destroy_handle(b);
        assert_eq!(LOADED.sweep(), 1);
        let c = load();
        assert_eq!(LOADS.load(Ordering::Relaxed), 2, "loaded afresh");
        destroy_handle(c);
    }

    static ENGINE_HELD: ByName<usize> = ByName::new();

    #[test]
    fn resident_resources_stay_loaded() {
        let first = ENGINE_HELD.find_or_insert_resident("Res", || 1);
        drop(first);
        assert_eq!(ENGINE_HELD.sweep(), 0);
        assert_eq!(*ENGINE_HELD.find_or_insert("res", || 2), 1, "same resource, not reloaded");
    }
}
