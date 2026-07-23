mod resize;
mod selection;
mod wire;

#[cfg(test)]
pub(in crate::ui::panels::node_editor) use resize::resize_regions;
pub(in crate::ui::panels::node_editor) use resize::{
    capture_container_resize_before_canvas, container_resize_interactions,
};
pub(in crate::ui::panels::node_editor) use selection::{
    select_logical_item, selected_container_owners, selection_target_for_owner,
};
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use wire::cubic_bezier_point;
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use wire::register_edge_component;
pub(in crate::ui::panels::node_editor) use wire::show_wire_context_menu;
pub(in crate::ui::panels::node_editor) use wire::{
    disconnect_context_target, edit_for_wire, embedded_pin_center, graph_item_owner,
};
pub(in crate::ui::panels::node_editor) use wire::{
    editable_wire_is_current, editable_wire_qa_value, editable_wire_sort_key,
    editable_wire_stable_key, knife_segment_hits_edge, reconnect_handle_at_position,
    reconnect_handle_position, rendered_container_output_at_position, rendered_edge_at_position,
    rendered_normal_port_at_position, rendered_port_at_position, rendered_wire_drag_kind,
    wire_secondary_click_hit,
};
pub(in crate::ui::panels::node_editor) use wire::{
    overview_wire_graph_points, wire_interactions, WireInteractionFrame,
};
pub(in crate::ui::panels::node_editor) use wire::{
    register_container_chrome, register_implicit_time_context_wires, register_rendered_edges,
    TimeContextNode,
};
pub(in crate::ui::panels::node_editor) use wire::{
    EdgeComponent, OverviewWirePainter, QaPin, RenderedEdge, RenderedEdgeKind, RenderedPortKey,
    WireSecondaryClickHit,
};
