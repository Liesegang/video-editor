mod gesture;
mod hit;
mod menu;
mod model;
mod render;
mod types;

pub(in crate::ui::panels::node_editor) use gesture::{
    overview_wire_graph_points, wire_interactions, WireInteractionFrame,
};
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use hit::{cubic_bezier_point, segments_intersect};
pub(in crate::ui::panels::node_editor) use hit::{
    editable_wire_is_current, editable_wire_qa_value, editable_wire_sort_key,
    editable_wire_stable_key, knife_segment_hits_edge, rendered_edge_at_position,
    rendered_normal_port_at_position, rendered_port_at_position, rendered_wire_drag_kind,
    wire_secondary_click_hit,
};
pub(in crate::ui::panels::node_editor) use menu::show_wire_context_menu;
pub(in crate::ui::panels::node_editor) use model::{
    edit_for_wire, embedded_pin_center, graph_item_owner,
};
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use render::register_edge_component;
pub(in crate::ui::panels::node_editor) use render::{
    register_container_chrome, register_rendered_edges,
};
pub(in crate::ui::panels::node_editor) use types::{
    EdgeComponent, OverviewWirePainter, QaPin, RenderedEdge, RenderedEdgeKind, RenderedPortKey,
    WireSecondaryClickHit,
};
