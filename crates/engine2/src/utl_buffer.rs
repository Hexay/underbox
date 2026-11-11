//! `CUtlBuffer`: bytes native hands managed (serialized KV3, mesh data), read via `Base`/`TellMaxPut`.

use abi::slots::c_utl_buffer as buf;
use abi::{NativeRef, RawPtr, Registry, bind, class};

pub type Ref = NativeRef<class::CUtlBuffer>;

/// Fixed once made, so `Base` stays valid until `Dispose`.
pub struct UtlBuffer(Box<[u8]>);

pub fn create(bytes: Vec<u8>) -> Ref {
    objects::create(UtlBuffer(bytes.into_boxed_slice()))
}

/// Native writing into a buffer managed made (`CUtlBuffer.Create`): it then holds `bytes`, and a
/// `Base` taken before is stale, as after a real CUtlBuffer grows. False for a dead buffer.
pub fn put(r: Ref, bytes: Vec<u8>) -> bool {
    objects::replace(r, UtlBuffer(bytes.into_boxed_slice()))
}

pub fn bytes(r: Ref) -> Option<std::sync::Arc<UtlBuffer>> {
    objects::get(r)
}

impl UtlBuffer {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, buf::CREATE, || create(Vec::new()));
    bind!(r, buf::DISPOSE, |r| {
        objects::destroy(r);
    });
    bind!(r, buf::BASE, |r| bytes(r).map_or(RawPtr::NULL, |b| RawPtr::from_raw(b.0.as_ptr().cast_mut().cast())));
    bind!(r, buf::TELL_MAX_PUT, |r| bytes(r).map_or(0, |b| b.0.len() as i32));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_and_size() {
        let r = create(b"abc".to_vec());
        let b = bytes(r).unwrap();
        assert_eq!(b.as_slice(), b"abc");
        assert!(put(r, b"wxyz".to_vec()));
        assert_eq!(bytes(r).unwrap().as_slice(), b"wxyz", "same handle, new contents");
        assert!(objects::destroy(r));
        assert!(bytes(r).is_none());
        assert!(!put(r, Vec::new()), "a disposed buffer takes nothing");
    }
}
