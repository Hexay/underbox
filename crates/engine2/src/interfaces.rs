//! Source's module interfaces, which managed asks for by name from `vfx_vulkan` and
//! `filesystem_stdio` (ShaderCompile.cs) through each module's `CreateInterface` export.
//! Every module name is this one library (tools/boot-server.sh links them), so they share state.

use std::ffi::{CStr, c_char, c_void};

/// Answers a known interface's object, or null with a failure code as Source does for an unknown name.
///
/// # Safety
/// `name` is null or NUL-terminated; `return_code` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CreateInterface(name: *const c_char, return_code: *mut i32) -> *mut c_void {
    let name = if name.is_null() { c"" } else { unsafe { CStr::from_ptr(name) } };
    let found = match name.to_bytes() {
        b"VFX_DLL_001" => crate::render::vfx::interface().as_raw(),
        _ => {
            eprintln!("engine2: CreateInterface({}): not provided", name.to_string_lossy());
            std::ptr::null_mut()
        }
    };
    if !return_code.is_null() {
        unsafe { *return_code = found.is_null() as i32 };
    }
    found
}
