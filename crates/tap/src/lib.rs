//! A libengine2 that records every managed↔native interop call and forwards it to a target engine2
//! (Facepunch's or ours) laid next to it as `libengine2_target.so`: docs/differential-tap.md.
#![cfg(target_os = "linux")]

mod call;
mod config;
mod generated;
mod sink;
mod val;

use std::ffi::{CStr, c_void};

type IgenEngine = unsafe extern "C" fn(i32, *const usize, *mut usize, *const i32);

/// Managed's handshake. Wraps both tables around the target's own `igen_engine`.
///
/// # Safety
/// As the target's `igen_engine`: `managed` holds `CALLBACK_COUNT` function pointers, `native` room
/// for `SLOT_COUNT`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn igen_engine(hash: i32, managed: *const usize, native: *mut usize, struct_sizes: *const i32) {
    let target = target_igen();
    if hash != abi::HASH {
        eprintln!("tap: managed interop hash {hash}, the tap was generated for {}: forwarding untapped", abi::HASH);
        return unsafe { target(hash, managed, native, struct_sizes) };
    }
    let Some(cfg) = config::init() else {
        eprintln!("tap: UNDERBOX_TAP_DIR unset: forwarding untapped");
        return unsafe { target(hash, managed, native, struct_sizes) };
    };
    sink::install_exit_flush();
    let ours = unsafe { call::wrap_managed(managed) };
    unsafe { target(hash, ours, native, struct_sizes) };
    let null = unsafe { call::wrap_native(native) };
    let (slots, callbacks) = cfg.counts();
    let tick = cfg.tick.map_or("none (UNDERBOX_TAP_TICK names no slot or callback)", |(side, i)| match side {
        call::Side::Native => abi::NAMES[i],
        call::Side::Managed => generated::trampolines::CALLBACK_NAMES[i],
    });
    eprintln!(
        "tap: logging {slots} slots and {callbacks} callbacks to {} ({null} slots left null by the target); tick on {tick}",
        cfg.dir.display()
    );
}

fn target_igen() -> IgenEngine {
    const TARGET: &CStr = c"libengine2_target.so";
    // SAFETY: plain libc calls; the target is our DT_NEEDED, so it's already mapped.
    let f = unsafe {
        let mut h = libc::dlopen(TARGET.as_ptr(), libc::RTLD_NOW | libc::RTLD_NOLOAD);
        if h.is_null() {
            h = libc::dlopen(TARGET.as_ptr(), libc::RTLD_NOW);
        }
        if h.is_null() { std::ptr::null_mut() } else { libc::dlsym(h, c"igen_engine".as_ptr()) }
    };
    if f.is_null() {
        eprintln!("tap: fatal: no igen_engine in {TARGET:?} (tools/diff/tap-install.sh lays the target next to the tap)");
        std::process::abort();
    }
    // SAFETY: the target's export has InteropGen's handshake signature.
    unsafe { std::mem::transmute::<*mut c_void, IgenEngine>(f) }
}
