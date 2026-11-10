//! A native `KeyValues3` tree. Managed holds a pointer to any node (a root from `Create`, or a
//! member/element it was handed), so each node gets its own object handle on first hand-out;
//! dropping a subtree (delete, or overwriting a container) frees every handle inside it.

use abi::{NativeRef, class};
use kv::{Object, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

pub type Ref = NativeRef<class::KeyValues3>;

/// `KeyValues3Type_t` (Sandbox.Engine/Core/Interop/NativeEngine/KeyValues3Type_t.cs).
pub const TYPE_INVALID: i64 = 0;

#[derive(Debug, Default)]
pub enum Data {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Double(f64),
    String { text: String, resource: bool },
    Binary(Vec<u8>),
    Array(Vec<Arc<Node>>),
    Table(Vec<(String, Arc<Node>)>),
}

#[derive(Debug, Default)]
pub struct Node {
    data: Mutex<Data>,
    handle: AtomicUsize,
}

impl Node {
    pub fn new(data: Data) -> Arc<Self> {
        Arc::new(Self { data: Mutex::new(data), handle: AtomicUsize::new(0) })
    }

    pub fn lock(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap()
    }

    /// Replace the value, freeing the handles of whatever subtree it held.
    pub fn set(&self, data: Data) {
        let old = std::mem::replace(&mut *self.lock(), data);
        release_data(&old);
    }

    /// Make this node a table (emptying it) unless it already is one.
    pub fn table(&self) -> MutexGuard<'_, Data> {
        if !matches!(*self.lock(), Data::Table(_)) {
            self.set(Data::Table(Vec::new()));
        }
        self.lock()
    }

    /// The member named `key` (exact match, as `CKV3MemberName` hashes it), created as null if missing.
    pub fn find_or_create(&self, key: &str) -> Arc<Node> {
        let mut data = self.table();
        let Data::Table(members) = &mut *data else { unreachable!("table() made it a table") };
        if let Some((_, n)) = members.iter().find(|(k, _)| k == key) {
            return n.clone();
        }
        let n = Node::new(Data::Null);
        members.push((key.to_owned(), n.clone()));
        n
    }

    pub fn member(&self, key: &str) -> Option<Arc<Node>> {
        match &*self.lock() {
            Data::Table(members) => members.iter().find(|(k, _)| k == key).map(|(_, n)| n.clone()),
            _ => None,
        }
    }

    pub fn push(&self) -> Arc<Node> {
        let mut data = self.lock();
        if !matches!(*data, Data::Array(_)) {
            drop(data);
            self.set(Data::Array(Vec::new()));
            data = self.lock();
        }
        let Data::Array(items) = &mut *data else { unreachable!("made an array above") };
        let n = Node::new(Data::Null);
        items.push(n.clone());
        n
    }
}

/// The handle managed holds for `node`, made on first use.
pub fn handle(node: &Arc<Node>) -> Ref {
    let addr = node.handle.load(Ordering::Acquire);
    if addr != 0 {
        return NativeRef::from_raw(addr as *mut _);
    }
    let r: Ref = objects::create_shared(node.clone());
    match node.handle.compare_exchange(0, r.addr(), Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => r,
        Err(winner) => {
            objects::destroy(r);
            NativeRef::from_raw(winner as *mut _)
        }
    }
}

pub fn handle_opt(node: Option<Arc<Node>>) -> Ref {
    node.as_ref().map_or(Ref::NULL, handle)
}

pub fn get(r: Ref) -> Option<Arc<Node>> {
    objects::get::<_, Node>(r)
}

/// Free `node`'s handle and every handle beneath it (`DeleteThis` on a root).
pub fn release(node: &Node) {
    release_data(&node.lock());
    let addr = node.handle.swap(0, Ordering::AcqRel);
    if addr != 0 {
        objects::destroy::<class::KeyValues3>(NativeRef::from_raw(addr as *mut _));
    }
}

fn release_data(data: &Data) {
    match data {
        Data::Array(items) => items.iter().for_each(|n| release(n)),
        Data::Table(members) => members.iter().for_each(|(_, n)| release(n)),
        _ => {}
    }
}

/// A new root holding `value`, with its handle.
pub fn create(value: &Value) -> Ref {
    handle(&from_value(value))
}

pub fn from_value(value: &Value) -> Arc<Node> {
    Node::new(match value {
        Value::Null => Data::Null,
        Value::Bool(b) => Data::Bool(*b),
        Value::Int(i) => Data::Int(*i),
        Value::Double(d) => Data::Double(*d),
        Value::String(s) => Data::String { text: s.clone(), resource: false },
        Value::Flagged(flag, v) => match &**v {
            Value::String(s) => Data::String { text: s.clone(), resource: flag.starts_with("resource") },
            v => return from_value(v),
        },
        Value::Binary(b) => Data::Binary(b.clone()),
        Value::Array(items) => Data::Array(items.iter().map(from_value).collect()),
        Value::Object(o) => Data::Table(o.0.iter().map(|(k, v)| (k.clone(), from_value(v))).collect()),
    })
}

pub fn to_value(node: &Node) -> Value {
    match &*node.lock() {
        Data::Null => Value::Null,
        Data::Bool(b) => Value::Bool(*b),
        Data::Int(i) => Value::Int(*i),
        Data::UInt(u) => i64::try_from(*u).map_or(Value::Double(*u as f64), Value::Int),
        Data::Double(d) => Value::Double(*d),
        Data::String { text, resource: false } => Value::String(text.clone()),
        Data::String { text, resource: true } => Value::Flagged("resource".into(), Box::new(Value::String(text.clone()))),
        Data::Binary(b) => Value::Binary(b.clone()),
        Data::Array(items) => Value::Array(items.iter().map(|n| to_value(n)).collect()),
        Data::Table(members) => Value::Object(Object(members.iter().map(|(k, n)| (k.clone(), to_value(n))).collect())),
    }
}
