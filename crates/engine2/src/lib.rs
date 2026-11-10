//! `libengine2`: what s&box's managed engine loads in place of Facepunch's closed core.
//! Managed calls [`igen_engine`] once; every native function it uses after that is a slot.

pub mod anim;
pub mod app;
pub mod audio;
pub mod compiled;
pub mod containers;
pub mod fs;
pub mod handles;
pub mod math;
pub mod mesh;
pub mod model;
mod igen;
mod interfaces;
pub mod physics;
mod physics2d;
pub mod render;
pub mod resource;
mod compress;
mod net;
mod resource_system;
mod steam;
pub mod scene;
pub mod subsystems;
pub mod kv3;
pub mod utl_buffer;
pub mod frustum;
pub mod bitmap;
pub mod convar;
pub mod resource_compiler;
pub mod shader;
mod video;
mod xr;

pub use igen::igen_engine;
pub use interfaces::CreateInterface;
