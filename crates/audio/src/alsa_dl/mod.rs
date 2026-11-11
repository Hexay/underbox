//! libasound without a link-time dependency, so one libengine2 loads on a server with no ALSA and
//! plays sound on a client. .cargo/config.toml overrides alsa-sys's build script (no `-lasound`);
//! every `snd_*` symbol the alsa crate references is defined here as a jump through a slot, which
//! [`load`] fills from a dlopened libasound.so.2. Signature-free: a `jmp` keeps every argument
//! register (variadics included) as the caller set it. See docs/release.md.

use std::ffi::CString;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;

/// Every slot's target until [`load`] succeeds. Callers check [`load`] first, so reaching it is a bug.
extern "C" fn missing() {
    eprintln!("audio: an ALSA function was called without libasound.so.2 (call alsa_dl::load first)");
    std::process::abort();
}

/// `snd_x:` → `jmp [slot::snd_x]`, one section per symbol so unused ones are gc'd.
macro_rules! trampolines {
    ($($name:ident),* $(,)?) => {
        #[allow(non_upper_case_globals)]
        mod slot {
            use std::ffi::c_void;
            use std::sync::atomic::AtomicPtr;
            $(pub static $name: AtomicPtr<c_void> = AtomicPtr::new($crate::alsa_dl::missing as *mut c_void);)*
        }
        $(std::arch::global_asm!(
            concat!(
                ".pushsection .text.underbox_alsa.", stringify!($name), ",\"ax\",@progbits\n",
                ".globl ", stringify!($name), "\n",
                ".type ", stringify!($name), ",@function\n",
                stringify!($name), ":\n",
                "    jmp qword ptr [rip + {slot}]\n",
                ".size ", stringify!($name), ", . - ", stringify!($name), "\n",
                ".popsection",
            ),
            slot = sym slot::$name,
        );)*
        pub(super) static SLOTS: &[(&str, &std::sync::atomic::AtomicPtr<std::ffi::c_void>)] =
            &[$((stringify!($name), &slot::$name)),*];
    };
}

mod symbols;

/// Dlopens libasound.so.2 once and points every slot at it; false when it isn't installed.
/// Gotcha: `#[inline(never)]` keeps this in the codegen unit that defines the trampolines, so
/// GNU ld pulls that object in before it scans the alsa rlib that references them.
#[inline(never)]
pub fn load() -> bool {
    static LOADED: OnceLock<bool> = OnceLock::new();
    *LOADED.get_or_init(|| {
        // SAFETY: dlopen/dlsym with NUL-terminated names; the handle is never closed.
        unsafe {
            let lib = libc::dlopen(c"libasound.so.2".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if lib.is_null() {
                return false;
            }
            for (name, slot) in symbols::SLOTS {
                let f = libc::dlsym(lib, CString::new(*name).unwrap().as_ptr());
                if !f.is_null() {
                    slot.store(f, Ordering::Release);
                }
            }
            true
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    #[test]
    fn every_symbol_resolves_when_libasound_is_installed() {
        if !load() {
            return;
        }
        let absent: Vec<_> = symbols::SLOTS
            .iter()
            .filter(|(_, s)| s.load(Ordering::Acquire) == missing as *mut c_void)
            .map(|(n, _)| *n)
            .collect();
        assert!(absent.is_empty(), "not in this libasound: {absent:?}");
    }
}
