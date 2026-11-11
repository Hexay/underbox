//! One pipe's callbacks by manual dispatch, as native engine2 pumps them: our own async calls'
//! results go to their handlers, and every other callback is handed on verbatim (managed's Steamworks
//! layer decodes those and fetches its own call results).

use std::collections::HashMap;
use std::sync::Mutex;
use steamworks::sys;

type Handler = Box<dyn FnOnce(Option<&[u8]>) + Send>;

/// Async calls we started and are waiting on.
#[derive(Default)]
pub(crate) struct Calls(Mutex<HashMap<u64, Handler>>);

impl Calls {
    /// Run `done` with the call's result struct, or `None` if Steam says it failed. False (and `done`
    /// dropped) for `k_uAPICallInvalid`: the call never started.
    pub fn expect(&self, call: u64, done: impl FnOnce(Option<&[u8]>) + Send + 'static) -> bool {
        if call == 0 {
            return false;
        }
        self.0.lock().unwrap().insert(call, Box::new(done));
        true
    }

    fn take(&self, call: u64) -> Option<Handler> {
        self.0.lock().unwrap().remove(&call)
    }
}

/// `T`'s bytes at `offset`, or zero when the struct is short.
pub(crate) fn read_at<T: Copy + Default>(bytes: &[u8], offset: usize) -> T {
    match bytes.get(offset..offset + size_of::<T>()) {
        // SAFETY: the slice holds size_of::<T>() bytes and T is plain data (integers, floats, bool from Steam's 0/1).
        Some(b) => unsafe { b.as_ptr().cast::<T>().read_unaligned() },
        None => T::default(),
    }
}

/// A field of a Steam callback struct, read from its bytes at Steam's own (platform-packed) offset.
macro_rules! field {
    ($bytes:expr, $ty:ty, $field:ident: $as:ty) => {
        $crate::steam::pump::read_at::<$as>($bytes, ::std::mem::offset_of!($ty, $field))
    };
}
pub(crate) use field;

/// A NUL-terminated string field (a `char[N]`) of a callback struct.
pub(crate) fn c_str_at(bytes: &[u8], offset: usize, len: usize) -> String {
    let field = bytes.get(offset..(offset + len).min(bytes.len())).unwrap_or_default();
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// Deliver `pipe`'s pending callbacks: `ours`' call results to their handlers, the rest to `other`.
/// Each callback's bytes are valid only during its call.
pub(crate) fn pump(pipe: sys::HSteamPipe, ours: &Calls, other: &mut dyn FnMut(i32, &[u8])) {
    let completed = sys::SteamAPICallCompleted_t_k_iCallback as i32;
    let mut result = Vec::new();
    // SAFETY: manual dispatch on a live pipe; each message's buffer lives until FreeLastCallback.
    unsafe {
        sys::SteamAPI_ManualDispatch_RunFrame(pipe);
        let mut msg: sys::CallbackMsg_t = std::mem::zeroed();
        while sys::SteamAPI_ManualDispatch_GetNextCallback(pipe, &mut msg) {
            let len = msg.m_cubParam.max(0) as usize;
            let data = if msg.m_pubParam.is_null() { &[][..] } else { std::slice::from_raw_parts(msg.m_pubParam, len) };
            let call = field!(data, sys::SteamAPICallCompleted_t, m_hAsyncCall: u64);
            match (msg.m_iCallback == completed).then(|| ours.take(call)).flatten() {
                Some(done) => {
                    let kind = field!(data, sys::SteamAPICallCompleted_t, m_iCallback: i32);
                    let size = field!(data, sys::SteamAPICallCompleted_t, m_cubParam: u32);
                    result.resize(size as usize, 0u8);
                    let mut failed = false;
                    let ok = sys::SteamAPI_ManualDispatch_GetAPICallResult(pipe, call, result.as_mut_ptr().cast(), size as i32, kind, &mut failed);
                    done((ok && !failed).then_some(&result[..]));
                }
                None => other(msg.m_iCallback, data),
            }
            sys::SteamAPI_ManualDispatch_FreeLastCallback(pipe);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn calls_run_once_and_invalid_calls_never() {
        let calls = Calls::default();
        let ran = Arc::new(AtomicBool::new(false));
        assert!(!calls.expect(0, |_| unreachable!()));
        let r = ran.clone();
        assert!(calls.expect(9, move |b| r.store(b == Some(&[1u8][..]), Ordering::Relaxed)));
        calls.take(9).unwrap()(Some(&[1]));
        assert!(ran.load(Ordering::Relaxed) && calls.take(9).is_none());
    }

    #[test]
    fn reads_fields_at_steam_offsets() {
        let mut bytes = vec![0u8; size_of::<sys::SteamAPICallCompleted_t>()];
        bytes[..8].copy_from_slice(&77u64.to_le_bytes());
        bytes[8..12].copy_from_slice(&3416i32.to_le_bytes());
        assert_eq!(field!(&bytes, sys::SteamAPICallCompleted_t, m_hAsyncCall: u64), 77);
        assert_eq!(field!(&bytes, sys::SteamAPICallCompleted_t, m_iCallback: i32), 3416);
        assert_eq!(read_at::<u64>(&bytes[..4], 0), 0, "short structs read as zero");
        assert_eq!(c_str_at(b"ab\0cd", 0, 5), "ab");
        assert_eq!(c_str_at(b"abc", 1, 9), "bc");
    }
}
