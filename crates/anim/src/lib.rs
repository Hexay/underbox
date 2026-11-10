//! Skeletal animation for the engine: skeletons, clips, poses, parameters, animation graphs, and
//! readers for the source formats that describe them. No engine or ABI types; engine2's `anim`
//! module binds this to managed's slots.

pub mod clip;
pub mod graph;
pub mod params;
pub mod pose;
pub mod sequence;
pub mod skeleton;
pub mod xform;

pub use clip::Clip;
pub use pose::Pose;
pub use skeleton::Skeleton;
pub use xform::Xform;
