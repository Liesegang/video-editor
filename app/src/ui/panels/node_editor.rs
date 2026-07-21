use crate::action::HistoryManager;
use crate::state::context_types::{
    ContextMenuState, NodeEditorMergeLayerReorderGesture, NodeEditorNodeDragOrigin,
    NodeEditorPendingEdit, NodeEditorReparentGesture, NodeEditorState,
};
#[cfg(test)]
use crate::state::context_types::{
    NodeEditorEditableWire, NodeEditorWireDragKind, SelectionTarget,
};
use crate::ui::widgets::searchable_context_menu::{
    register_searchable_popup_qa, searchable_menu_click_is_outside, searchable_popup_placement,
    show_searchable_items_with_qa, show_searchable_popup_frame,
};
use eframe::egui::{self, Color32};
#[cfg(test)]
use egui_snarl::ui::{SnarlPin, SnarlStyle};
#[cfg(test)]
use egui_snarl::Snarl;
use library::model::project::PortOwner;
#[cfg(test)]
use library::model::project::{PortAddress, PortDataType, PortDirection};
#[cfg(test)]
use library::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
#[cfg(test)]
use library::model::{BlendMode, GeneratorContent, ValueContent};
use library::model::{Clip, Node, NodeContainer, NodeContent, NodeGraphBundle, Project};
#[cfg(test)]
use library::plugin::{
    PluginManager, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID,
    STYLE_APPLY_OPERATION, TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
};
use library::EditorService;
#[cfg(test)]
use ordered_float::OrderedFloat;
#[cfg(test)]
use std::collections::BTreeSet;
use std::collections::{BTreeMap, HashMap, HashSet};
#[cfg(test)]
use std::sync::Mutex;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[cfg(test)]
mod blend_popup_tests;
mod canvas;
mod container_output;
#[cfg(test)]
mod merge_connection_contract_tests;
#[cfg(test)]
mod merge_reorder_tests;
mod property_evaluation;
#[cfg(test)]
mod sound_merge_reorder_tests;
mod surface;
#[cfg(test)]
mod test_fixture;
#[cfg(test)]
mod time_context_tests;

use canvas::{
    node_editor_canvas_metadata, node_editor_details_visible,
    node_editor_port_interactions_enabled, node_editor_resize_interactions_enabled,
    node_editor_snarl_style_for, paint_node_editor_canvas_grid, resolve_node_editor_transform,
    sanitized_node_editor_scale, screen_stroke_in_graph_units,
};
#[cfg(test)]
use canvas::{
    node_editor_snarl_style, sanitize_node_editor_transform, NODE_EDITOR_DETAIL_SCALE,
    NODE_EDITOR_MAX_SCALE, NODE_EDITOR_MAX_TRANSLATION, NODE_EDITOR_MIN_SCALE,
    NODE_EDITOR_RESIZE_INTERACTION_SCALE,
};
use container_output::{
    container_output_binding_port, container_output_binding_type, container_output_node_id,
    container_output_port, container_output_type_key, AUDIO_OUTPUT_BINDING_PORT,
    IMAGE_OUTPUT_BINDING_PORT,
};
use property_evaluation::{evaluate_node_property, render_node_property_issue};
mod types;

use types::{
    ContainerKind, ContainerVisual, GraphItem, PinDefinition, PortAnchorKind, AUTO_LAYOUT_CLIP_GAP,
    AUTO_LAYOUT_CLIP_TOP, AUTO_LAYOUT_COLUMN_GAP, AUTO_LAYOUT_COMPOSITION_BOTTOM,
    AUTO_LAYOUT_COMPOSITION_LEFT, AUTO_LAYOUT_COMPOSITION_RIGHT, AUTO_LAYOUT_COMPOSITION_TOP,
    AUTO_LAYOUT_NODE_PADDING, AUTO_LAYOUT_ROW_GAP, AUTO_LAYOUT_TRACK_BOTTOM, AUTO_LAYOUT_TRACK_GAP,
    AUTO_LAYOUT_TRACK_LEFT, AUTO_LAYOUT_TRACK_RIGHT, AUTO_LAYOUT_TRACK_TOP,
    CONTAINER_CONTROL_OFFSET, CONTAINER_HEADER_HEIGHT, CONTAINER_PORT_Y,
    CONTAINER_RIGHT_PORT_ROW_HEIGHT, CONTAINER_RIGHT_PORT_Y, DETACHED_GRAPH_NODE_GAP,
    EMBEDDED_PORT_LABEL_INSET, INLINE_CONTROL_WIDTH, MERGE_BODY_WIDTH, MIN_CONTAINER_SIZE,
    NODE_BODY_WIDTH, NODE_HEADER_WIDTH, NODE_REPARENT_DRAG_THRESHOLD,
    NODE_REPARENT_POINTER_OVERLAP_THRESHOLD, PORT_LABEL_WIDTH, PORT_ROW_HEIGHT, PORT_SOCKET_SIZE,
    PROPERTY_LABEL_WIDTH, RESIZE_CORNER_SIZE, RESIZE_HIT_WIDTH, WIRE_DRAG_THRESHOLD,
    WIRE_ENDPOINT_RADIUS, WIRE_HIT_RADIUS, WIRE_PORT_DROP_RADIUS, WIRE_RECONNECT_HANDLE_OFFSET,
    WIRE_RECONNECT_HANDLE_RADIUS,
};
mod interaction;

use interaction::{
    EdgeComponent, OverviewWirePainter, QaPin, RenderedEdge, RenderedEdgeKind, RenderedPortKey,
    WireSecondaryClickHit,
};
use surface::{deselects_wire, selection_change, SurfaceCapture, SurfaceProjection};
mod qa;

#[cfg(test)]
use qa::{
    capture_test_metadata, capture_test_rect, reset_test_rects, test_metadata, test_rect,
    test_rects,
};
mod commands;

use commands::{NodeEdit, QueuedNodeEdit};
mod layout;

use layout::{AutoLayoutPlan, AutoLayoutScope, ContainerLayout, LayoutEdit};
mod viewer;

use viewer::ProjectNodeViewer;

mod components;

use components::{
    bounded_non_selectable_label, continuous_color_edit_button, continuous_response_finished,
    layout_toolbar, non_selectable_label, port_label_width, property_label,
    strong_non_selectable_label,
};
use components::{
    container_highlight_metadata, container_icon, container_visual_style, node_icon, node_palette,
    paint_container_backdrop, paint_container_foreground, pin_color, pin_info,
    value_operation_label, ContainerVisualStyle, VALUE_NODE_CATEGORY_LABEL,
};
use qa::{
    clipped_qa_rect, edge_endpoint_qa_metadata, qa_container_key, qa_port_id, qa_rect_metadata,
    wire_port_drop_rect,
};
mod queries;
#[cfg(test)]
use interaction::node_selection_after_snarl_click;
#[cfg(test)]
use interaction::resize_regions;
use interaction::{capture_container_resize_before_canvas, container_resize_interactions};
use interaction::{
    captured_snarl_drag_node, captured_snarl_drag_target, select_logical_item,
    selected_container_owners, selection_target_for_owner,
};
#[cfg(test)]
use interaction::{cubic_bezier_point, register_edge_component};
use interaction::{
    disconnect_context_target, edit_for_wire, embedded_pin_center, graph_item_owner,
};
use interaction::{
    editable_wire_is_current, editable_wire_qa_value, editable_wire_sort_key,
    editable_wire_stable_key, knife_segment_hits_edge, reconnect_handle_at_position,
    reconnect_handle_position, rendered_container_output_at_position, rendered_edge_at_position,
    rendered_normal_port_at_position, rendered_port_at_position, rendered_wire_drag_kind,
    wire_secondary_click_hit,
};
use interaction::{overview_wire_graph_points, wire_interactions, WireInteractionFrame};
use interaction::{
    register_container_chrome, register_implicit_time_context_wires, register_rendered_edges,
    TimeContextNode,
};
#[cfg(test)]
use queries::clip_is_active;
pub(super) use queries::node_timing_drag_config;
use queries::{
    canonical_pin_definitions, container_collapsed, container_inactive, container_name_and_size,
    container_title, graph_item_inactive, graph_item_inactive_reason, graph_item_title,
    input_definitions, node_property_definition, node_property_time, node_title,
    output_definitions, parent_container_owner, port_owner_composition,
    port_owner_for_node_container, GraphItemInactiveReason,
};
mod panel;
pub use panel::node_editor_panel;
mod graph_build;
pub use commands::flush_pending_continuous_edit;
use commands::node_can_splice_connection;
use commands::{apply_edit, apply_queued_node_edits};
use commands::{
    create_operation_node_for_request, node_create_menu_items, wire_splice_menu_items,
    NodeCreateRequest,
};
#[cfg(test)]
use commands::{insert_node_on_connection, splice_existing_node_on_connection};
use components::{
    blend_mode_label, blend_mode_qa_key, blend_mode_searchable_items,
    connection_supports_authored_blend, estimated_merge_input_anchor_offset,
    merge_input_index_for_connection, merge_input_slots, merge_layer_rows,
    native_variadic_merge_for_node, native_variadic_merge_target, register_merge_layer_component,
    wire_order_menu_state, wire_order_menu_states, wire_order_qa_metadata, MergeInputSlot,
    MergeInputSlotRole, MergeLayerRow, NativeVariadicMergeKind,
};
#[cfg(test)]
use components::{merge_images_target_node_id, merge_vacant_slot, WireOrderMenuState};
use graph_build::{build_snarl, container_visual};
use interaction::show_wire_context_menu;
#[cfg(test)]
use layout::collect_layout_edits;
#[cfg(test)]
use layout::padded_intersection;
use layout::{
    apply_auto_layout, ensure_structural_merge_layout, set_container_geometry, set_container_size,
    translate_container,
};
use layout::{
    apply_layout_edit, collect_layout_edits_for_selection, composition_content_rect,
    container_hierarchy_needs_reflow, estimated_node_rect, layout_needs_reflow,
    nested_content_rect, rect_contains_rect,
};
#[cfg(test)]
use layout::{canonical_edges, composition_graph_node_ids, estimated_merge_node_width};
use layout::{compute_auto_layout, compute_full_composition_layout};
use layout::{
    estimated_node_size, estimated_node_width, immediate_child_rects, rank_nodes_by_scc,
    rects_are_closer_than,
};
mod authoring;
use authoring::*;
mod reparent;
use reparent::*;
#[cfg(test)]
mod root_tests;
