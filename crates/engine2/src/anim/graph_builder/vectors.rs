//! The graph builder's child vectors' `Add`: scalar elements (node indices, weights, IDs, 2D
//! points) and struct elements, which `Add()` makes and returns for managed to fill in.
//! `CUtlVector<float>` is the shared container (see `containers`).

use super::items::{push_int, push_item, push_vec2};
use abi::slots::*;
use abi::{Registry, bind};

pub fn register(r: &mut Registry) {
    bind!(r, c_utl_vector_int16::ADD, |v, x| push_int(v, x as i64));
    bind!(r, c_utl_vector_uint8::ADD, |v, x| push_int(v, x as i64));
    bind!(r, c_utl_vector_string_id::ADD, |v, x| push_int(v, x as i64));
    bind!(r, c_utl_vector_vector2_d::ADD, |v, x| push_vec2(v, [x.vec.x, x.vec.y]));
    bind!(r, c_utl_vector_c_graph_event_condition_node_condition::ADD, push_item);
    bind!(r, c_utl_vector_c_layer_blend_node_layer_definition::ADD, push_item);
    bind!(r, c_utl_vector_c_parameterized_blend_node_blend_range::ADD, push_item);
    bind!(r, c_utl_vector_c_state_machine_node_state_definition::ADD, push_item);
    bind!(r, c_utl_vector_c_state_machine_node_transition_definition::ADD, push_item);
    bind!(r, c_utl_vector_c_state_node_timed_event::ADD, push_item);
    bind!(r, c_utl_vector_float_curve_t_point::ADD, push_item);
}
