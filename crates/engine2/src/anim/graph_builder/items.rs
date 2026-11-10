//! The native objects a graph builder hands out: node definitions and the structs inside them
//! (`Item`), their child vectors (`List`, or a `CUtlVector<float>`), all owned by one builder and
//! freed with it. Plain `m_*` fields live in the generic field store, keyed by object address.

use crate::containers::{self, UtlVector};
use abi::{NativeClass, NativeRef, class};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Every object address a builder made, so `DeleteThis` frees them all.
#[derive(Default)]
pub struct Owned(Mutex<Vec<usize>>);

impl Owned {
    fn adopt<C>(&self, r: NativeRef<C>) -> NativeRef<C> {
        self.0.lock().unwrap().push(r.addr());
        r
    }

    pub fn free_all(&self) {
        for addr in std::mem::take(&mut *self.0.lock().unwrap()) {
            objects::destroy(NativeRef::<class::CGraphBuilder>::from_raw(addr as *mut _));
        }
    }
}

pub enum Child {
    List(usize, Arc<List>),
    Floats(usize, Arc<UtlVector<f32>>),
    Object(usize, Arc<Item>),
}

impl Child {
    fn addr(&self) -> usize {
        match self {
            Child::List(a, _) | Child::Floats(a, _) | Child::Object(a, _) => *a,
        }
    }
}

/// A node definition or a struct element (state, transition, blend range...).
pub struct Item {
    pub class: &'static str,
    owned: Arc<Owned>,
    pub setters: Mutex<BTreeMap<&'static str, i64>>,
    pub children: Mutex<BTreeMap<&'static str, Child>>,
}

#[derive(Default)]
pub struct List {
    owned: Arc<Owned>,
    pub ints: Mutex<Vec<i64>>,
    pub vec2: Mutex<Vec<[f32; 2]>>,
    pub items: Mutex<Vec<(usize, Arc<Item>)>>,
}

/// Make an item of class `C` owned by `owned`.
pub fn new_item<C: NativeClass>(owned: &Arc<Owned>) -> (NativeRef<C>, Arc<Item>) {
    let item = Arc::new(Item { class: C::NAME, owned: owned.clone(), setters: Mutex::default(), children: Mutex::default() });
    (owned.adopt(objects::create_shared::<C, _>(item.clone())), item)
}

pub fn item<C>(r: NativeRef<C>) -> Option<Arc<Item>> {
    objects::get::<C, Item>(r)
}

/// Record a setter's value (`SetBlendMode`, `SetID`...) on the item.
pub fn set<C>(r: NativeRef<C>, name: &'static str, value: i64) {
    if let Some(i) = item(r) {
        i.setters.lock().unwrap().insert(name, value);
    }
}

impl Item {
    /// The child `name`, made by `make` on first use; the same object every call after.
    fn child<V>(&self, name: &'static str, make: impl FnOnce(&Arc<Owned>) -> Child) -> NativeRef<V> {
        let mut children = self.children.lock().unwrap();
        let addr = children.entry(name).or_insert_with(|| make(&self.owned)).addr();
        NativeRef::from_raw(addr as *mut _)
    }
}

/// The item's child vector `name` (indices, IDs, 2D points or struct elements).
pub fn list<N, V: NativeClass>(n: NativeRef<N>, name: &'static str) -> NativeRef<V> {
    item(n).map_or(NativeRef::NULL, |i| {
        i.child(name, |owned| {
            let list = Arc::new(List { owned: owned.clone(), ..List::default() });
            Child::List(owned.adopt(objects::create_shared::<V, _>(list.clone())).addr(), list)
        })
    })
}

/// The item's child `CUtlVector<float>` `name` (bound in `containers`).
pub fn floats<N, V: NativeClass>(n: NativeRef<N>, name: &'static str) -> NativeRef<V> {
    item(n).map_or(NativeRef::NULL, |i| {
        i.child(name, |owned| {
            let v = owned.adopt(containers::create::<V, f32>(0, 0));
            Child::Floats(v.addr(), objects::get::<V, UtlVector<f32>>(v).expect("just made"))
        })
    })
}

/// The item's child struct `name` (`FloatCurve_t`, `FloatRange_t`).
pub fn object<N, V: NativeClass>(n: NativeRef<N>, name: &'static str) -> NativeRef<V> {
    item(n).map_or(NativeRef::NULL, |i| {
        i.child(name, |owned| {
            let (r, it) = new_item::<V>(owned);
            Child::Object(r.addr(), it)
        })
    })
}

fn with_list<V>(v: NativeRef<V>, f: impl FnOnce(&List)) {
    if let Some(l) = objects::get::<V, List>(v) {
        f(&l);
    }
}

pub fn push_int<V>(v: NativeRef<V>, x: i64) {
    with_list(v, |l| l.ints.lock().unwrap().push(x));
}

pub fn push_vec2<V>(v: NativeRef<V>, x: [f32; 2]) {
    with_list(v, |l| l.vec2.lock().unwrap().push(x));
}

/// `CUtlVector<Struct>::Add()`: a new element, appended.
pub fn push_item<V, C: NativeClass>(v: NativeRef<V>) -> NativeRef<C> {
    let Some(l) = objects::get::<V, List>(v) else { return NativeRef::NULL };
    let (r, it) = new_item::<C>(&l.owned);
    l.items.lock().unwrap().push((r.addr(), it));
    r
}
