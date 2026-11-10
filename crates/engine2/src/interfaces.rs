//! Source's module interfaces, which managed asks for by name from `vfx_vulkan` and
//! `filesystem_stdio` (ShaderCompile.cs) through each module's `CreateInterface` export.

use std::ffi::{CStr, c_char, c_void};

/// No module interfaces yet: answers null with a failure code, as Source's does for an unknown name.
// TODO: VFX_DLL_001 backed by the shader translation pipeline (docs/PLAN.md, Shaders).
///
/// # Safety
/// `name` is null or NUL-terminated; `return_code` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CreateInterface(name: *const c_char, return_code: *mut i32) -> *mut c_void {
    let name = if name.is_null() { "" } else { &*unsafe { CStr::from_ptr(name) }.to_string_lossy() };
    eprintln!("engine2: CreateInterface({name}): not provided");
    if !return_code.is_null() {
        unsafe { *return_code = 1 };
    }
    std::ptr::null_mut()
}
