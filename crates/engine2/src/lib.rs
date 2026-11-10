//! `libengine2`: what s&box's managed engine loads in place of Facepunch's closed core.
//! Managed calls [`igen_engine`] once; every native function it uses after that is a slot.

mod igen;
mod interfaces;
pub mod subsystems;

pub use igen::igen_engine;
pub use interfaces::CreateInterface;
