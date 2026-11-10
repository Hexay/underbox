//! `CUtlVector<T>` instantiations managed creates to pass lists across (trace results, floats...).

use abi::{NativeClass, NativeRef, Registry, StrRet};
use std::sync::Mutex;

pub struct UtlVector<T>(pub Mutex<Vec<T>>);

pub fn create<C: NativeClass, T: Send + 'static>(_grow: i32, init_capacity: i32) -> NativeRef<C> {
    objects::create(UtlVector(Mutex::new(Vec::<T>::with_capacity(init_capacity.max(0) as usize))))
}

pub fn with<C, T: Send + 'static, R: Default>(v: NativeRef<C>, f: impl FnOnce(&mut Vec<T>) -> R) -> R {
    objects::get::<C, UtlVector<T>>(v).map(|v| f(&mut v.0.lock().unwrap())).unwrap_or_default()
}

pub fn element<C, T: Clone + Default + Send + 'static>(v: NativeRef<C>, i: i32) -> T {
    with(v, |v: &mut Vec<T>| usize::try_from(i).ok().and_then(|i| v.get(i).cloned()).unwrap_or_default())
}

/// Bind a CUtlVector class's slots. `$elem` is the stored type; the ops listed after it are the
/// ones InteropGen exposes for that instantiation (not every vector has `SET_COUNT` or `ADD`).
macro_rules! utl_vector {
    ($r:expr, $m:ident, $elem:ty $(, $op:ident)*) => {{
        use abi::slots::$m as m;
        abi::bind!($r, m::CREATE, create::<_, $elem>);
        abi::bind!($r, m::DELETE_THIS, |v| { objects::destroy(v); });
        abi::bind!($r, m::COUNT, |v| with(v, |v: &mut Vec<$elem>| v.len() as i32));
        $(utl_vector!(@$op $r, m, $elem);)*
    }};
    (@SET_COUNT $r:expr, $m:ident, $elem:ty) => {
        abi::bind!($r, $m::SET_COUNT, |v, n: i32| with(v, |v: &mut Vec<$elem>| v.resize_with(n.max(0) as usize, Default::default)));
    };
    (@ELEMENT $r:expr, $m:ident, $elem:ty) => {
        abi::bind!($r, $m::ELEMENT, element::<_, $elem>);
    };
    (@ADD $r:expr, $m:ident, $elem:ty) => {
        abi::bind!($r, $m::ADD, |v, e: $elem| with(v, |v: &mut Vec<$elem>| v.push(e)));
    };
    (@REMOVE_ALL $r:expr, $m:ident, $elem:ty) => {
        abi::bind!($r, $m::REMOVE_ALL, |v| with(v, |v: &mut Vec<$elem>| v.clear()));
    };
}

pub fn register(r: &mut Registry) {
    use abi::class;
    use abi::structs::{MeshTraceOutput, PhysicsTrace_Result, Vector3};
    utl_vector!(r, c_utl_vector_float, f32, SET_COUNT, ELEMENT, ADD);
    utl_vector!(r, c_utl_vector_uint32, u32, SET_COUNT, ELEMENT);
    utl_vector!(r, c_utl_vector_vector, Vector3, SET_COUNT, ELEMENT);
    utl_vector!(r, c_utl_vector_h_render_texture, NativeRef<class::CTextureBase>, ELEMENT);
    utl_vector!(r, c_utl_vector_mesh_trace_output, MeshTraceOutput, ELEMENT);
    utl_vector!(r, c_utl_vector_physics_trace_result, PhysicsTrace_Result, ELEMENT, REMOVE_ALL);

    use abi::slots::c_utl_vector_c_utl_string as s;
    abi::bind!(r, s::CREATE, create::<_, String>);
    abi::bind!(r, s::DELETE_THIS, |v| {
        objects::destroy(v);
    });
    abi::bind!(r, s::COUNT, |v| with(v, |v: &mut Vec<String>| v.len() as i32));
    abi::bind!(r, s::SET_COUNT, |v, n: i32| with(v, |v: &mut Vec<String>| v.resize(n.max(0) as usize, String::new())));
    abi::bind!(r, s::ELEMENT, |v, i| StrRet::from(element::<_, String>(v, i)));
}
