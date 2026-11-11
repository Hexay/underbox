//! `IVfx` (`VFX_DLL_001`, Facepunch's vfx_vulkan): the shader compiler managed drives.

use abi::slots::i_vfx as vfx;
use abi::{NativeRef, Registry, bind, class};
use std::sync::OnceLock;

pub struct Vfx;
pub struct CompileContext;

/// The process's one compiler, as Source modules hand out singletons.
pub fn interface() -> NativeRef<class::IVfx> {
    static VFX: OnceLock<usize> = OnceLock::new();
    let addr = *VFX.get_or_init(|| objects::create::<class::IVfx, _>(Vfx).addr());
    NativeRef::from_raw(addr as *mut _)
}

pub fn register(r: &mut Registry) {
    bind!(r, vfx::INIT, |_, _create_interface| {});
    bind!(r, vfx::CLEAR_SHADER_CACHE, |_| {});
    bind!(r, vfx::CREATE_SHARED_CONTEXT, |_| objects::create(CompileContext));
}
