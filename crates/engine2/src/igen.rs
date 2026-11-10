//! The handshake: managed's tables in, ours out. Mirrors InteropGen's native `igen_engine`, which
//! fails fast on a stale pairing; a wrong table here would misroute every call, so we do too.

use abi::{CALLBACK_COUNT, HASH, Registry, SLOT_COUNT, STRUCT_SIZES};

/// # Safety
/// Called once by managed (`NativeInterop.Initialize`) with `managed` holding `CALLBACK_COUNT`
/// function pointers, `native` room for `SLOT_COUNT`, and `struct_sizes` one `int` per declared struct.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn igen_engine(hash: i32, managed: *const usize, native: *mut usize, struct_sizes: *const i32) {
    if hash != HASH {
        fatal(&format!(
            "managed interop hash is {hash}, this engine2 was generated for {HASH}: rerun tools/regen.sh against the same sbox-public"
        ));
    }

    let sizes = unsafe { std::slice::from_raw_parts(struct_sizes, STRUCT_SIZES.len()) };
    for (&(name, ours), &theirs) in STRUCT_SIZES.iter().zip(sizes) {
        if ours != theirs as usize {
            fatal(&format!("struct {name}: managed sizeof is {theirs}, ours is {ours}"));
        }
    }

    let callbacks = unsafe { std::slice::from_raw_parts(managed, CALLBACK_COUNT) };
    if let Some(i) = callbacks.iter().position(|&p| p == 0) {
        fatal(&format!("managed callback {i} is null"));
    }
    unsafe { abi::rt::callbacks::set_table(managed) };

    let mut registry = Registry::default();
    crate::subsystems::register(&mut registry);
    registry.install();

    let table = abi::native_table();
    unsafe { std::ptr::copy_nonoverlapping(table.as_ptr(), native, SLOT_COUNT) };

    let bound = (0..SLOT_COUNT).filter(|&i| abi::status(i) != abi::SlotStatus::Stub).count();
    eprintln!("engine2: igen_engine hash {HASH}: {SLOT_COUNT} slots ({bound} bound), {CALLBACK_COUNT} callbacks");
}

/// The slot table as managed receives it, every subsystem bound: tests call slots through the
/// thunks, exercising the ABI. Installed once per test process (`Registry::install` is one-shot).
#[cfg(test)]
pub(crate) fn test_table() -> &'static [usize; SLOT_COUNT] {
    static TABLE: std::sync::OnceLock<Box<[usize; SLOT_COUNT]>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut registry = Registry::default();
        crate::subsystems::register(&mut registry);
        registry.install();
        Box::new(abi::native_table())
    })
}

fn fatal(message: &str) -> ! {
    eprintln!("engine2: fatal: {message}");
    std::process::abort()
}
