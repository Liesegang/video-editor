mod property_controls;

pub(in crate::ui::panels::node_editor) use property_controls::{
    bounded_non_selectable_label, bounded_strong_non_selectable_label,
    continuous_color_edit_button, continuous_response_finished, non_selectable_label,
    port_label_width, property_label, strong_non_selectable_label,
};

mod merge_layers;
mod theme;

#[cfg(test)]
pub(in crate::ui::panels::node_editor) use merge_layers::WireOrderMenuState;
pub(in crate::ui::panels::node_editor) use merge_layers::{
    blend_mode_label, blend_mode_qa_key, connection_supports_authored_blend, merge_layer_rows,
    register_merge_layer_component, register_merge_layer_popup_component, wire_order_menu_state,
    wire_order_menu_states, wire_order_qa_metadata, AUTHORED_BLEND_MODES,
};
pub(in crate::ui::panels::node_editor) use theme::{
    container_icon, node_icon, node_palette, paint_container_backdrop, paint_container_foreground,
    pin_color, pin_info, value_operation_label, VALUE_NODE_CATEGORY_LABEL,
};
