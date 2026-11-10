//! `libengine2`: what s&box's managed engine loads in place of Facepunch's closed core.
//! Managed calls [`igen_engine`] once; every native function it uses after that is a slot.

pub mod app;
pub mod audio;
pub mod compiled;
pub mod containers;
pub mod fs;
pub mod handles;
pub mod math;
pub mod model;
mod igen;
mod interfaces;
pub mod physics;
pub mod render;
pub mod resource;
mod resource_system;
pub mod scene;
pub mod subsystems;

pub use igen::igen_engine;
pub use interfaces::CreateInterface;
