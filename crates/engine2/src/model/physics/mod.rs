//! A model's physics: the parts/joints data model (`CPhysicsData`), the desc arrays managed
//! builds it from, and the views managed reads it through. Aggregates that simulate it live in
//! `crate::physics::aggregate`.

pub mod data;
pub mod data_slots;
pub mod desc;
pub mod hull;
mod joint_fields;
mod part_slots;
mod views;

pub fn register(r: &mut abi::Registry) {
    data_slots::register(r);
    desc::register(r);
    joint_fields::register(r);
    part_slots::register(r);
}
