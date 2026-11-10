//! Server-side animation: models' skeletons, attachments and hitboxes; animatable scene objects'
//! poses from AnimGraph2 graphs or direct sequences; and the graph resources and controllers
//! managed sets parameters through. The animation itself is the `anim` crate.

mod bone_slots;
mod builder;
mod callback;
mod controller;
mod controller_debug;
mod conv;
pub mod data;
mod graph;
mod graph_builder;
mod model_slots;
pub mod object;
mod playback_slots;
pub mod resources;
mod values;

#[cfg(test)]
mod tests;

pub fn register(r: &mut abi::Registry) {
    model_slots::register(r);
    graph::register(r);
    graph_builder::register(r);
    controller::register(r);
    controller_debug::register(r);
    bone_slots::register(r);
    playback_slots::register(r);
    builder::register(r);
    resources::register(r);
    abi::bind!(r, abi::slots::g_p_animation_system_utils::MAINTAIN_DECODE_CACHES, || {});
}
