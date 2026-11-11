//! Native objects. Managed holds each as an opaque pointer (`NativeRef<C>`); that pointer is the
//! address of a small token we allocate, so it's unique while the object lives and never
//! dereferenced. The object itself is an `Arc<T>` found by that address, so a slot can use it
//! without holding the registry's lock.

use abi::{NativeClass, NativeRef};
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

struct Entry {
    class: &'static str,
    value: Arc<dyn Any + Send + Sync>,
}

// A non-zero-sized allocation, so every live object has its own address.
#[allow(dead_code)]
struct Token(u64);

static OBJECTS: LazyLock<RwLock<HashMap<usize, Entry>>> = LazyLock::new(|| {
    abi::rt::casts::set_class_of(class_of);
    RwLock::new(HashMap::new())
});

/// Make an object of native class `C` holding `value`.
pub fn create<C: NativeClass, T: Any + Send + Sync>(value: T) -> NativeRef<C> {
    create_shared::<C, T>(Arc::new(value))
}

/// As [`create`], for a value other owners hold too.
pub fn create_shared<C: NativeClass, T: Any + Send + Sync>(value: Arc<T>) -> NativeRef<C> {
    let addr = Box::into_raw(Box::new(Token(0))) as usize;
    OBJECTS.write().unwrap().insert(addr, Entry { class: C::NAME, value });
    NativeRef::from_raw(addr as *mut _)
}

/// The object at `r`, if it's alive and holds a `T`.
pub fn get<C, T: Any + Send + Sync>(r: NativeRef<C>) -> Option<Arc<T>> {
    let value = OBJECTS.read().unwrap().get(&r.addr())?.value.clone();
    value.downcast().ok()
}

/// Swap the value of the live object at `r`, keeping its address and fields. False if there was none.
pub fn replace<C, T: Any + Send + Sync>(r: NativeRef<C>, value: T) -> bool {
    OBJECTS.write().unwrap().get_mut(&r.addr()).map(|e| e.value = Arc::new(value)).is_some()
}

/// Free the object at `r` (its `Destroy`/`[delete]` slot). False if there was none.
pub fn destroy<C>(r: NativeRef<C>) -> bool {
    let removed = OBJECTS.write().unwrap().remove(&r.addr());
    if removed.is_some() {
        abi::rt::fields::forget(r.addr());
        // SAFETY: the address came from Box::into_raw in create_shared and was in the map, so it's freed once.
        drop(unsafe { Box::from_raw(r.addr() as *mut Token) });
    }
    removed.is_some()
}

/// The class the object at `addr` was created as.
pub fn class_of(addr: usize) -> Option<&'static str> {
    OBJECTS.read().unwrap().get(&addr).map(|e| e.class)
}

pub fn live_count() -> usize {
    OBJECTS.read().unwrap().len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use abi::class;

    #[test]
    fn create_get_destroy() {
        let r = create::<class::CMaterialSystem2AppSystemDict, _>(41u32);
        assert_eq!(get::<_, u32>(r).as_deref(), Some(&41));
        assert!(get::<_, String>(r).is_none());
        assert_eq!(class_of(r.addr()), Some("CMaterialSystem2AppSystemDict"));
        assert!(destroy(r));
        assert!(get::<_, u32>(r).is_none());
        assert!(!destroy(r));
    }

    #[test]
    fn class_hierarchy() {
        assert!(abi::rt::casts::derives_from("CDecalSceneObject", "CSceneObject"));
        assert!(!abi::rt::casts::derives_from("CSceneObject", "CDecalSceneObject"));
        let decal = create::<class::CDecalSceneObject, _>(());
        assert!(abi::rt::casts::derives_from(class_of(decal.addr()).unwrap(), "CSceneObject"));
        destroy(decal);
    }
}
