//! Every node class's static `Definition::Add(builder)`: a new node appended to the builder.

use super::add_node;
use abi::Registry;

macro_rules! adds {
    ($r:expr; $($m:ident),* $(,)?) => {
        $(abi::bind!($r, abi::slots::$m::ADD, add_node);)*
    };
}

pub fn register(r: &mut Registry) {
    adds!(r;
        c_aim_ik_node_definition, c_and_node_definition, c_animation_pose_node_definition, c_blend1_d_node_definition,
        c_blend2_d_node_definition, c_bone_mask_blend_node_definition, c_bone_mask_node_definition,
        c_bone_mask_selector_node_definition, c_bone_mask_switch_node_definition, c_cached_bool_node_definition,
        c_cached_float_node_definition, c_cached_id_node_definition, c_cached_target_node_definition,
        c_cached_vector_node_definition, c_clip_node_definition, c_clip_selector_node_definition,
        c_const_bool_node_definition, c_const_float_node_definition, c_const_id_node_definition,
        c_const_target_node_definition, c_const_vector_node_definition, c_control_parameter_bool_node_definition,
        c_control_parameter_float_node_definition, c_control_parameter_id_node_definition,
        c_control_parameter_target_node_definition, c_control_parameter_vector_node_definition,
        c_current_sync_event_id_node_definition, c_current_sync_event_node_definition, c_duration_scale_node_definition,
        c_external_pose_node_definition, c_fixed_weight_bone_mask_node_definition, c_float_angle_math_node_definition,
        c_float_clamp_node_definition, c_float_comparison_node_definition, c_float_curve_event_node_definition,
        c_float_curve_node_definition, c_float_ease_node_definition, c_float_math_node_definition,
        c_float_random_node_definition, c_float_range_comparison_node_definition, c_float_remap_node_definition,
        c_float_selector_node_definition, c_float_spring_node_definition, c_float_switch_node_definition,
        c_foot_event_condition_node_definition, c_foot_ik_node_definition, c_footstep_event_id_node_definition,
        c_footstep_event_percentage_through_node_definition, c_graph_event_condition_node_definition,
        c_is_external_graph_slot_filled_node_definition, c_is_target_set_node_definition, c_layer_blend_node_definition,
        c_look_at_chain_node_definition, c_not_node_definition, c_or_node_definition, c_orientation_warp_node_definition,
        c_parameterized_clip_selector_node_definition, c_parameterized_selector_node_definition,
        c_passthrough_node_definition, c_reference_pose_node_definition, c_referenced_graph_node_definition,
        c_root_motion_override_node_definition, c_scale_node_definition, c_selector_node_definition,
        c_speed_scale_node_definition, c_state_completed_condition_node_definition, c_state_machine_node_definition,
        c_state_node_definition, c_sync_event_index_condition_node_definition, c_target_info_node_definition,
        c_target_offset_node_definition, c_target_point_node_definition, c_target_selector_node_definition,
        c_target_warp_node_definition, c_time_condition_node_definition, c_time_controlled_clip_node_definition,
        c_transition_event_condition_node_definition, c_transition_node_definition, c_two_bone_ik_node_definition,
        c_vector_create_node_definition, c_vector_info_node_definition, c_vector_negate_node_definition,
        c_velocity_based_speed_scale_node_definition, c_velocity_blend_node_definition,
        c_virtual_parameter_bone_mask_node_definition, c_virtual_parameter_bool_node_definition,
        c_virtual_parameter_float_node_definition, c_virtual_parameter_id_node_definition,
        c_virtual_parameter_target_node_definition, c_virtual_parameter_vector_node_definition,
        c_zero_pose_node_definition, cid_based_clip_selector_node_definition, cid_based_selector_node_definition,
        cid_comparison_node_definition, cid_event_condition_node_definition, cid_event_node_definition,
        cid_event_percentage_through_node_definition, cid_selector_node_definition, cid_switch_node_definition,
        cid_to_float_node_definition,
    );
}
