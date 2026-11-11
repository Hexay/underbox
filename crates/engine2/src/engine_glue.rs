//! Small engine services: string tokens, `NativeLowLevel` (memcpy, Valve's MurmurHash64),
//! `ThreadJob` names, the main-loop exit request, native search paths and editor-only leftovers.

use abi::slots::{
    c_utl_symbol_table as symbols, engine_glue as glue, error_reports as reports, g_p_engine_service_mgr as services, native_low_level as low,
    thread_job as job, windows_glue as windows,
};
use abi::{RawPtr, Registry, StrRet, StringToken, bind};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Token → the string it was made from, for every token handed out here.
static TOKENS: LazyLock<Mutex<HashMap<u32, String>>> = LazyLock::new(Default::default);

thread_local! {
    static JOBS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Managed asked the main loop to end (`g_pEngineServiceMgr.ExitMainLoop`).
pub fn exit_requested() -> bool {
    EXIT_REQUESTED.load(Ordering::Relaxed)
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn token(s: &str) -> u32 {
    let t = StringToken::of(s).0;
    TOKENS.lock().unwrap().entry(t).or_insert_with(|| s.to_owned());
    t
}

/// Valve's `MurmurHash64` (Appleby's MurmurHash64B: two 32-bit lanes), as resource ids use it.
pub fn murmur_hash64(data: &[u8], seed: u32) -> u64 {
    const M: u32 = 0x5bd1_e995;
    let mix = |h: &mut u32, k: u32| {
        let k = k.wrapping_mul(M);
        *h = h.wrapping_mul(M) ^ (k ^ k >> 24).wrapping_mul(M);
    };
    let (mut h1, mut h2) = (seed ^ data.len() as u32, 0u32);
    let (chunks, mut tail) = data.as_chunks::<8>();
    for c in chunks {
        mix(&mut h1, u32::from_le_bytes([c[0], c[1], c[2], c[3]]));
        mix(&mut h2, u32::from_le_bytes([c[4], c[5], c[6], c[7]]));
    }
    if tail.len() >= 4 {
        mix(&mut h1, u32::from_le_bytes(tail[..4].try_into().unwrap()));
        tail = &tail[4..];
    }
    if !tail.is_empty() {
        h2 ^= tail.iter().enumerate().fold(0, |k, (i, &b)| k | (b as u32) << (8 * i));
        h2 = h2.wrapping_mul(M);
    }
    h1 = (h1 ^ h2 >> 18).wrapping_mul(M);
    h2 = (h2 ^ h1 >> 22).wrapping_mul(M);
    h1 = (h1 ^ h2 >> 17).wrapping_mul(M);
    h2 = (h2 ^ h1 >> 19).wrapping_mul(M);
    (h1 as u64) << 32 | h2 as u64
}

fn copy(dst: RawPtr<c_void>, src: RawPtr<c_void>, len: i64) {
    if len <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    // SAFETY: managed passes two buffers of at least `len` bytes; `copy` tolerates overlap.
    unsafe { std::ptr::copy(src.as_ptr().cast::<u8>(), dst.as_ptr().cast::<u8>(), len as usize) }
}

fn hash(data: RawPtr<c_void>, len: i32, seed: u32) -> u64 {
    // SAFETY: managed passes its key buffer and the key's length.
    murmur_hash64(unsafe { data.cast::<u8>().as_slice(len.max(0) as usize) }, seed)
}

pub fn register(r: &mut Registry) {
    bind!(r, glue::GET_STRING_TOKEN, |s| token(&text(s)));
    bind!(r, glue::GET_STRING_TOKEN_VALUE, |t| StrRet::from(TOKENS.lock().unwrap().get(&t).cloned()));
    bind!(r, glue::ADD_SEARCH_PATH, |path, id, front| crate::fs::add_search_path(&text(id), &text(path), front));
    bind!(r, glue::REMOVE_SEARCH_PATH, |path, id| crate::fs::remove_search_path(&text(id), &text(path)));
    bind!(r, glue::SET_ENGINE_LOGGING_VERBOSE, |_| {});
    // Web API tickets come from a Steam user; until one arrives managed keeps asking, then runs offline.
    // Web auth tickets are bound in steam::client.

    bind!(r, low::COPY, copy);
    bind!(r, low::MURMUR_HASH64, hash);

    bind!(r, job::BEGIN, |name| JOBS.with(|j| j.borrow_mut().push(text(name))));
    bind!(r, job::END, || JOBS.with(|j| {
        j.borrow_mut().pop();
    }));
    bind!(r, job::NAME, || StrRet::from(JOBS.with(|j| j.borrow().last().cloned())));

    // Facepunch's crash reporter, which we don't ship.
    bind!(r, reports::INIT, || {});
    bind!(r, reports::SHUTDOWN, || {});
    bind!(r, services::EXIT_MAIN_LOOP, || EXIT_REQUESTED.store(true, Ordering::Relaxed));
    // Editor-only: Hammer's resource references and the Windows file picker.
    bind!(r, symbols::ADD_STRING, |_, _| {});
    bind!(r, windows::FIND_FILE, || StrRet::from(c""));
    bind!(r, abi::slots::aggregate_glue::GET_MAX_FRAGMENT_COUNT, || MAX_AGGREGATE_FRAGMENTS);
}

/// The most fragments one `SceneAggregateObject` draws; managed splits map geometry and
/// compiled scenes into aggregates of at most this many.
pub const MAX_AGGREGATE_FRAGMENTS: i32 = 4096;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn murmur_hash64_matches_compiled_resource_ids() {
        // RERL ids from compiled resources (ValveResourceFormat's test files) and their names.
        let id = |s: &str| murmur_hash64(s.as_bytes(), 0xEDAB_CDEF);
        assert_eq!(id("materials/megascans/floor_wooden_01/floor_wooden_02.vmat"), 0xb877_3f0f_8a40_fdfa);
        assert_eq!(id("models/props_gameplay/wooden_crate_01/wooden_crate_01_piece_a.vmdl"), 0x2eb3_13da_5077_9853);
        assert_eq!(id("dev/helper/reflectivity_90b_color_tga_88186957.vtex"), 0xedf5_86e1_5e87_a9f5);
    }

    #[test]
    fn tokens_and_jobs_round_trip() {
        let t = token("Hello_World");
        assert_eq!(t, StringToken::of("hello_world").0);
        assert_eq!(TOKENS.lock().unwrap().get(&t).map(String::as_str), Some("Hello_World"));
        JOBS.with(|j| j.borrow_mut().push("a".into()));
        assert_eq!(JOBS.with(|j| j.borrow().last().cloned()).as_deref(), Some("a"));
    }
}
