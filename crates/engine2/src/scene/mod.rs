//! The scene system: worlds and the objects in them. Server-side it's bookkeeping managed reads
//! back; the client renderer (M2) draws from the same objects.

pub mod aggregate;
mod create;
pub mod dynamic;
pub mod light;
pub mod object;
mod object_slots;
mod probes;
mod sky;
pub mod world;
mod world_queries;

pub fn register(r: &mut abi::Registry) {
    world::register(r);
    world_queries::register(r);
    object_slots::register(r);
    create::register(r);
    aggregate::register(r);
    dynamic::register(r);
    light::register(r);
    probes::register(r);
    sky::register(r);
}
