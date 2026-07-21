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
mod merge_reorder_tests;
mod property_evaluation;
#[cfg(test)]
mod test_fixture;
#[cfg(test)]
mod time_context_tests;

use canvas::{
    bridge_node_editor_transform, node_editor_canvas_metadata, node_editor_details_visible,
    node_editor_port_interactions_enabled, node_editor_resize_interactions_enabled,
    node_editor_snarl_style_for, paint_node_editor_canvas_grid, sanitize_node_editor_transform,
    sanitized_node_editor_scale, screen_stroke_in_graph_units,
};
#[cfg(test)]
use canvas::{
    node_editor_snarl_style, NODE_EDITOR_DETAIL_SCALE, NODE_EDITOR_MAX_SCALE,
    NODE_EDITOR_MAX_TRANSLATION, NODE_EDITOR_MIN_SCALE, NODE_EDITOR_RESIZE_INTERACTION_SCALE,
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
    WIRE_ENDPOINT_RADIUS, WIRE_HIT_RADIUS, WIRE_PORT_DROP_RADIUS,
};
mod interaction;

use interaction::{
    EdgeComponent, OverviewWirePainter, QaPin, RenderedEdge, RenderedEdgeKind, RenderedPortKey,
    WireSecondaryClickHit,
};
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

#[cfg(test)]
use viewer::resolve_node_editor_transform;
use viewer::ProjectNodeViewer;

mod components;

use components::{
    bounded_non_selectable_label, bounded_strong_non_selectable_label,
    continuous_color_edit_button, continuous_response_finished, non_selectable_label,
    port_label_width, property_label, strong_non_selectable_label,
};
use components::{
    container_icon, node_icon, node_palette, paint_container_backdrop, paint_container_foreground,
    pin_color, pin_info, value_operation_label, VALUE_NODE_CATEGORY_LABEL,
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
use interaction::{
    canvas_marquee_interaction, captured_snarl_drag_node, captured_snarl_drag_target,
    logical_hit_owner, selection_after_logical_click, selection_after_marquee,
    selection_target_for_owner, CanvasSelectionOutcome,
};
use interaction::{capture_container_resize_before_canvas, container_resize_interactions};
#[cfg(test)]
use interaction::{cubic_bezier_point, register_edge_component};
use interaction::{
    disconnect_context_target, edit_for_wire, embedded_pin_center, graph_item_owner,
};
use interaction::{
    editable_wire_is_current, editable_wire_qa_value, editable_wire_sort_key,
    editable_wire_stable_key, knife_segment_hits_edge, rendered_container_output_at_position,
    rendered_edge_at_position, rendered_normal_port_at_position, rendered_port_at_position,
    rendered_wire_drag_kind, wire_secondary_click_hit,
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
#[cfg(test)]
use components::WireOrderMenuState;
use components::{
    blend_mode_label, blend_mode_qa_key, blend_mode_searchable_items,
    connection_supports_authored_blend, estimated_merge_input_anchor_offset,
    merge_images_target_node_id, merge_input_index_for_connection, merge_input_slots,
    merge_layer_rows, register_merge_layer_component, wire_order_menu_state,
    wire_order_menu_states, wire_order_qa_metadata, MergeInputSlot, MergeInputSlotRole,
};
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
type CreateAction = Box<dyn FnOnce(&mut Project) -> bool>;
fn create_action_for_request(
    request: NodeCreateRequest,
    project_service: &EditorService,
    canvas_size: (u64, u64),
    graph_position: egui::Pos2,
    comp_id: Uuid,
) -> Option<CreateAction> {
    let plugin_manager = project_service.get_plugin_manager();
    match request {
        NodeCreateRequest::Text => match project_service.create_text_node(
            "Hello World",
            library::editor::project_service::DEFAULT_TEXT_FONT,
            canvas_size.0,
            canvas_size.1,
        ) {
            Ok(node) => Some(Box::new(move |project| {
                create_prebuilt_node(project, graph_position, node, comp_id)
            })),
            Err(error) => {
                log::error!("Cannot create Text Node: {error}");
                None
            }
        },
        NodeCreateRequest::Solid => match project_service.create_solid_node(
            library::model::frame::color::Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            canvas_size.0,
            canvas_size.1,
        ) {
            Ok(node) => Some(Box::new(move |project| {
                create_prebuilt_node(project, graph_position, node, comp_id)
            })),
            Err(error) => {
                log::error!("Cannot create Solid Node: {error}");
                None
            }
        },
        NodeCreateRequest::Shape => match project_service.create_shape_node(
            library::editor::project_service::DEFAULT_SHAPE_PATH,
            canvas_size.0,
            canvas_size.1,
            100,
            100,
        ) {
            Ok(node) => Some(Box::new(move |project| {
                create_prebuilt_node(project, graph_position, node, comp_id)
            })),
            Err(error) => {
                log::error!("Cannot create Shape Node: {error}");
                None
            }
        },
        NodeCreateRequest::SkSL => match project_service.create_sksl_node(
            library::editor::project_service::DEFAULT_SKSL_SHADER,
            canvas_size.0,
            canvas_size.1,
        ) {
            Ok(node) => Some(Box::new(move |project| {
                create_prebuilt_node(project, graph_position, node, comp_id)
            })),
            Err(error) => {
                log::error!("Cannot create SkSL Node: {error}");
                None
            }
        },
        NodeCreateRequest::Value(value) => Some(Box::new(move |project| {
            create_prebuilt_node(
                project,
                graph_position,
                Node::new_value(value.label(), value),
                comp_id,
            )
        })),
        transform_request @ (NodeCreateRequest::ShapeTransform
        | NodeCreateRequest::ImageTransform) => {
            let node =
                create_operation_node_for_request(&transform_request, plugin_manager.as_ref())?;
            Some(Box::new(move |project| {
                insert_prebuilt_graph(
                    project,
                    graph_position,
                    NodeGraphBundle::new(vec![node], Vec::new(), None),
                    comp_id,
                )
            }))
        }
        NodeCreateRequest::Style(component_id) => {
            match plugin_manager.create_style_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Style Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Effector(component_id) => {
            match plugin_manager.create_effector_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Effector Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Decorator(component_id) => {
            match plugin_manager.create_decorator_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Decorator Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Effect(effect_id) => {
            match plugin_manager.create_effect_operation_node(&effect_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Effect Node {effect_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Merge => Some(Box::new(move |project| {
            create_merge_node(project, graph_position, comp_id)
        })),
        NodeCreateRequest::Clip => Some(Box::new(move |project| {
            create_clip_at_free_slot(project, graph_position, comp_id, "Clip").is_some()
        })),
        NodeCreateRequest::Track => Some(Box::new(move |project| {
            create_track_at_free_slot(project, graph_position, comp_id, "Track").is_some()
        })),
        NodeCreateRequest::Composition => Some(Box::new(move |project| {
            create_composition_node(project, graph_position, comp_id)
        })),
    }
}

struct NodeContextMenuFrame<'a> {
    project_lock: &'a Arc<RwLock<Project>>,
    project_service: &'a EditorService,
    comp_id: Uuid,
    exclusion_rects: &'a [egui::Rect],
    to_global: egui::emath::TSTransform,
    suppress_secondary_click: bool,
}

fn handle_context_menu(
    ui: &mut egui::Ui,
    state: &mut Option<ContextMenuState>,
    frame: NodeContextMenuFrame<'_>,
) -> bool {
    let canvas_size = frame
        .project_lock
        .read()
        .ok()
        .and_then(|project| {
            project
                .get_composition(frame.comp_id)
                .map(|composition| (composition.width, composition.height))
        })
        .unwrap_or((1920, 1080));
    let from_global = frame.to_global.inverse();
    let (secondary_clicked, pointer_position, open_time) = ui.input(|input| {
        (
            input.pointer.secondary_clicked(),
            input.pointer.interact_pos(),
            input.time,
        )
    });
    update_global_context_menu_for_secondary_click(
        state,
        secondary_clicked && !frame.suppress_secondary_click,
        pointer_position,
        ui.min_rect(),
        frame.exclusion_rects,
        frame.to_global,
        open_time,
    );
    let mut should_close = false;
    let mut action: Option<CreateAction> = None;
    if let Some(context) = state {
        let position = context.position;
        let graph_position = from_global * position;
        let popup =
            searchable_popup_placement(position, egui::vec2(320.0, 348.0), ui.ctx().content_rect());
        let menu_id = format!("node_editor_add_menu:{}", context.open_time.to_bits());
        let response = egui::Area::new(egui::Id::new("node_ctx_menu"))
            .order(egui::Order::Foreground)
            .pivot(popup.pivot)
            .fixed_pos(popup.area_anchor)
            .constrain(false)
            .show(ui.ctx(), |ui| {
                show_searchable_popup_frame(ui, popup, |ui| {
                    let plugin_manager = frame.project_service.get_plugin_manager();
                    let items = node_create_menu_items(plugin_manager.as_ref());
                    if let Some(request) = show_searchable_items_with_qa(
                        ui,
                        &menu_id,
                        Some("node_editor.menu.search"),
                        &items,
                    ) {
                        action = create_action_for_request(
                            request,
                            frame.project_service,
                            canvas_size,
                            graph_position,
                            frame.comp_id,
                        );
                        should_close = true;
                    }
                })
            });
        let root_rect = response.inner.response.rect;
        register_searchable_popup_qa("node_editor.menu.root", position, popup, root_rect);
        if ui.input(|input| input.pointer.any_click())
            && ui.input(|input| input.time) - context.open_time > 0.2
            && searchable_menu_click_is_outside(ui.ctx(), &menu_id, root_rect)
        {
            should_close = true;
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
    }
    let mut changed = false;
    if let Some(action) = action {
        if let Ok(mut project) = frame.project_lock.write() {
            changed = action(&mut project);
        }
    }
    if should_close {
        *state = None;
    }
    changed
}

fn update_global_context_menu_for_secondary_click(
    state: &mut Option<ContextMenuState>,
    secondary_clicked: bool,
    pointer_position: Option<egui::Pos2>,
    canvas_rect: egui::Rect,
    exclusion_rects: &[egui::Rect],
    to_global: egui::emath::TSTransform,
    open_time: f64,
) {
    if !secondary_clicked {
        return;
    }
    let Some(position) = pointer_position.filter(|position| canvas_rect.contains(*position)) else {
        return;
    };
    let graph_position = to_global.inverse() * position;
    if exclusion_rects
        .iter()
        .any(|rect| rect.contains(graph_position))
    {
        // A Snarl item owns this gesture. Also close a stale Create menu so a
        // Node/container menu and the global menu cannot remain visible at the
        // same time after a secondary click.
        *state = None;
        return;
    }
    *state = Some(ContextMenuState::new(position, open_time));
}

fn push_history_snapshot(
    project_lock: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) {
    if let Ok(project) = project_lock.read() {
        history_manager.push_project_state(project.clone());
    }
}

#[cfg(test)]
fn available_effector_menu_entries(plugin_manager: &PluginManager) -> Vec<(String, String)> {
    let mut entries = plugin_manager
        .get_available_effectors()
        .into_iter()
        .filter_map(|component_id| {
            match plugin_manager.operation_descriptor(
                EFFECTOR_CATEGORY,
                &component_id,
                EFFECTOR_APPLY_OPERATION,
            ) {
                Ok(descriptor) => Some((component_id, descriptor.label().to_string())),
                Err(error) => {
                    log::warn!("Cannot expose Effector {component_id} in the Node Editor: {error}");
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    entries
}

fn insert_prebuilt_graph(
    project: &mut Project,
    desired: egui::Pos2,
    mut graph: NodeGraphBundle,
    composition_id: Uuid,
) -> bool {
    let Some(container) = node_container_at_position(project, composition_id, desired) else {
        return false;
    };
    let Some((container_position, container_size, existing_node_ids)) =
        container_geometry(project, container)
    else {
        return false;
    };

    // Factory coordinates are only hints. Plugin-backed Nodes may have many
    // more property ports than a factory can anticipate, so lay the detached
    // graph out from its canonical connections and the same conservative
    // card measurements used by the rest of this editor before placement.
    // This is deliberately an app concern: the library factory remains a
    // renderer/UI-independent graph constructor.
    layout_detached_node_graph(project, &mut graph);

    // `output_node_id` identifies the consumer/sink within a detached factory
    // graph. It is useful for creating a brand-new Clip, but ordinary Add in
    // an existing container must never silently replace that container's
    // explicit output binding. Setting an output remains a separate command.
    graph.output_node_id = None;

    // Measure with the same canonical port-derived estimator used for
    // existing Nodes. A temporary Project keeps this layout calculation out
    // of the authoritative model until the atomic insert succeeds.
    let mut measurement_project = project.clone();
    for node in &graph.nodes {
        measurement_project.add_node(node.clone());
    }
    let mut graph_bounds = egui::Rect::NOTHING;
    for node in &graph.nodes {
        let rect = egui::Rect::from_min_size(
            egui::pos2(node.ui_position[0], node.ui_position[1]),
            estimated_node_size(&measurement_project, node.id),
        );
        graph_bounds = graph_bounds.union(rect);
    }
    if !graph_bounds.is_finite() || !graph_bounds.is_positive() {
        return false;
    }

    let content_left = container_position[0]
        + match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_LEFT,
            NodeContainer::Track(_) | NodeContainer::Clip(_) => AUTO_LAYOUT_TRACK_LEFT,
        };
    let content_top = container_position[1]
        + match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_TOP,
            NodeContainer::Track(_) => AUTO_LAYOUT_TRACK_TOP,
            NodeContainer::Clip(_) => AUTO_LAYOUT_CLIP_TOP,
        };
    let max_x = (container_position[0] + container_size[0]
        - AUTO_LAYOUT_TRACK_RIGHT
        - graph_bounds.width())
    .max(content_left);
    let anchor = egui::pos2(
        desired.x.max(content_left).clamp(content_left, max_x),
        desired.y.max(content_top),
    );
    let mut candidate = egui::Rect::from_min_size(anchor, graph_bounds.size());
    let mut occupied = existing_node_ids
        .iter()
        .filter_map(|node_id| estimated_node_rect(project, *node_id))
        .collect::<Vec<_>>();
    occupied.extend(immediate_child_rects(
        project,
        &AutoLayoutPlan::default(),
        container,
    ));
    loop {
        let next_y = occupied
            .iter()
            .filter(|other| rects_are_closer_than(candidate, **other, DETACHED_GRAPH_NODE_GAP))
            .map(|other| other.bottom() + DETACHED_GRAPH_NODE_GAP)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            break;
        };
        candidate = egui::Rect::from_min_size(egui::pos2(anchor.x, next_y), graph_bounds.size());
    }

    let translation = candidate.min - graph_bounds.min;
    for node in &mut graph.nodes {
        node.ui_position[0] += translation.x;
        node.ui_position[1] += translation.y;
    }
    if let Err(error) = project.insert_node_graph(container, graph) {
        log::warn!("Cannot insert Node graph into {container:?}: {error}");
        return false;
    }

    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_collapsed = false;
    }
    ensure_container_hierarchy_contains(project, container, candidate);
    true
}

fn layout_detached_node_graph(project: &Project, graph: &mut NodeGraphBundle) {
    if graph.nodes.is_empty() {
        return;
    }

    let node_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let node_set = node_ids.iter().copied().collect::<HashSet<_>>();
    let mut edges = graph
        .connections
        .iter()
        .filter_map(|connection| {
            let (PortOwner::Node(from), PortOwner::Node(to)) =
                (connection.from.owner, connection.to.owner)
            else {
                return None;
            };
            (node_set.contains(&from) && node_set.contains(&to)).then_some((from, to))
        })
        .collect::<Vec<_>>();
    edges.sort_unstable();
    edges.dedup();
    let ranks = rank_nodes_by_scc(&node_ids, &edges);

    let mut measurement_project = project.clone();
    for node in &graph.nodes {
        measurement_project.add_node(node.clone());
    }

    // Preserve factory order within a rank. Connection order has semantic
    // meaning for multi-input Style ports, and matching that order visually
    // makes Fill-before-Stroke immediately legible.
    let mut columns = BTreeMap::<usize, Vec<Uuid>>::new();
    for node_id in &node_ids {
        columns
            .entry(ranks.get(node_id).copied().unwrap_or_default())
            .or_default()
            .push(*node_id);
    }

    // Half-open geometry would allow an exact 24 px gap. egui Rect
    // intersection treats touching expanded edges as an intersection, so a
    // tiny extra margin keeps this layout out of the reflow detector while
    // preserving the intended 24 px visual rhythm.
    let gap = DETACHED_GRAPH_NODE_GAP;
    let mut column_x = BTreeMap::<usize, f32>::new();
    let mut x = 0.0;
    for (rank, node_ids) in &columns {
        column_x.insert(*rank, x);
        let width = node_ids
            .iter()
            .map(|node_id| estimated_node_size(&measurement_project, *node_id).x)
            .max_by(f32::total_cmp)
            .unwrap_or_default();
        x += width + gap;
    }

    let mut positions = HashMap::<Uuid, [f32; 2]>::new();
    for (rank, node_ids) in columns {
        let mut y = 0.0;
        for node_id in node_ids {
            positions.insert(node_id, [column_x[&rank], y]);
            y += estimated_node_size(&measurement_project, node_id).y + gap;
        }
    }
    for node in &mut graph.nodes {
        if let Some(position) = positions.get(&node.id) {
            node.ui_position = *position;
        }
    }
}

fn create_prebuilt_node(
    project: &mut Project,
    position: egui::Pos2,
    mut node: Node,
    comp_id: Uuid,
) -> bool {
    node.ui_position = [position.x, position.y];
    let node_id = node.id;
    project.add_node(node);
    if let Some(container) = attach_node_at_position(project, node_id, comp_id, position) {
        place_node_in_free_slot(project, node_id, container, position, &[]);
        true
    } else {
        if let Err(error) = project.remove_node(node_id) {
            log::warn!("Cannot roll back unattached Node {node_id}: {error}");
        }
        false
    }
}

fn create_composition_node(project: &mut Project, position: egui::Pos2, comp_id: Uuid) -> bool {
    let mut candidate = project.clone();
    let (composition, root) =
        library::model::Composition::new("Nested Comp", 1920, 1080, 30.0, 10.0);
    let nested_id = composition.id;
    if candidate
        .add_track(root)
        .and_then(|()| candidate.add_composition(composition))
        .is_err()
    {
        return false;
    }

    let mut node = Node::new_composition_instance(
        "Container",
        library::model::CompositionInstanceContent {
            composition_id: nested_id,
        },
    );
    node.ui_position = [position.x, position.y];
    let node_id = node.id;
    candidate.add_node(node);
    if let Some(container) = attach_node_at_position(&mut candidate, node_id, comp_id, position) {
        place_node_in_free_slot(&mut candidate, node_id, container, position, &[]);
        *project = candidate;
        true
    } else {
        false
    }
}

fn create_merge_node(project: &mut Project, position: egui::Pos2, comp_id: Uuid) -> bool {
    create_prebuilt_node(project, position, Node::new_merge("Merge"), comp_id)
}

fn attach_node_at_position(
    project: &mut Project,
    node_id: Uuid,
    comp_id: Uuid,
    position: egui::Pos2,
) -> Option<NodeContainer> {
    let container = node_container_at_position(project, comp_id, position)?;
    if let Err(error) = project.attach_node_to_container(container, node_id) {
        log::warn!("Cannot add Node to {container:?}: {error}");
        return None;
    }
    // A collapsed root has no visible expanded parent to receive a Node.
    // Expand it after the atomic attachment so every successfully created Node
    // is immediately projected by `build_snarl`.
    if let NodeContainer::Composition(composition_id) = container {
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_collapsed = false;
        }
    }
    Some(container)
}

fn place_node_in_free_slot(
    project: &mut Project,
    node_id: Uuid,
    container: NodeContainer,
    desired: egui::Pos2,
    dependencies: &[Uuid],
) -> Option<egui::Pos2> {
    let (container_position, container_size, node_ids) = container_geometry(project, container)?;
    let node_size = estimated_node_size(project, node_id);
    let dependency_anchor = dependencies
        .iter()
        .filter_map(|dependency_id| project.get_node(*dependency_id))
        .map(|dependency| dependency.ui_position)
        .collect::<Vec<_>>();
    let mut anchor = if dependency_anchor.is_empty() {
        desired
    } else {
        let count = dependency_anchor.len() as f32;
        egui::pos2(
            dependency_anchor
                .iter()
                .map(|position| position[0])
                .sum::<f32>()
                / count
                + estimated_node_width()
                + AUTO_LAYOUT_COLUMN_GAP,
            dependency_anchor
                .iter()
                .map(|position| position[1])
                .sum::<f32>()
                / count,
        )
    };
    let min = egui::pos2(
        container_position[0] + AUTO_LAYOUT_TRACK_LEFT,
        container_position[1]
            + match container {
                NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_TOP,
                NodeContainer::Track(_) => AUTO_LAYOUT_TRACK_TOP,
                NodeContainer::Clip(_) => AUTO_LAYOUT_CLIP_TOP,
            },
    );
    let max_x = (container_position[0] + container_size[0] - AUTO_LAYOUT_TRACK_RIGHT - node_size.x)
        .max(min.x);
    // A dependency-derived position expresses graph order, so preserve it and
    // let `ensure_container_hierarchy_contains` grow the owning containers.
    // Clamping it to a container that is still sized for the old children can
    // place the new dependent Node to the *left* of its source. Pointer-based
    // placement, on the other hand, should remain within the current bounds.
    anchor.x = if dependency_anchor.is_empty() {
        anchor.x.clamp(min.x, max_x)
    } else {
        anchor.x.max(min.x)
    };
    anchor.y = anchor.y.max(min.y);

    let mut occupied = node_ids
        .iter()
        .filter(|child_id| **child_id != node_id)
        .filter_map(|child_id| {
            let child = project.get_node(*child_id)?;
            Some(egui::Rect::from_min_size(
                egui::pos2(child.ui_position[0], child.ui_position[1]),
                estimated_node_size(project, *child_id),
            ))
        })
        .collect::<Vec<_>>();
    occupied.extend(immediate_child_rects(
        project,
        &AutoLayoutPlan::default(),
        container,
    ));
    let mut candidate = egui::Rect::from_min_size(anchor, node_size);
    loop {
        let next_y = occupied
            .iter()
            .filter(|other| candidate.expand(4.0).intersects(**other))
            .map(|other| other.bottom() + AUTO_LAYOUT_ROW_GAP)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            break;
        };
        candidate = egui::Rect::from_min_size(egui::pos2(anchor.x, next_y), node_size);
    }

    project.get_node_mut(node_id)?.ui_position = [candidate.min.x, candidate.min.y];
    ensure_container_hierarchy_contains(project, container, candidate);
    Some(candidate.min)
}

fn container_geometry(
    project: &Project,
    container: NodeContainer,
) -> Option<([f32; 2], [f32; 2], Vec<Uuid>)> {
    match container {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|item| (item.ui_position, item.ui_size, item.node_ids.clone())),
        NodeContainer::Track(id) => project
            .get_track(id)
            .map(|item| (item.ui_position, item.ui_size, item.node_ids.clone())),
        NodeContainer::Clip(id) => project
            .get_clip(id)
            .map(|item| (item.ui_position, item.ui_size, item.node_ids.clone())),
    }
}

/// Resolves the deepest visible container chrome for pointer-only creation and
/// insertion callers. Those callers intentionally accept headers. Geometry
/// reparenting uses `reparent_container_geometries` and content-only target
/// evaluation instead, so neither a collapsed header nor its stored body can
/// acquire an existing Node.
fn node_container_at_position(
    project: &Project,
    composition_id: Uuid,
    position: egui::Pos2,
) -> Option<NodeContainer> {
    let composition = project.get_composition(composition_id)?;
    if composition.ui_collapsed {
        return Some(NodeContainer::Composition(composition_id));
    }
    for track_id in composition.track_ids.iter().rev() {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        if !container_visual(project, PortOwner::Track(*track_id))
            .is_some_and(|visual| visual.rect().contains(position))
        {
            continue;
        }
        if !track.ui_collapsed {
            for clip_id in track.clip_ids.iter().rev() {
                if container_visual(project, PortOwner::Clip(*clip_id))
                    .is_some_and(|visual| visual.rect().contains(position))
                {
                    return Some(NodeContainer::Clip(*clip_id));
                }
            }
        }
        return Some(NodeContainer::Track(*track_id));
    }
    Some(NodeContainer::Composition(composition_id))
}

#[derive(Clone, Copy, Debug)]
struct ReparentContainerGeometry {
    container: NodeContainer,
    visible_rect: egui::Rect,
    content_rect: Option<egui::Rect>,
    depth: u8,
    stacking_order: usize,
    collapsed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReparentTargetKind {
    Geometry,
    RetainedOrigin,
    RootFallback,
}

impl ReparentTargetKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Geometry => "geometry",
            Self::RetainedOrigin => "retained_origin",
            Self::RootFallback => "root_fallback",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ReparentTargetEvaluation {
    container: NodeContainer,
    visible_rect: egui::Rect,
    content_rect: egui::Rect,
    depth: u8,
    stacking_order: usize,
    overlap_ratio: f32,
    center_inside: bool,
    pointer_inside: bool,
    root_fallback: bool,
    score: f32,
    kind: ReparentTargetKind,
}

#[derive(Clone, Copy, Debug)]
struct NodeDropIntent {
    node_id: Uuid,
    final_rect: egui::Rect,
    target: ReparentTargetEvaluation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReparentReleaseOutcome {
    NoIntent,
    Applied,
    Rejected,
}

fn reparent_container_geometries(
    project: &Project,
    composition_id: Uuid,
) -> Vec<ReparentContainerGeometry> {
    let Some(composition) = project.get_composition(composition_id) else {
        return Vec::new();
    };
    let composition_rect = container_rect(composition.ui_position, composition.ui_size);
    let mut geometries = vec![ReparentContainerGeometry {
        container: NodeContainer::Composition(composition_id),
        visible_rect: if composition.ui_collapsed {
            egui::Rect::from_min_size(
                composition_rect.min,
                egui::vec2(composition_rect.width(), CONTAINER_HEADER_HEIGHT),
            )
        } else {
            composition_rect
        },
        content_rect: (!composition.ui_collapsed)
            .then(|| composition_content_rect(composition_rect)),
        depth: 0,
        stacking_order: 0,
        collapsed: composition.ui_collapsed,
    }];
    if composition.ui_collapsed {
        return geometries;
    }

    let mut stacking_order = 1;
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let track_rect = container_rect(track.ui_position, track.ui_size);
        let visible_rect = if track.ui_collapsed {
            egui::Rect::from_min_size(
                track_rect.min,
                egui::vec2(track_rect.width(), CONTAINER_HEADER_HEIGHT),
            )
        } else {
            track_rect
        };
        geometries.push(ReparentContainerGeometry {
            container: NodeContainer::Track(*track_id),
            visible_rect,
            content_rect: (!track.ui_collapsed)
                .then(|| nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP)),
            depth: 1,
            stacking_order,
            collapsed: track.ui_collapsed,
        });
        stacking_order += 1;
        if track.ui_collapsed {
            continue;
        }
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            let clip_rect = container_rect(clip.ui_position, clip.ui_size);
            let visible_rect = if clip.ui_collapsed {
                egui::Rect::from_min_size(
                    clip_rect.min,
                    egui::vec2(clip_rect.width(), CONTAINER_HEADER_HEIGHT),
                )
            } else {
                clip_rect
            };
            geometries.push(ReparentContainerGeometry {
                container: NodeContainer::Clip(*clip_id),
                visible_rect,
                content_rect: (!clip.ui_collapsed)
                    .then(|| nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP)),
                depth: 2,
                stacking_order,
                collapsed: clip.ui_collapsed,
            });
            stacking_order += 1;
        }
    }
    geometries
}

fn rect_area(rect: egui::Rect) -> f32 {
    if rect.is_positive() {
        rect.width() * rect.height()
    } else {
        0.0
    }
}

fn overlap_ratio(outer: egui::Rect, inner: egui::Rect) -> f32 {
    let inner_area = rect_area(inner);
    if inner_area <= f32::EPSILON {
        return 0.0;
    }
    (rect_area(outer.intersect(inner)) / inner_area).clamp(0.0, 1.0)
}

fn evaluate_reparent_target(
    geometry: ReparentContainerGeometry,
    node_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> Option<ReparentTargetEvaluation> {
    let content_rect = geometry.content_rect?;
    if !content_rect.is_positive() || !node_rect.is_positive() {
        return None;
    }
    let overlap_ratio = overlap_ratio(content_rect, node_rect);
    let center_inside = content_rect.contains(node_rect.center());
    let pointer_inside = content_rect.contains(drop_point);
    let eligible =
        center_inside || pointer_inside && overlap_ratio >= NODE_REPARENT_POINTER_OVERLAP_THRESHOLD;
    if !eligible {
        return None;
    }
    let score = f32::from(geometry.depth) * 10_000.0
        + if center_inside { 1_000.0 } else { 0.0 }
        + if pointer_inside { 500.0 } else { 0.0 }
        + overlap_ratio * 100.0
        + geometry.stacking_order as f32 * 0.001;
    Some(ReparentTargetEvaluation {
        container: geometry.container,
        visible_rect: geometry.visible_rect,
        content_rect,
        depth: geometry.depth,
        stacking_order: geometry.stacking_order,
        overlap_ratio,
        center_inside,
        pointer_inside,
        root_fallback: false,
        score,
        kind: ReparentTargetKind::Geometry,
    })
}

fn deepest_legal_reparent_target(
    project: &Project,
    composition_id: Uuid,
    node_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> Option<ReparentTargetEvaluation> {
    let geometries = reparent_container_geometries(project, composition_id);
    let selected = geometries
        .iter()
        .copied()
        .filter_map(|geometry| evaluate_reparent_target(geometry, node_rect, drop_point))
        .max_by(|left, right| {
            left.depth
                .cmp(&right.depth)
                .then_with(|| left.center_inside.cmp(&right.center_inside))
                .then_with(|| left.pointer_inside.cmp(&right.pointer_inside))
                .then_with(|| left.overlap_ratio.total_cmp(&right.overlap_ratio))
                .then_with(|| left.stacking_order.cmp(&right.stacking_order))
        });
    if selected.is_some() {
        return selected;
    }

    // The Composition is the root graph owner. A deliberate drop beyond its
    // current bounds remains legal; the atomic reparent step expands the root
    // on all four sides so visual containment catches up without moving the
    // Node. Nested containers never receive this fallback.
    let root = geometries
        .into_iter()
        .find(|geometry| geometry.container == NodeContainer::Composition(composition_id))?;
    let content_rect = root.content_rect?;
    Some(ReparentTargetEvaluation {
        container: root.container,
        visible_rect: root.visible_rect,
        content_rect,
        depth: root.depth,
        stacking_order: root.stacking_order,
        overlap_ratio: overlap_ratio(content_rect, node_rect),
        center_inside: false,
        pointer_inside: false,
        root_fallback: true,
        score: -1.0,
        kind: ReparentTargetKind::RootFallback,
    })
}

fn node_drop_intents(
    project: &Project,
    composition_id: Uuid,
    gesture: &NodeEditorReparentGesture,
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    final_positions: &HashMap<Uuid, [f32; 2]>,
    drop_point: egui::Pos2,
    canvas_scale: f32,
) -> Vec<NodeDropIntent> {
    let scale = sanitized_node_editor_scale(canvas_scale);
    let mut node_ids = gesture.origins.keys().copied().collect::<Vec<_>>();
    node_ids.sort_unstable();
    let Some(primary_node_id) = gesture.primary_node_id else {
        return Vec::new();
    };
    let Some(primary_origin) = gesture.origins.get(&primary_node_id) else {
        return Vec::new();
    };
    let Some(primary_final_position) =
        final_positions.get(&primary_node_id).copied().or_else(|| {
            project
                .get_node(primary_node_id)
                .map(|node| node.ui_position)
        })
    else {
        return Vec::new();
    };
    let displacement = egui::vec2(
        primary_final_position[0] - primary_origin.position[0],
        primary_final_position[1] - primary_origin.position[1],
    );
    let Some(primary_rect) = rendered_node_rects.get(&primary_node_id).copied() else {
        return Vec::new();
    };
    if displacement.length() * scale < NODE_REPARENT_DRAG_THRESHOLD {
        return retained_origin_intents(
            project,
            composition_id,
            gesture,
            rendered_node_rects,
            drop_point,
        );
    }
    let Some(resolved_target) =
        deepest_legal_reparent_target(project, composition_id, primary_rect, drop_point)
    else {
        return Vec::new();
    };
    if resolved_target.container == primary_origin.container {
        if !resolved_target.root_fallback {
            return retained_origin_intents(
                project,
                composition_id,
                gesture,
                rendered_node_rects,
                drop_point,
            );
        }
    } else if !node_has_clearly_exited_origin(
        project,
        composition_id,
        primary_origin.container,
        resolved_target.container,
        primary_rect,
        drop_point,
    ) {
        // Header/padding hysteresis retains canonical ownership, but the Node
        // still has an exact final position. Grow the old owner around that
        // position atomically so a later auto-layout pass cannot move it.
        return retained_origin_intents(
            project,
            composition_id,
            gesture,
            rendered_node_rects,
            drop_point,
        );
    }

    // A multi-selected drag is one semantic move. Resolve ownership from the
    // physically grabbed Node, then keep every selected Node's relative
    // layout by applying that same target in one candidate transaction.
    node_ids
        .into_iter()
        .filter_map(|node_id| {
            rendered_node_rects
                .get(&node_id)
                .copied()
                .map(|final_rect| NodeDropIntent {
                    node_id,
                    final_rect,
                    target: resolved_target,
                })
        })
        .collect()
}

fn retained_origin_intents(
    project: &Project,
    composition_id: Uuid,
    gesture: &NodeEditorReparentGesture,
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    drop_point: egui::Pos2,
) -> Vec<NodeDropIntent> {
    let mut node_ids = gesture.origins.keys().copied().collect::<Vec<_>>();
    node_ids.sort_unstable();
    node_ids
        .into_iter()
        .filter_map(|node_id| {
            let origin = gesture.origins.get(&node_id)?.container;
            let final_rect = rendered_node_rects.get(&node_id).copied()?;
            let target = retained_origin_reparent_target(
                project,
                composition_id,
                origin,
                final_rect,
                drop_point,
            )?;
            (!rect_contains_rect(target.content_rect, final_rect)).then_some(NodeDropIntent {
                node_id,
                final_rect,
                target,
            })
        })
        .collect()
}

fn retained_origin_reparent_target(
    project: &Project,
    composition_id: Uuid,
    origin: NodeContainer,
    node_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> Option<ReparentTargetEvaluation> {
    let geometry = reparent_container_geometries(project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == origin)?;
    let content_rect = geometry.content_rect.unwrap_or(geometry.visible_rect);
    Some(ReparentTargetEvaluation {
        container: origin,
        visible_rect: geometry.visible_rect,
        content_rect,
        depth: geometry.depth,
        stacking_order: geometry.stacking_order,
        overlap_ratio: overlap_ratio(content_rect, node_rect),
        center_inside: content_rect.contains(node_rect.center()),
        pointer_inside: content_rect.contains(drop_point),
        root_fallback: false,
        score: -0.5,
        kind: ReparentTargetKind::RetainedOrigin,
    })
}

fn target_is_deeper_within_origin(
    project: &Project,
    origin: NodeContainer,
    target: NodeContainer,
) -> bool {
    match (origin, target) {
        (NodeContainer::Composition(composition_id), NodeContainer::Track(track_id)) => {
            project.find_composition_for_track(track_id) == Some(composition_id)
        }
        (NodeContainer::Composition(composition_id), NodeContainer::Clip(clip_id)) => {
            project
                .find_track_for_clip(clip_id)
                .and_then(|track_id| project.find_composition_for_track(track_id))
                == Some(composition_id)
        }
        (NodeContainer::Track(track_id), NodeContainer::Clip(clip_id)) => {
            project.find_track_for_clip(clip_id) == Some(track_id)
        }
        _ => false,
    }
}

fn target_is_authoritative_sibling(
    project: &Project,
    composition_id: Uuid,
    origin: NodeContainer,
    target: NodeContainer,
    drop_point: egui::Pos2,
) -> bool {
    let origin_parent = parent_container_owner(project, port_owner_for_node_container(origin));
    let target_parent = parent_container_owner(project, port_owner_for_node_container(target));
    if origin == target || origin_parent.is_none() || origin_parent != target_parent {
        return false;
    }
    reparent_container_geometries(project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == target)
        .is_some_and(|geometry| {
            geometry
                .content_rect
                .unwrap_or(geometry.visible_rect)
                .contains(drop_point)
        })
}

fn node_has_clearly_exited_origin(
    project: &Project,
    composition_id: Uuid,
    origin: NodeContainer,
    target: NodeContainer,
    final_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> bool {
    if target_is_deeper_within_origin(project, origin, target)
        || target_is_authoritative_sibling(project, composition_id, origin, target, drop_point)
    {
        return true;
    }
    let origin_geometry = reparent_container_geometries(project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == origin);
    origin_geometry.is_some_and(|geometry| {
        !geometry.visible_rect.intersects(final_rect) && !geometry.visible_rect.contains(drop_point)
    })
}

fn record_node_reparent_origins(
    project: &Project,
    layout_edits: &[LayoutEdit],
    state: &mut NodeEditorState,
    gesture_allowed: bool,
) {
    if !gesture_allowed {
        return;
    }
    for edit in layout_edits {
        let LayoutEdit::MoveNode { node_id, .. } = edit else {
            continue;
        };
        let Some(node) = project.get_node(*node_id) else {
            continue;
        };
        let Some(container) = project.find_node_container(*node_id) else {
            continue;
        };
        state.moved_node_ids.insert(*node_id);
        let gesture = state
            .node_reparent
            .get_or_insert_with(|| NodeEditorReparentGesture {
                origins: HashMap::new(),
                primary_node_id: None,
                hovered_target: None,
                hovered_node_id: None,
                hovered_score: None,
            });
        gesture
            .origins
            .entry(*node_id)
            .or_insert(NodeEditorNodeDragOrigin {
                container,
                position: node.ui_position,
            });
    }
}

fn final_node_positions(
    project: &Project,
    gesture: &NodeEditorReparentGesture,
    layout_edits: &[LayoutEdit],
) -> HashMap<Uuid, [f32; 2]> {
    let mut positions = gesture
        .origins
        .keys()
        .filter_map(|node_id| {
            project
                .get_node(*node_id)
                .map(|node| (*node_id, node.ui_position))
        })
        .collect::<HashMap<_, _>>();
    for edit in layout_edits {
        if let LayoutEdit::MoveNode { node_id, position } = edit {
            if positions.contains_key(node_id) {
                positions.insert(*node_id, *position);
            }
        }
    }
    positions
}

fn primary_node_drop_intent(
    intents: &[NodeDropIntent],
    drop_point: egui::Pos2,
) -> Option<NodeDropIntent> {
    intents
        .iter()
        .copied()
        .find(|intent| intent.final_rect.contains(drop_point))
        .or_else(|| intents.first().copied())
}

fn reparent_ineligible_reason(geometry: ReparentContainerGeometry) -> Option<&'static str> {
    geometry.collapsed.then_some("collapsed_hidden_content")
}

fn reported_reparent_target(
    evaluation: Option<ReparentTargetEvaluation>,
    selected: bool,
    active: NodeDropIntent,
) -> Option<ReparentTargetEvaluation> {
    if selected {
        Some(active.target)
    } else {
        evaluation
    }
}

fn register_reparent_drop_targets(
    project: &Project,
    composition_id: Uuid,
    active: NodeDropIntent,
    drop_point: egui::Pos2,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    painter: &egui::Painter,
) {
    for geometry in reparent_container_geometries(project, composition_id) {
        let evaluation = evaluate_reparent_target(geometry, active.final_rect, drop_point);
        let selected = active.target.container == geometry.container;
        let reported = reported_reparent_target(evaluation, selected, active);
        let graph_rect = geometry.content_rect.unwrap_or(geometry.visible_rect);
        let unclipped_rect = to_global * graph_rect;
        let rect = clipped_qa_rect(unclipped_rect, canvas_clip);
        let owner = qa_container_key(port_owner_for_node_container(geometry.container));
        let (overlap, center_inside, pointer_inside, score) =
            reported.map_or((0.0, false, false, None), |evaluation| {
                (
                    evaluation.overlap_ratio,
                    evaluation.center_inside,
                    evaluation.pointer_inside,
                    Some(evaluation.score),
                )
            });
        crate::qa::register_component_with_metadata(
            format!("node_editor.reparent_target.{owner}"),
            "node_reparent_target",
            rect,
            reported.is_some(),
            Some(serde_json::json!({
                "owner": owner,
                "active_node_id": active.node_id,
                "selected": selected,
                "eligible": reported.is_some(),
                "geometry_eligible": evaluation.is_some(),
                "collapsed": geometry.collapsed,
                "ineligible_reason": reparent_ineligible_reason(geometry),
                "selection_kind": selected.then_some(active.target.kind.as_str()),
                "depth": geometry.depth,
                "stacking_order": geometry.stacking_order,
                "overlap_ratio": overlap,
                "center_inside": center_inside,
                "pointer_inside": pointer_inside,
                "root_fallback": selected && active.target.root_fallback,
                "score": score,
                "drop_point": {"x": drop_point.x, "y": drop_point.y},
                "node_rect": qa_rect_metadata(active.final_rect),
                "content_rect": geometry.content_rect.map(qa_rect_metadata),
                "drop_target_rect": qa_rect_metadata(graph_rect),
                "unclipped_rect": qa_rect_metadata(unclipped_rect),
                "visible_in_canvas": rect.is_positive(),
            })),
        );
    }

    let highlight = if active.target.root_fallback {
        active.target.visible_rect
    } else {
        active.target.content_rect
    };
    painter.rect_filled(
        highlight,
        egui::CornerRadius::same(8),
        Color32::from_rgba_premultiplied(78, 190, 128, 22),
    );
    painter.rect_stroke(
        highlight,
        egui::CornerRadius::same(8),
        egui::Stroke::new(
            screen_stroke_in_graph_units(2.0, to_global.scaling),
            Color32::from_rgb(94, 221, 151),
        ),
        egui::StrokeKind::Inside,
    );
}

fn reparent_nodes_from_intents(project: &mut Project, intents: &[NodeDropIntent]) -> bool {
    let mut candidate = project.clone();
    let mut changed = false;
    for intent in intents {
        let node_id = intent.node_id;
        let destination = intent.target.container;
        let containment_rect = estimated_node_rect(&candidate, node_id)
            .map_or(intent.final_rect, |estimated| {
                estimated.union(intent.final_rect)
            });
        if candidate.find_node_container(node_id) == Some(destination) {
            changed |=
                ensure_reparent_hierarchy_contains(&mut candidate, destination, containment_rect);
            continue;
        }
        match candidate.attach_node_to_container(destination, node_id) {
            Ok(()) => {
                ensure_reparent_hierarchy_contains(&mut candidate, destination, containment_rect);
                changed = true;
            }
            Err(error) => {
                log::warn!("Cannot move Node {node_id} to {destination:?}: {error}");
                return false;
            }
        }
    }
    if changed {
        *project = candidate;
    }
    changed
}

fn finish_node_reparent(
    project: &mut Project,
    intents: &[NodeDropIntent],
    gesture: Option<&NodeEditorReparentGesture>,
) -> ReparentReleaseOutcome {
    if intents.is_empty() {
        return ReparentReleaseOutcome::NoIntent;
    }
    let mut baseline = project.clone();
    restore_reparent_origins(&mut baseline, gesture);
    let mut candidate = project.clone();
    if reparent_nodes_from_intents(&mut candidate, intents) {
        let composition_ids = intents
            .iter()
            .filter_map(|intent| {
                port_owner_composition(
                    &candidate,
                    port_owner_for_node_container(intent.target.container),
                )
            })
            .collect::<HashSet<_>>();
        let introduces_invalid_layout = composition_ids.into_iter().any(|composition_id| {
            !container_hierarchy_needs_reflow(&baseline, composition_id)
                && container_hierarchy_needs_reflow(&candidate, composition_id)
        });
        if !introduces_invalid_layout {
            *project = candidate;
            return ReparentReleaseOutcome::Applied;
        }
        log::warn!("Rejected Node reparent because it would require automatic layout repair");
    }

    // The live graph follows the pointer during the drag. If the semantic
    // candidate is rejected, roll every participating Node back together so
    // a half-applied layout cannot remain under the old owner or splice a wire.
    restore_reparent_origins(project, gesture);
    ReparentReleaseOutcome::Rejected
}

fn restore_reparent_origins(project: &mut Project, gesture: Option<&NodeEditorReparentGesture>) {
    if let Some(gesture) = gesture {
        for (node_id, origin) in &gesture.origins {
            if let Some(node) = project.get_node_mut(*node_id) {
                node.ui_position = origin.position;
            }
        }
    }
}

fn splice_node_for_release(
    outcome: ReparentReleaseOutcome,
    moved_node_ids: &HashSet<Uuid>,
    captured_primary_node_id: Option<Uuid>,
) -> Option<Uuid> {
    if outcome != ReparentReleaseOutcome::NoIntent || moved_node_ids.len() != 1 {
        return None;
    }
    let node_id = moved_node_ids.iter().next().copied()?;
    (captured_primary_node_id == Some(node_id)).then_some(node_id)
}

#[cfg(test)]
fn reparent_test_node_at_drop(
    project: &mut Project,
    composition_id: Uuid,
    node_id: Uuid,
    drop_point: egui::Pos2,
) -> bool {
    let node_size = estimated_node_size(project, node_id);
    let final_rect = egui::Rect::from_center_size(drop_point, node_size);
    let Some(node) = project.get_node_mut(node_id) else {
        return false;
    };
    node.ui_position = [final_rect.min.x, final_rect.min.y];
    let Some(target) =
        deepest_legal_reparent_target(project, composition_id, final_rect, drop_point)
    else {
        return false;
    };
    reparent_nodes_from_intents(
        project,
        &[NodeDropIntent {
            node_id,
            final_rect,
            target,
        }],
    )
}

fn container_rect(position: [f32; 2], size: [f32; 2]) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(position[0], position[1]),
        egui::vec2(
            size[0].max(MIN_CONTAINER_SIZE.x),
            size[1].max(MIN_CONTAINER_SIZE.y),
        ),
    )
}

fn ensure_container_hierarchy_contains(
    project: &mut Project,
    container: NodeContainer,
    node_rect: egui::Rect,
) -> bool {
    let owner = port_owner_for_node_container(container);
    let mut changed = grow_container_to_rect(project, owner, node_rect);
    changed |= ensure_structural_merge_layout(project, container);

    // Creation and auto-layout clamp their leaf Node to the owning container's
    // minimum content edge. Preserve existing container origins here and grow
    // only the far edges, otherwise adding an item can move an entire existing
    // hierarchy merely because legacy child chrome sits inside its margin.
    let parent_track = match container {
        NodeContainer::Clip(clip_id) => project.find_track_for_clip(clip_id),
        NodeContainer::Track(track_id) => Some(track_id),
        NodeContainer::Composition(_) => None,
    };
    if let Some(track_id) = parent_track {
        if let NodeContainer::Clip(clip_id) = container {
            if let Some(clip) = project.get_clip(clip_id) {
                let clip_rect = container_rect(clip.ui_position, clip.ui_size);
                changed |= grow_container_to_rect(project, PortOwner::Track(track_id), clip_rect);
            }
            changed |= ensure_structural_merge_layout(project, NodeContainer::Track(track_id));
        }
        if let Some(composition_id) = project.find_composition_for_track(track_id) {
            if let Some(track) = project.get_track(track_id) {
                let track_rect = container_rect(track.ui_position, track.ui_size);
                changed |= grow_container_to_rect(
                    project,
                    PortOwner::Composition(composition_id),
                    track_rect,
                );
            }
            changed |=
                ensure_structural_merge_layout(project, NodeContainer::Composition(composition_id));
        }
    }
    changed
}

fn ensure_reparent_hierarchy_contains(
    project: &mut Project,
    container: NodeContainer,
    node_rect: egui::Rect,
) -> bool {
    let owner = port_owner_for_node_container(container);
    let mut changed = grow_container_to_rect_all_edges(project, owner, node_rect);
    changed |= ensure_structural_merge_layout(project, container);

    // Propagate each *updated child container rectangle*, not only the Node.
    // Expanding the min edge intentionally changes only container chrome;
    // child Nodes keep their absolute graph coordinates and therefore their
    // exact drop position.
    let mut child_owner = owner;
    while let Some(parent_owner) = parent_container_owner(project, child_owner) {
        let Some(child) = container_visual(project, child_owner) else {
            break;
        };
        let child_rect = container_rect(child.position, child.size);
        changed |= grow_container_to_rect_all_edges(project, parent_owner, child_rect);
        if let Some(parent_container) = node_container_for_port_owner(parent_owner) {
            changed |= ensure_structural_merge_layout(project, parent_container);
        }
        child_owner = parent_owner;
    }
    changed
}

fn node_container_for_port_owner(owner: PortOwner) -> Option<NodeContainer> {
    match owner {
        PortOwner::Composition(id) => Some(NodeContainer::Composition(id)),
        PortOwner::Track(id) => Some(NodeContainer::Track(id)),
        PortOwner::Clip(id) => Some(NodeContainer::Clip(id)),
        PortOwner::Node(_) => None,
    }
}

fn grow_container_to_rect(project: &mut Project, owner: PortOwner, rect: egui::Rect) -> bool {
    let Some(visual) = container_visual(project, owner) else {
        return false;
    };
    let (right_margin, bottom_margin) = match owner {
        PortOwner::Composition(_) => (
            AUTO_LAYOUT_COMPOSITION_RIGHT,
            AUTO_LAYOUT_COMPOSITION_BOTTOM,
        ),
        PortOwner::Track(_) | PortOwner::Clip(_) => {
            (AUTO_LAYOUT_TRACK_RIGHT, AUTO_LAYOUT_TRACK_BOTTOM)
        }
        PortOwner::Node(_) => return false,
    };
    let size = [
        visual.size[0].max(rect.right() - visual.position[0] + right_margin),
        visual.size[1].max(rect.bottom() - visual.position[1] + bottom_margin),
    ];
    set_container_size(project, owner, size)
}

fn grow_container_to_rect_all_edges(
    project: &mut Project,
    owner: PortOwner,
    rect: egui::Rect,
) -> bool {
    let Some(visual) = container_visual(project, owner) else {
        return false;
    };
    let (left_margin, top_margin, right_margin, bottom_margin) = match owner {
        PortOwner::Composition(_) => (
            AUTO_LAYOUT_COMPOSITION_LEFT,
            AUTO_LAYOUT_COMPOSITION_TOP,
            AUTO_LAYOUT_COMPOSITION_RIGHT,
            AUTO_LAYOUT_COMPOSITION_BOTTOM,
        ),
        PortOwner::Track(_) => (
            AUTO_LAYOUT_TRACK_LEFT,
            AUTO_LAYOUT_TRACK_TOP,
            AUTO_LAYOUT_TRACK_RIGHT,
            AUTO_LAYOUT_TRACK_BOTTOM,
        ),
        PortOwner::Clip(_) => (
            AUTO_LAYOUT_TRACK_LEFT,
            AUTO_LAYOUT_CLIP_TOP,
            AUTO_LAYOUT_TRACK_RIGHT,
            AUTO_LAYOUT_TRACK_BOTTOM,
        ),
        PortOwner::Node(_) => return false,
    };
    let old_rect = container_rect(visual.position, visual.size);
    let left = old_rect.left().min(rect.left() - left_margin);
    let top = old_rect.top().min(rect.top() - top_margin);
    let right = old_rect.right().max(rect.right() + right_margin);
    let bottom = old_rect.bottom().max(rect.bottom() + bottom_margin);
    let position = [left, top];
    let size = [
        (right - left).max(MIN_CONTAINER_SIZE.x),
        (bottom - top).max(MIN_CONTAINER_SIZE.y),
    ];
    set_container_geometry(project, owner, position, size)
}

fn create_clip_at_free_slot(
    project: &mut Project,
    desired: egui::Pos2,
    composition_id: Uuid,
    name: &str,
) -> Option<Uuid> {
    let composition = project.get_composition(composition_id)?;
    let track_id = composition
        .track_ids
        .iter()
        .rev()
        .find(|track_id| {
            project.get_track(**track_id).is_some_and(|track| {
                container_rect(track.ui_position, track.ui_size).contains(desired)
            })
        })
        .copied()
        .or_else(|| composition.track_ids.first().copied())?;
    let track = project.get_track(track_id)?.clone();
    let mut clip = library::model::Clip::new(name, 0.0, 5.0);
    let size = egui::vec2(clip.ui_size[0], clip.ui_size[1]);
    let min = egui::pos2(
        track.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
        track.ui_position[1] + AUTO_LAYOUT_TRACK_TOP,
    );
    let mut candidate =
        egui::Rect::from_min_size(egui::pos2(desired.x.max(min.x), desired.y.max(min.y)), size);
    for existing_id in &track.clip_ids {
        let Some(existing) = project.get_clip(*existing_id) else {
            continue;
        };
        let existing_rect = container_rect(existing.ui_position, existing.ui_size);
        if candidate.intersects(existing_rect) {
            candidate = candidate.translate(egui::vec2(
                0.0,
                existing_rect.bottom() - candidate.top() + AUTO_LAYOUT_ROW_GAP,
            ));
        }
    }
    clip.ui_position = [candidate.min.x, candidate.min.y];
    let clip_id = clip.id;
    project.add_clip(clip);
    if let Err(error) = project.attach_clip_to_track(track_id, clip_id) {
        project.remove_clip(clip_id);
        log::warn!("Cannot add Clip to Track: {error}");
        return None;
    }
    ensure_container_hierarchy_contains(project, NodeContainer::Track(track_id), candidate);
    Some(clip_id)
}

fn create_track_at_free_slot(
    project: &mut Project,
    desired: egui::Pos2,
    composition_id: Uuid,
    name: &str,
) -> Option<Uuid> {
    let composition = project.get_composition(composition_id)?.clone();
    let mut track = library::model::Track::new(name);
    let size = egui::vec2(track.ui_size[0], track.ui_size[1]);
    let min = egui::pos2(
        composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_LEFT,
        composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP,
    );
    let mut candidate =
        egui::Rect::from_min_size(egui::pos2(desired.x.max(min.x), desired.y.max(min.y)), size);
    let occupied = composition
        .track_ids
        .iter()
        .filter_map(|track_id| project.get_track(*track_id))
        .map(|track| {
            egui::Rect::from_min_size(
                egui::pos2(track.ui_position[0], track.ui_position[1]),
                egui::vec2(track.ui_size[0], track.ui_size[1]),
            )
        })
        .collect::<Vec<_>>();
    loop {
        let next_y = occupied
            .iter()
            .filter(|other| candidate.expand(8.0).intersects(**other))
            .map(|other| other.bottom() + AUTO_LAYOUT_TRACK_GAP)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            break;
        };
        candidate = egui::Rect::from_min_size(egui::pos2(candidate.min.x, next_y), size);
    }
    track.ui_position = [candidate.min.x, candidate.min.y];
    let track_id = track.id;
    if let Err(error) = project.add_track(track) {
        log::warn!("Cannot add Track to project: {error}");
        return None;
    }
    if let Err(error) = project.attach_track_to_composition(composition_id, track_id) {
        project.remove_track(track_id);
        log::warn!("Cannot add track to composition: {error}");
        return None;
    }
    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_size[0] = composition.ui_size[0]
            .max(candidate.right() - composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_RIGHT);
        composition.ui_size[1] = composition.ui_size[1]
            .max(candidate.bottom() - composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_BOTTOM);
    }
    Some(track_id)
}

#[cfg(test)]
mod root_tests;
