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

/// A stored field value, widened to a few plain kinds (see [`dump`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Dumped {
    Bool(bool),
    Int(i64),
    Float(f32),
    /// Vector3 as `[x, y, z]`, Rotation as `[x, y, z, w]`.
    Floats(Vec<f32>),
    Str(String),
}

fn widen(v: &(dyn Any + Send)) -> Option<Dumped> {
    use crate::structs::{Rotation, Vector3};
    macro_rules! int {
        ($($t:ty),*) => { $(if let Some(x) = v.downcast_ref::<$t>() { return Some(Dumped::Int(*x as i64)); })* };
    }
    int!(i8, i16, i32, i64, u8, u16, u32, u64);
    if let Some(x) = v.downcast_ref::<bool>() {
        return Some(Dumped::Bool(*x));
    }
    if let Some(x) = v.downcast_ref::<f32>() {
        return Some(Dumped::Float(*x));
    }
    if let Some(x) = v.downcast_ref::<f64>() {
        return Some(Dumped::Float(*x as f32));
    }
    if let Some(x) = v.downcast_ref::<Vector3>() {
        return Some(Dumped::Floats(vec![x.vec.x, x.vec.y, x.vec.z]));
    }
    if let Some(x) = v.downcast_ref::<Rotation>() {
        return Some(Dumped::Floats(vec![x.quat.x, x.quat.y, x.quat.z, x.quat.w]));
    }
    v.downcast_ref::<String>().map(|s| Dumped::Str(s.clone()))
}

/// Every field set on the object at `addr`, as (getter slot index, value), sorted by index.
/// Values of other types are left out.
pub fn dump(addr: usize) -> Vec<(usize, Dumped)> {
    let store = STORE.lock().unwrap();
    let mut out: Vec<_> = store.get(&addr).map(|f| f.iter().filter_map(|(k, v)| Some((*k, widen(v.as_ref())?))).collect()).unwrap_or_default();
    out.sort_by_key(|(k, _)| *k);
    out
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

    #[test]
    fn dump_widens_and_sorts() {
        let a = 0x3000 as *mut c_void;
        set::<i16, 12>(a, -1);
        set::<bool, 11>(a, true);
        set::<f32, 13>(a, 0.5);
        set::<(), 14>(a, ());
        assert_eq!(dump(0x3000), vec![(11, Dumped::Bool(true)), (12, Dumped::Int(-1)), (13, Dumped::Float(0.5))]);
        forget(0x3000);
    }
}
