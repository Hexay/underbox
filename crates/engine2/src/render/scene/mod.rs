//! Drawing scenes the way native's scene system does (docs/renderer.md, "From scene objects to
//! draw calls"): a [`camera`] renderer builds an [`view`] per render, managed's pipeline (or our
//! [`execute::default_layers`]) fills it with [`layer`]s through a [`setup`], and
//! [`execute`] runs the layers into the view's targets. [`system`] has the per-frame slots.

pub mod camera;
mod collect;
pub mod execute;
pub mod layer;
mod occlusion;
pub(crate) mod pvs;
pub mod setup;
pub mod shadow_view;
pub mod skybox;
pub mod system;
pub(crate) mod trace;
pub mod view;

#[cfg(test)]
mod layer_tests;
#[cfg(test)]
pub(crate) mod tests;

/// Read or write a generated field (`bind_fields`) of the object at `addr`, keyed by its getter.
macro_rules! field {
    (get $ty:ty, $getter:path, $addr:expr) => {
        abi::rt::fields::get::<$ty, { $getter.index }>($addr as *mut std::ffi::c_void)
    };
    (set $ty:ty, $getter:path, $addr:expr, $value:expr) => {
        abi::rt::fields::set::<$ty, { $getter.index }>($addr as *mut std::ffi::c_void, $value)
    };
}
pub(crate) use field;

pub fn register(r: &mut abi::Registry) {
    camera::register(r);
    layer::register(r);
    setup::register(r);
    system::register(r);
    view::register(r);
}
