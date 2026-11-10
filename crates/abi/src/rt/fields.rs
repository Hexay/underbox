//! The per-object field store behind default field-accessor bindings (`bind_fields`; see
//! tools/abigen/src/fields.rs). Keyed by (object address, getter slot index).

use crate::rt::strings::StrRet;
use std::any::Any;
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::sync::{LazyLock, Mutex};

type Fields = HashMap<usize, Box<dyn Any + Send>>;

static STORE: LazyLock<Mutex<HashMap<usize, Fields>>> = LazyLock::new(Default::default);

/// A field's value; zeroed (`Default`) until set.
pub fn get<T: Copy + Default + Send + 'static, const KEY: usize>(this: *mut c_void) -> T {
    let store = STORE.lock().unwrap();
    store.get(&(this as usize)).and_then(|f| f.get(&KEY)).and_then(|v| v.downcast_ref::<T>()).copied().unwrap_or_default()
}

pub fn set<T: Copy + Send + 'static, const KEY: usize>(this: *mut c_void, value: T) {
    STORE.lock().unwrap().entry(this as usize).or_default().insert(KEY, Box::new(value));
}

pub fn get_str<const KEY: usize>(this: *mut c_void) -> StrRet {
    let store = STORE.lock().unwrap();
    StrRet::from(store.get(&(this as usize)).and_then(|f| f.get(&KEY)).and_then(|v| v.downcast_ref::<String>()).cloned())
}

pub fn set_str<const KEY: usize>(this: *mut c_void, value: Option<&CStr>) {
    let value = value.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    STORE.lock().unwrap().entry(this as usize).or_default().insert(KEY, Box::new(value));
}

/// Drop every field of the object at `addr` (it's being destroyed; the address may be reused).
pub fn forget(addr: usize) {
    STORE.lock().unwrap().remove(&addr);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_round_trip_per_object_and_key() {
        let (a, b) = (0x1000 as *mut c_void, 0x2000 as *mut c_void);
        assert_eq!(get::<f32, 7>(a), 0.0, "unset reads zero");
        set::<f32, 7>(a, 2.5);
        set::<f32, 8>(a, 9.0);
        assert_eq!((get::<f32, 7>(a), get::<f32, 8>(a), get::<f32, 7>(b)), (2.5, 9.0, 0.0));
        set_str::<9>(a, Some(c"hi"));
        assert!(matches!(get_str::<9>(a), StrRet::Owned(s) if s.as_c_str() == c"hi"));
        forget(0x1000);
        assert_eq!(get::<f32, 7>(a), 0.0);
    }
}
