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
    adaptive_grid_spacing, node_editor_canvas_metadata, node_editor_details_visible,
    node_editor_port_interactions_enabled, node_editor_resize_interactions_enabled,
    node_editor_snarl_style, sanitize_node_editor_transform, sanitized_node_editor_scale,
    screen_stroke_in_graph_units,
};
#[cfg(test)]
use canvas::{
    GRID_TARGET_SCREEN_SPACING, NODE_EDITOR_DETAIL_SCALE, NODE_EDITOR_MAX_SCALE,
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
    connection_supports_authored_blend, merge_images_target_node_id,
    merge_input_index_for_connection, merge_input_slots, merge_layer_rows,
    register_merge_layer_component, wire_order_menu_state, wire_order_menu_states,
    wire_order_qa_metadata, MergeInputSlot, MergeInputSlotRole,
};
use graph_build::{build_snarl, container_visual};
use interaction::show_wire_context_menu;
#[cfg(test)]
use layout::collect_layout_edits;
#[cfg(test)]
use layout::padded_intersection;
use layout::{apply_auto_layout, set_container_geometry, set_container_size, translate_container};
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
        NodeCreateRequest::Transform => {
            match plugin_manager.create_shape_transform_operation_node() {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Transform Node: {error}");
                    None
                }
            }
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
        project.remove_node(node_id);
        false
    }
}

fn create_composition_node(project: &mut Project, position: egui::Pos2, comp_id: Uuid) -> bool {
    let mut candidate = project.clone();
    let (composition, root) =
        library::model::Composition::new("Nested Comp", 1920, 1080, 30.0, 10.0);
    let nested_id = composition.id;
    candidate.add_track(root);
    candidate.add_composition(composition);

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
        child_owner = parent_owner;
    }
    changed
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
    project.add_track(track);
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
mod tests {
    use super::test_fixture::fixture;
    use super::*;
    use crate::test_support::generator_node;
    use library::animation::EasingFunction;
    use library::editor::project_service::GeneratorNodeRequest;
    use library::model::frame::color::Color;
    use library::model::frame::draw_type::DrawStyle;
    use library::model::frame::entity::StyleConfig;
    use library::model::project::{
        ProjectConnection, FMOD_X_INPUT_PORT, FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT,
        MERGE_IMAGES_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
    };
    use library::model::property::{Keyframe, Property, PropertyMap};
    use library::model::Composition;
    use library::plugin::{
        FrameEvaluationContext, OperationDescriptor, OperationDescriptorError, Plugin, StylePlugin,
    };

    #[test]
    fn typed_port_owner_composition_ignores_same_uuid_clip() {
        let shared_id = Uuid::new_v4();
        let mut project = Project::new("typed owner composition");

        let (clip_composition, clip_track) =
            Composition::new("Clip composition", 1920, 1080, 30.0, 10.0);
        let clip_composition_id = clip_composition.id;
        let clip_track_id = clip_track.id;
        let mut collision = Clip::new("same UUID Clip", 0.0, 5.0);
        collision.id = shared_id;
        project.add_track(clip_track);
        project.add_composition(clip_composition);
        project.add_clip(collision);
        project
            .attach_clip_to_track(clip_track_id, shared_id)
            .unwrap();

        let (node_composition, node_track) =
            Composition::new("Node composition", 1920, 1080, 30.0, 10.0);
        let node_composition_id = node_composition.id;
        let mut node = Node::new_merge("same UUID Node");
        node.id = shared_id;
        project.add_track(node_track);
        project.add_composition(node_composition);
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Composition(node_composition_id), shared_id)
            .unwrap();

        assert_eq!(
            project.find_containing_composition(shared_id),
            Some(clip_composition_id)
        );
        assert_eq!(
            port_owner_composition(&project, PortOwner::Node(shared_id)),
            Some(node_composition_id)
        );
        assert_eq!(
            port_owner_composition(&project, PortOwner::Clip(shared_id)),
            Some(clip_composition_id)
        );
    }

    struct RuntimeCatalogStylePlugin {
        id: String,
    }

    impl Plugin for RuntimeCatalogStylePlugin {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> String {
            "Runtime Hatch".to_string()
        }

        fn category(&self) -> String {
            "Runtime QA".to_string()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 1, 0)
        }
    }

    impl StylePlugin for RuntimeCatalogStylePlugin {
        fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
            OperationDescriptor::style(self.id.clone(), self.name(), Vec::new())
        }

        fn evaluate_source(
            &self,
            _context: &FrameEvaluationContext,
            source_id: Uuid,
            _properties: &PropertyMap,
            _eval_time: f64,
        ) -> Option<StyleConfig> {
            Some(StyleConfig {
                id: source_id,
                style: DrawStyle::Fill {
                    color: Color::white(),
                    offset: 0.0,
                },
            })
        }
    }

    #[test]
    fn native_values_have_explicit_operation_presentation() {
        let mut project = Project::new("value node presentation");
        assert_eq!(VALUE_NODE_CATEGORY_LABEL, "Value");
        for value in ValueContent::ALL {
            let node = Node::new_value(value.label(), value);
            let node_id = node.id;
            project.add_node(node);
            assert_eq!(value_operation_label(value), value.label());
            let icon = node_icon(&project, node_id);
            assert_eq!(
                icon.glyph,
                match value {
                    ValueContent::Fmod => egui_phosphor::regular::PERCENT,
                    ValueContent::Add => egui_phosphor::regular::PLUS,
                    ValueContent::Subtract => egui_phosphor::regular::MINUS,
                    ValueContent::Multiply => egui_phosphor::regular::X,
                    ValueContent::Divide => egui_phosphor::regular::DIVIDE,
                }
            );
            assert!(icon.label.starts_with(value.label()));
            let palette = node_palette(&project, node_id);
            assert_eq!(palette.body, Color32::from_rgb(28, 41, 46));
            assert_eq!(palette.header, Color32::from_rgb(39, 83, 95));
            assert_eq!(palette.accent, Color32::from_rgb(91, 197, 218));
            assert_eq!(estimated_node_size(&project, node_id).y, 220.0);
        }
    }

    #[test]
    fn container_chrome_uses_bundled_phosphor_icons() {
        let id = Uuid::new_v4();
        assert_eq!(
            container_icon(PortOwner::Composition(id)).glyph,
            egui_phosphor::regular::PROJECTOR_SCREEN
        );
        assert_eq!(
            container_icon(PortOwner::Track(id)).glyph,
            egui_phosphor::regular::STACK
        );
        assert_eq!(
            container_icon(PortOwner::Clip(id)).glyph,
            egui_phosphor::regular::FILM_STRIP
        );
        assert_eq!(
            container_icon(PortOwner::Node(id)).glyph,
            egui_phosphor::regular::CIRCLE
        );
    }

    #[test]
    fn math_add_items_create_every_native_value_and_fmod_accepts_explicit_time() {
        let plugins = PluginManager::default();
        let items = node_create_menu_items(&plugins);
        for value in ValueContent::ALL {
            let item = items
                .iter()
                .find(|item| item.value == NodeCreateRequest::Value(value))
                .expect("every native value is exposed by the shared Add catalog");
            assert_eq!(item.label, value.label());
            assert_eq!(item.category.as_deref(), Some("Math / Values"));
            assert_eq!(
                item.qa_id.as_deref(),
                Some(format!("node_editor.menu.create.value:{}", value.operation_key()).as_str())
            );
            let node = create_operation_node_for_request(&item.value, &plugins)
                .expect("native value request creates a Node");
            assert_eq!(node.content(), &NodeContent::Value(value));
        }
        let item = items
            .iter()
            .find(|item| item.value == NodeCreateRequest::Value(ValueContent::Fmod))
            .expect("Fmod is exposed by the shared Add catalog");
        let matches = crate::ui::widgets::searchable_context_menu::filter_searchable_items(
            &items,
            "loop value",
        );
        assert!(matches.iter().any(|index| items[*index] == *item));

        let (mut project, composition_id, _, clip_id, _, _) = fixture();
        let node = create_operation_node_for_request(&item.value, &plugins)
            .expect("native value request creates a Node");
        let node_id = node.id;
        let clip = project.get_clip(clip_id).unwrap();
        let position = egui::pos2(clip.ui_position[0] + 260.0, clip.ui_position[1] + 280.0);
        assert!(create_prebuilt_node(
            &mut project,
            position,
            node,
            composition_id,
        ));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );
        let connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
                PortAddress::new(PortOwner::Node(node_id), FMOD_X_INPUT_PORT),
            )
            .expect("container Time connects explicitly to Fmod.x");
        let connection = project
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .unwrap();
        assert_eq!(connection.from.port, TIME_PORT);
        assert_eq!(connection.to.port, FMOD_X_INPUT_PORT);
    }

    #[test]
    fn real_snarl_connected_output_fans_out_to_time_value_without_reconnect_or_pan() {
        let (mut project, composition_id, _, clip_id, _, _) = fixture();
        let mut modulo = Node::new_fmod("Fmod");
        modulo.ui_position = [520.0, 620.0];
        let modulo_id = modulo.id;
        project.add_node(modulo);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), modulo_id)
            .unwrap();
        let original_time_connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from == PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT)
                    && connection.to.port == TIME_PORT
            })
            .expect("fixture Time output is already connected")
            .clone();
        let initial = project.clone();
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let mut queued = Vec::new();
        let mut gesture_transforms = Vec::new();
        reset_test_rects();

        let source_address = PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT);
        let target_address = PortAddress::new(PortOwner::Node(modulo_id), FMOD_X_INPUT_PORT);
        let source_key = RenderedPortKey {
            address: source_address.clone(),
            direction: PortDirection::Output,
            connection_id: None,
        };
        let target_key = RenderedPortKey {
            address: target_address.clone(),
            direction: PortDirection::Input,
            connection_id: None,
        };

        // Let Snarl finish its initial look-at/layout pass before using the
        // published socket rectangles as physical input coordinates.
        for frame in 0..6 {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        rendered_ports.lock().unwrap().clear();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::default();
                        let mut canvas_clip = ui.clip_rect();
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut queued,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: state.wire_gesture.is_some()
                                || state.normal_connect_gesture.is_some()
                                || state.normal_connect_cancel_pending_release,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::clone(&rendered_ports),
                            merge_layer_reorder: &mut state.merge_layer_reorder,
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                            rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("time-value-port-drag", composition_id)),
                            ui,
                        );
                        drop(viewer);
                        let edges =
                            register_rendered_edges(&project, &rendered_ports, canvas_clip, None);
                        queued.extend(wire_interactions(
                            ui,
                            &mut state,
                            WireInteractionFrame {
                                project: &project,
                                edges: &edges,
                                rendered_ports: &rendered_ports,
                                canvas_clip,
                                graph_item_rects: &exclusions,
                                to_global,
                            },
                        ));
                    });
                },
            ));
        }
        assert!(queued.is_empty());
        let ports = rendered_ports.lock().unwrap();
        let source_rect = *ports.get(&source_key).expect("actual Time output socket");
        let target_rect = *ports.get(&target_key).expect("actual value input socket");
        drop(ports);
        assert!(source_rect.is_positive());
        assert!(target_rect.is_positive());
        assert_eq!(
            test_rect(&qa_port_id(
                &project,
                Some(GraphItem::PortAnchor {
                    owner: PortOwner::Clip(clip_id),
                    kind: PortAnchorKind::InternalMetadata,
                }),
                "output",
                TIME_PORT,
            ))
            .expect("Time output QA hit")
            .center(),
            source_rect.center(),
        );
        assert_eq!(
            test_rect(&qa_port_id(
                &project,
                Some(GraphItem::Node(modulo_id)),
                "input",
                FMOD_X_INPUT_PORT,
            ))
            .expect("value input QA hit")
            .center(),
            target_rect.center(),
        );

        let source = source_rect.center();
        let target = target_rect.center();
        let drag_start = source + egui::vec2(WIRE_DRAG_THRESHOLD + 2.0, 0.0);
        let input_frames = vec![
            // First cancel the forward fan-out. Snarl still owns internal
            // drag state, so suppression must survive through release.
            vec![egui::Event::PointerMoved(source)],
            vec![egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(target)],
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            // Then perform the same connected-output fan-out successfully.
            vec![egui::Event::PointerMoved(source)],
            vec![egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            // Cross egui's drag threshold while the captured Time output is
            // still nearby. A single endpoint-to-endpoint jump can enter the
            // destination before Snarl has observed `drag_started_by`.
            vec![egui::Event::PointerMoved(drag_start)],
            vec![egui::Event::PointerMoved(target)],
            vec![egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        for (frame, events) in input_frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some((frame + 6) as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        rendered_ports.lock().unwrap().clear();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::default();
                        let mut canvas_clip = ui.clip_rect();
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut queued,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: state.wire_gesture.is_some()
                                || state.normal_connect_gesture.is_some()
                                || state.normal_connect_cancel_pending_release,
                            locked_canvas_transform: state
                                .normal_connect_gesture
                                .as_ref()
                                .map(|gesture| gesture.canvas_transform),
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::clone(&rendered_ports),
                            merge_layer_reorder: &mut state.merge_layer_reorder,
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                            rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("time-value-port-drag", composition_id)),
                            ui,
                        );
                        drop(viewer);
                        let edges =
                            register_rendered_edges(&project, &rendered_ports, canvas_clip, None);
                        queued.extend(wire_interactions(
                            ui,
                            &mut state,
                            WireInteractionFrame {
                                project: &project,
                                edges: &edges,
                                rendered_ports: &rendered_ports,
                                canvas_clip,
                                graph_item_rects: &exclusions,
                                to_global,
                            },
                        ));
                        gesture_transforms.push(to_global);
                    });
                },
            ));
            match frame {
                1 | 2 | 6..=8 => {
                    assert!(state.normal_connect_gesture.is_some());
                    assert!(!state.normal_connect_cancel_pending_release);
                }
                3 => {
                    assert!(state.normal_connect_gesture.is_none());
                    assert!(state.normal_connect_cancel_pending_release);
                    assert!(queued.is_empty());
                }
                4 | 5 => {
                    assert!(state.normal_connect_gesture.is_none());
                    assert!(!state.normal_connect_cancel_pending_release);
                    assert!(queued.is_empty());
                }
                0 | 9 => {
                    assert!(state.normal_connect_gesture.is_none());
                    assert!(!state.normal_connect_cancel_pending_release);
                }
                unexpected => panic!("unexpected input frame {unexpected}"),
            }
            assert!(!state.normal_wire_drag_active);
        }

        assert!(gesture_transforms.windows(2).all(|pair| pair[0] == pair[1]));
        assert!(state.wire_gesture.is_none());
        assert!(state.selected_connection_id.is_none());
        assert!(
            queued.iter().any(|queued| {
                matches!(
                    queued,
                    QueuedNodeEdit::Atomic(NodeEdit::Connect { from, to })
                        if *from == source_address && *to == target_address
                )
            }),
            "real Snarl drag queued {queued:?}; wire state: {:?}",
            state.wire_gesture,
        );
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        assert!(apply_queued_node_edits(
            &mut project,
            queued,
            &mut history,
            &mut state,
        ));
        assert!(project.connections.iter().any(|connection| {
            connection.from == source_address && connection.to == target_address
        }));
        assert_eq!(
            project
                .connections
                .iter()
                .find(|connection| connection.id == original_time_connection.id),
            Some(&original_time_connection),
            "fan-out must preserve the original Time wire byte-for-byte"
        );
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    fn style_graph_factory() -> library::editor::project_service::ProjectManager {
        library::editor::project_service::ProjectManager::new(
            Arc::new(RwLock::new(Project::new("detached graph factory"))),
            Arc::new(PluginManager::default()),
        )
    }

    fn run_wire_interaction_frames(
        project: &Project,
        edge: &RenderedEdge,
        rendered_ports: &Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
        state: &mut NodeEditorState,
        frames: Vec<Vec<egui::Event>>,
    ) -> Vec<QueuedNodeEdit> {
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
        let mut queued = Vec::new();
        for (frame, events) in frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        queued.extend(wire_interactions(
                            ui,
                            state,
                            WireInteractionFrame {
                                project,
                                edges: std::slice::from_ref(edge),
                                rendered_ports,
                                canvas_clip: screen,
                                graph_item_rects: &[],
                                to_global: egui::emath::TSTransform::IDENTITY,
                            },
                        ));
                    });
                },
            ));
        }
        queued
    }

    fn plugin_operation_component(node: &Node) -> Option<&str> {
        match node.content() {
            NodeContent::PluginOperation(operation) => Some(&operation.component_id),
            _ => None,
        }
    }

    fn assert_detached_graph_has_clean_ltr_layout(project: &Project, graph: &NodeGraphBundle) {
        let mut measurement_project = project.clone();
        for node in &graph.nodes {
            measurement_project.add_node(node.clone());
        }
        let rects = graph
            .nodes
            .iter()
            .map(|node| {
                (
                    node.id,
                    egui::Rect::from_min_size(
                        egui::pos2(node.ui_position[0], node.ui_position[1]),
                        estimated_node_size(&measurement_project, node.id),
                    ),
                )
            })
            .collect::<HashMap<_, _>>();

        for (index, left) in graph.nodes.iter().enumerate() {
            for right in &graph.nodes[index + 1..] {
                assert!(
                    !padded_intersection(rects[&left.id], rects[&right.id]),
                    "detached Nodes overlap or have less than {AUTO_LAYOUT_NODE_PADDING}px gap: {} {:?} / {} {:?}",
                    left.id,
                    rects[&left.id],
                    right.id,
                    rects[&right.id],
                );
            }
        }

        let ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        let edges = graph
            .connections
            .iter()
            .filter_map(|connection| {
                let (PortOwner::Node(from), PortOwner::Node(to)) =
                    (connection.from.owner, connection.to.owner)
                else {
                    return None;
                };
                Some((from, to))
            })
            .collect::<Vec<_>>();
        let ranks = rank_nodes_by_scc(&ids, &edges);
        for (from, to) in edges {
            if ranks[&from] == ranks[&to] {
                continue;
            }
            assert!(
                rects[&from].right() + AUTO_LAYOUT_NODE_PADDING <= rects[&to].left(),
                "detached edge is not left-to-right: {from} {:?} -> {to} {:?}",
                rects[&from],
                rects[&to],
            );
        }
    }

    #[test]
    fn add_menu_discovers_descriptor_operations_and_wire_menu_filters_them_by_type() {
        let plugins = Arc::new(PluginManager::default());
        let runtime_style_id = "runtime.qa.style.hatch";
        plugins.register_style_plugin(Arc::new(RuntimeCatalogStylePlugin {
            id: runtime_style_id.to_string(),
        }));
        let items = node_create_menu_items(plugins.as_ref());
        assert!(items.iter().all(|item| item.category.is_some()));
        assert!(items
            .iter()
            .any(|item| matches!(item.value, NodeCreateRequest::SkSL)));

        let root_transform = items
            .iter()
            .find(|item| item.value == NodeCreateRequest::Transform)
            .expect("root Transform is exposed as its own Add request");
        assert_eq!(root_transform.label, "Shape Transform");
        assert_eq!(
            root_transform.category.as_deref(),
            Some("Shape Operations / Transform")
        );
        assert_eq!(
            root_transform.qa_id.as_deref(),
            Some("node_editor.menu.create.transform")
        );
        assert_eq!(
            root_transform.qa_metadata.as_ref().unwrap()["operation_category"],
            TRANSFORM_CATEGORY
        );
        assert_eq!(
            root_transform.qa_metadata.as_ref().unwrap()["operation"],
            TRANSFORM_APPLY_OPERATION
        );
        let root_matches = crate::ui::widgets::searchable_context_menu::filter_searchable_items(
            &items,
            "root placement",
        );
        assert!(root_matches
            .iter()
            .any(|index| items[*index] == *root_transform));
        let root_node = create_operation_node_for_request(&root_transform.value, plugins.as_ref())
            .expect("root Transform request uses the operation factory");
        let NodeContent::PluginOperation(root_operation) = root_node.content() else {
            panic!("root Transform factory must create a PluginOperation")
        };
        assert_eq!(root_operation.category, TRANSFORM_CATEGORY);
        assert_eq!(root_operation.component_id, SHAPE_TRANSFORM_COMPONENT_ID);
        assert_eq!(root_operation.operation, TRANSFORM_APPLY_OPERATION);

        for (component_id, label) in [
            ("transform", "Effector · Transform Modulation"),
            ("opacity", "Effector · Opacity Modulation"),
        ] {
            let item = items
                .iter()
                .find(|item| {
                    matches!(&item.value, NodeCreateRequest::Effector(id) if id == component_id)
                })
                .expect("built-in modulation Effector is exposed in the Add menu");
            assert_eq!(item.label, label);
        }

        for component_id in ["fill", "stroke"] {
            let style = items
                .iter()
                .find(|item| {
                    matches!(&item.value, NodeCreateRequest::Style(id) if id == component_id)
                })
                .unwrap();
            assert_eq!(
                style.qa_metadata.as_ref().unwrap()["component_id"],
                component_id
            );
            assert_eq!(
                style.qa_metadata.as_ref().unwrap()["operation"],
                STYLE_APPLY_OPERATION
            );
        }
        let backplate = items
            .iter()
            .find(
                |item| matches!(&item.value, NodeCreateRequest::Decorator(id) if id == "backplate"),
            )
            .unwrap();
        assert_eq!(
            backplate.qa_id.as_deref(),
            Some("node_editor.menu.create.decorator:backplate")
        );

        let runtime_style = items
            .iter()
            .find(|item| {
                matches!(&item.value, NodeCreateRequest::Style(id) if id == runtime_style_id)
            })
            .expect("a style registered after PluginManager construction must be discoverable");
        assert_eq!(
            runtime_style.qa_id.as_deref(),
            Some("node_editor.menu.create.style:runtime.qa.style.hatch")
        );
        assert_eq!(
            runtime_style.qa_metadata.as_ref().unwrap()["label"],
            "Runtime Hatch"
        );
        assert!(runtime_style
            .keywords
            .iter()
            .any(|keyword| keyword == runtime_style_id));

        let blur = items
            .iter()
            .find(|item| {
                matches!(&item.value, NodeCreateRequest::Effect(effect_id) if effect_id == "blur")
            })
            .expect("built-in Blur effect is exposed in the Add menu");
        assert!(blur
            .category
            .as_deref()
            .is_some_and(|category| category.starts_with("Image Effects /")));
        assert_eq!(
            blur.qa_id.as_deref(),
            Some("node_editor.menu.create.effect:blur")
        );
        assert!(blur.keywords.iter().any(|keyword| keyword == "blur"));
        let matches = crate::ui::widgets::searchable_context_menu::filter_searchable_items(
            &items,
            "runtime.qa hatch",
        );
        assert!(matches.iter().any(|index| items[*index] == *runtime_style));

        let (mut project, _, _, clip_id, _, _) = fixture();
        let shape = generator_node(
            "Shape",
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 100 V 100 Z".to_string(),
            },
        );
        let shape_id = shape.id;
        let transform = plugins.create_effector_operation_node("transform").unwrap();
        let transform_id = transform.id;
        project.add_node(shape);
        project.add_node(transform);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), shape_id)
            .unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), transform_id)
            .unwrap();
        let shape_connection = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT),
            )
            .unwrap();
        let splice_items = wire_splice_menu_items(&project, shape_connection, plugins.as_ref());
        assert!(splice_items
            .iter()
            .any(|item| item.value == NodeCreateRequest::Transform));
        assert!(splice_items.iter().any(|item| {
            matches!(&item.value, NodeCreateRequest::Decorator(id) if id == "backplate")
        }));
        assert!(!splice_items.iter().any(|item| {
            matches!(&item.value, NodeCreateRequest::Style(id) if id == runtime_style_id)
        }));

        let root =
            create_operation_node_for_request(&NodeCreateRequest::Transform, plugins.as_ref())
                .expect("wire insertion uses the root Transform factory");
        let root_id = root.id;
        project.add_node(root);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), root_id)
            .unwrap();
        assert!(splice_existing_node_on_connection(
            &mut project,
            shape_connection,
            root_id,
        ));
        assert!(project.connections.iter().any(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(root_id), SHAPE_INPUT_PORT)
        }));
        assert!(project.connections.iter().any(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(root_id), SHAPE_OUTPUT_PORT)
                && connection.to
                    == PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT)
        }));
        assert!(!project.connections.iter().any(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT)
                && connection.to
                    == PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT)
        }));
    }

    #[test]
    fn node_enabled_context_command_is_atomic_and_undoable() {
        let (mut project, _, _, _, node_id, _) = fixture();
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        let mut state = NodeEditorState::default();
        assert!(apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::SetEnabled {
                node_id,
                enabled: false,
            })],
            &mut history,
            &mut state,
        ));
        assert!(!project.get_node(node_id).unwrap().enabled);
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    #[test]
    fn merge_wire_layer_order_and_authored_blend_are_canonical_and_undoable() {
        let (mut project, _, _, clip_id, solid_id, merge_id) = fixture();
        let first_connection_id = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
            })
            .unwrap()
            .id;
        let mut second = generator_node(
            "Second Solid",
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        );
        second.ui_position = [450.0, 560.0];
        let second_id = second.id;
        project.add_node(second);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), second_id)
            .unwrap();
        let second_connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            )
            .unwrap();

        let first = project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection_id)
            .unwrap();
        let second = project
            .connections
            .iter()
            .find(|connection| connection.id == second_connection_id)
            .unwrap();
        assert!(connection_supports_authored_blend(&project, first));
        assert!(connection_supports_authored_blend(&project, second));
        assert_eq!(
            wire_order_menu_state(&project, first),
            Some(WireOrderMenuState {
                back_to_front_index: 0,
                layer_count: 2,
            })
        );
        assert_eq!(
            wire_order_menu_state(&project, second),
            Some(WireOrderMenuState {
                back_to_front_index: 1,
                layer_count: 2,
            })
        );

        // Disabled boundary actions are true no-ops: no Project change and no
        // extra history snapshot even if the QA bridge injects their click.
        let boundary_initial = project.clone();
        let mut boundary_history = HistoryManager::new();
        boundary_history.push_project_state(boundary_initial.clone());
        let mut state = NodeEditorState::default();
        assert!(!apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                connection_id: first_connection_id,
                new_order: 0,
            })],
            &mut boundary_history,
            &mut state,
        ));
        assert_eq!(project, boundary_initial);
        assert_eq!(boundary_history.undo_depth(), 1);
        assert!(!apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                connection_id: second_connection_id,
                new_order: 1,
            })],
            &mut boundary_history,
            &mut state,
        ));
        assert_eq!(project, boundary_initial);
        assert_eq!(boundary_history.undo_depth(), 1);

        let blend_initial = project.clone();
        let original_first = project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection_id)
            .unwrap()
            .clone();
        let mut blend_history = HistoryManager::new();
        blend_history.push_project_state(blend_initial.clone());
        assert!(apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
                connection_id: first_connection_id,
                blend_mode: BlendMode::Multiply,
            })],
            &mut blend_history,
            &mut state,
        ));
        let blended_first = project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection_id)
            .unwrap();
        assert_eq!(blended_first.id, original_first.id);
        assert_eq!(blended_first.from, original_first.from);
        assert_eq!(blended_first.to, original_first.to);
        assert_eq!(blended_first.order, original_first.order);
        assert_eq!(blended_first.blend_mode, BlendMode::Multiply);
        let blend_edited = project.clone();
        assert_single_gesture_undo_redo(&mut blend_history, &blend_initial, &blend_edited);

        let reorder_initial = project.clone();
        let original_second_blend = project
            .connections
            .iter()
            .find(|connection| connection.id == second_connection_id)
            .unwrap()
            .blend_mode;
        let mut reorder_history = HistoryManager::new();
        reorder_history.push_project_state(reorder_initial.clone());
        assert!(apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                connection_id: first_connection_id,
                new_order: 1,
            })],
            &mut reorder_history,
            &mut state,
        ));
        let mut merge_connections = project
            .connections
            .iter()
            .filter(|connection| {
                connection.to == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
            })
            .collect::<Vec<_>>();
        merge_connections.sort_by_key(|connection| connection.order);
        assert_eq!(
            merge_connections
                .iter()
                .map(|connection| (connection.id, connection.order))
                .collect::<Vec<_>>(),
            vec![(second_connection_id, 0), (first_connection_id, 1)]
        );
        assert_eq!(merge_connections[0].blend_mode, original_second_blend);
        assert_eq!(merge_connections[1].blend_mode, BlendMode::Multiply);
        assert_eq!(merge_connections[1].from, original_first.from);
        assert_eq!(merge_connections[1].to, original_first.to);
        let reorder_edited = project.clone();
        assert_single_gesture_undo_redo(&mut reorder_history, &reorder_initial, &reorder_edited);

        let no_op_initial = project.clone();
        let mut no_op_history = HistoryManager::new();
        no_op_history.push_project_state(no_op_initial.clone());
        assert!(!apply_queued_node_edits(
            &mut project,
            vec![
                QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                    connection_id: first_connection_id,
                    new_order: 1,
                }),
                QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
                    connection_id: first_connection_id,
                    blend_mode: BlendMode::Multiply,
                }),
            ],
            &mut no_op_history,
            &mut state,
        ));
        assert_eq!(project, no_op_initial);
        assert_eq!(no_op_history.undo_depth(), 1);

        let time_connection = project
            .connections
            .iter()
            .find(|connection| connection.from.port == TIME_PORT)
            .unwrap()
            .clone();
        assert!(!connection_supports_authored_blend(
            &project,
            &time_connection
        ));
        assert!(!apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
                connection_id: time_connection.id,
                blend_mode: BlendMode::LinearDodge,
            })],
            &mut no_op_history,
            &mut state,
        ));
        assert_eq!(project, no_op_initial);
        assert_eq!(no_op_history.undo_depth(), 1);
    }

    #[test]
    fn merge_body_rows_are_back_to_front_and_keep_per_wire_blend_and_source_identity() {
        let (mut project, composition_id, _, clip_id, solid_id, merge_id) = fixture();
        let single_layer_estimated = estimated_node_size(&project, merge_id);
        let first_connection_id = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
            })
            .expect("fixture Merge connection")
            .id;
        project
            .set_connection_blend_mode(first_connection_id, BlendMode::LinearDodge)
            .expect("first wire Add");

        let mut middle = generator_node(
            "Middle Green",
            GeneratorNodeRequest::Solid {
                color: Color {
                    r: 0,
                    g: 255,
                    b: 0,
                    a: 255,
                },
            },
        );
        middle.ui_position = [490.0, 520.0];
        let middle_id = middle.id;
        project.add_node(middle);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), middle_id)
            .expect("attach middle source");
        let middle_connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(middle_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            )
            .expect("connect middle source");
        project
            .set_connection_blend_mode(middle_connection_id, BlendMode::Multiply)
            .expect("middle wire Multiply");

        let mut front = generator_node(
            "Front Blue",
            GeneratorNodeRequest::Solid {
                color: Color {
                    r: 0,
                    g: 0,
                    b: 255,
                    a: 255,
                },
            },
        );
        front.ui_position = [530.0, 650.0];
        let front_id = front.id;
        project.add_node(front);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), front_id)
            .expect("attach front source");
        let front_connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(front_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            )
            .expect("connect front source");
        project
            .set_connection_blend_mode(front_connection_id, BlendMode::Screen)
            .expect("front wire Screen");

        let rows = merge_layer_rows(&project, merge_id);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .map(|row| (
                    row.connection_id,
                    row.back_to_front_index,
                    row.authored_order,
                    row.authored_blend_mode,
                    row.source.owner,
                    row.source_label.as_str(),
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    first_connection_id,
                    0,
                    0,
                    BlendMode::LinearDodge,
                    PortOwner::Node(solid_id),
                    "Node · Solid",
                ),
                (
                    middle_connection_id,
                    1,
                    1,
                    BlendMode::Multiply,
                    PortOwner::Node(middle_id),
                    "Node · Middle Green",
                ),
                (
                    front_connection_id,
                    2,
                    2,
                    BlendMode::Screen,
                    PortOwner::Node(front_id),
                    "Node · Front Blue",
                ),
            ]
        );
        assert!(rows.iter().all(|row| {
            row.merge_id == merge_id && row.layer_count == 3 && row.authored_blend_available
        }));

        let estimated = estimated_node_size(&project, merge_id);
        assert_eq!(estimated.x, 518.0);
        assert_eq!(estimated.x, estimated_merge_node_width());
        assert_eq!(estimated_node_size(&project, solid_id).x, 462.0);
        assert_eq!(estimated_node_width(), 462.0);
        assert!(estimated.y > single_layer_estimated.y);
        let (rects, _, rendered_transform, _) =
            render_test_graph_with_context_menu_exclusions(&project, composition_id);
        let rendered_merge = rects
            .get(&format!("node_editor.node:{merge_id}"))
            .expect("rendered Merge card");
        assert!(rendered_merge.width() <= estimated.x * rendered_transform.scaling + 1.0);
        assert!(rendered_merge.height() <= estimated.y * rendered_transform.scaling + 1.0);
        let port_rects = [
            qa_port_id(
                &project,
                Some(GraphItem::Node(merge_id)),
                "input",
                MERGE_IMAGES_PORT,
            ),
            qa_port_id(
                &project,
                Some(GraphItem::Node(merge_id)),
                "output",
                IMAGE_OUTPUT_PORT,
            ),
        ]
        .map(|component_id| {
            *rects
                .get(&component_id)
                .unwrap_or_else(|| panic!("missing Merge port {component_id}"))
        });
        for row in &rows {
            for component_id in [
                format!("node_editor.merge_layer:{merge_id}:{}", row.connection_id),
                format!(
                    "node_editor.merge_layer.blend_select:{merge_id}:{}",
                    row.connection_id
                ),
                format!(
                    "node_editor.merge_layer.order_back:{merge_id}:{}",
                    row.connection_id
                ),
                format!(
                    "node_editor.merge_layer.order_front:{merge_id}:{}",
                    row.connection_id
                ),
            ] {
                let control = rects
                    .get(&component_id)
                    .unwrap_or_else(|| panic!("missing Merge body component {component_id}"));
                assert!(control.is_positive(), "empty Merge control {component_id}");
                assert!(
                    port_rects
                        .iter()
                        .all(|port_rect| !control.intersects(*port_rect)),
                    "Merge control {component_id} overlaps a left/right port: {control:?}"
                );
            }
        }
    }

    #[test]
    fn empty_merge_body_has_a_stable_empty_state_and_minimum_estimated_height() {
        let (mut project, composition_id, _, _, _, merge_id) = fixture();
        project.connections.retain(|connection| {
            connection.to != PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
        });

        assert!(merge_layer_rows(&project, merge_id).is_empty());
        assert_eq!(estimated_node_size(&project, merge_id).y, 220.0);
        let rects = render_test_graph(&project, composition_id);
        assert!(rects
            .get(&format!("node_editor.merge_layers.empty:{merge_id}"))
            .is_some_and(egui::Rect::is_positive));
    }

    #[test]
    fn real_egui_wire_hit_selects_and_dragging_the_body_queues_disconnect() {
        let (project, _, _, _, solid_id, merge_id) = fixture();
        let connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap();
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: connection.id,
            },
            start: egui::pos2(120.0, 180.0),
            control_a: egui::pos2(200.0, 180.0),
            control_b: egui::pos2(300.0, 180.0),
            end: egui::pos2(380.0, 180.0),
        };
        let midpoint =
            cubic_bezier_point(edge.start, edge.control_a, edge.control_b, edge.end, 0.5);
        let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let click = vec![
            vec![egui::Event::PointerMoved(midpoint)],
            vec![egui::Event::PointerButton {
                pos: midpoint,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: midpoint,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        assert!(
            run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, click,)
                .is_empty()
        );
        assert_eq!(state.selected_connection_id, Some(connection.id));

        let escape = vec![vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]];
        assert!(
            run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, escape,)
                .is_empty()
        );
        assert!(state.selected_connection_id.is_none());

        assert!(run_wire_interaction_frames(
            &project,
            &edge,
            &rendered_ports,
            &mut state,
            vec![
                vec![egui::Event::PointerMoved(midpoint)],
                vec![egui::Event::PointerButton {
                    pos: midpoint,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                vec![egui::Event::PointerButton {
                    pos: midpoint,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            ],
        )
        .is_empty());
        assert_eq!(state.selected_connection_id, Some(connection.id));
        let blank = egui::pos2(32.0, 32.0);
        assert!(run_wire_interaction_frames(
            &project,
            &edge,
            &rendered_ports,
            &mut state,
            vec![
                vec![egui::Event::PointerMoved(blank)],
                vec![egui::Event::PointerButton {
                    pos: blank,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                vec![egui::Event::PointerButton {
                    pos: blank,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            ],
        )
        .is_empty());
        assert!(state.selected_connection_id.is_none());

        let dragged = midpoint + egui::vec2(0.0, 48.0);
        let drag = vec![
            vec![egui::Event::PointerMoved(midpoint)],
            vec![egui::Event::PointerButton {
                pos: midpoint,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(dragged)],
            vec![egui::Event::PointerButton {
                pos: dragged,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        let edits = run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, drag);
        assert!(
            matches!(
                edits.as_slice(),
                [QueuedNodeEdit::Atomic(NodeEdit::DisconnectConnection { connection_id })]
                    if *connection_id == connection.id
            ),
            "unexpected wire drag edits: {edits:?}; gesture: {:?}",
            state.wire_gesture
        );
    }

    #[test]
    fn connected_output_invalid_drop_and_escape_leave_project_and_history_untouched() {
        let (project, _, _, _, solid_id, merge_id) = fixture();
        let connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap();
        let source = egui::pos2(120.0, 180.0);
        let invalid_target = egui::pos2(540.0, 340.0);
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: connection.id,
            },
            start: source,
            control_a: egui::pos2(200.0, 180.0),
            control_b: egui::pos2(300.0, 180.0),
            end: egui::pos2(380.0, 180.0),
        };
        let rendered_ports = Arc::new(Mutex::new(HashMap::from([(
            RenderedPortKey {
                address: connection.from.clone(),
                direction: PortDirection::Output,
                connection_id: None,
            },
            egui::Rect::from_center_size(source, egui::vec2(13.0, 13.0)),
        )])));

        let invalid_drop = vec![
            vec![egui::Event::PointerMoved(source)],
            vec![egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(invalid_target)],
            vec![egui::Event::PointerButton {
                pos: invalid_target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        let mut state = NodeEditorState::default();
        let edits =
            run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, invalid_drop);
        assert!(edits.is_empty());
        assert!(state.normal_connect_gesture.is_none());
        assert!(state.selected_connection_id.is_none());

        let escaped = vec![
            vec![egui::Event::PointerMoved(source)],
            vec![egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(invalid_target)],
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: invalid_target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        let edits =
            run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, escaped);
        assert!(edits.is_empty());
        assert!(state.normal_connect_gesture.is_none());

        let mut untouched = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(project.clone());
        let undo_depth = history.undo_depth();
        assert!(!apply_queued_node_edits(
            &mut untouched,
            edits,
            &mut history,
            &mut state,
        ));
        assert_eq!(untouched, project);
        assert_eq!(history.undo_depth(), undo_depth);
    }

    #[test]
    fn overview_wire_midpoint_remains_a_body_target_when_endpoint_radii_overlap() {
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: Uuid::new_v4(),
            },
            start: egui::pos2(100.0, 100.0),
            control_a: egui::pos2(106.0, 100.0),
            control_b: egui::pos2(114.0, 100.0),
            end: egui::pos2(120.0, 100.0),
        };
        assert_eq!(
            rendered_wire_drag_kind(&edge, egui::pos2(110.0, 100.0)),
            NodeEditorWireDragKind::Disconnect
        );
        assert_eq!(
            rendered_wire_drag_kind(&edge, edge.start),
            NodeEditorWireDragKind::ReconnectSource
        );
        assert_eq!(
            rendered_wire_drag_kind(&edge, edge.end),
            NodeEditorWireDragKind::ReconnectTarget
        );
    }

    #[test]
    fn endpoint_drag_reconnects_through_real_pointer_frames_without_changing_wire_identity() {
        let (mut project, _, _, clip_id, solid_id, merge_id) = fixture();
        let mut alternate = generator_node(
            "Alternate",
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        );
        alternate.ui_position = [250.0, 520.0];
        let alternate_id = alternate.id;
        project.add_node(alternate);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), alternate_id)
            .unwrap();
        let connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap()
            .clone();
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: connection.id,
            },
            start: egui::pos2(120.0, 180.0),
            control_a: egui::pos2(200.0, 180.0),
            control_b: egui::pos2(300.0, 180.0),
            end: egui::pos2(380.0, 180.0),
        };
        let alternate_position = egui::pos2(480.0, 260.0);
        let rendered_ports = Arc::new(Mutex::new(HashMap::from([(
            RenderedPortKey {
                address: PortAddress::new(PortOwner::Node(alternate_id), IMAGE_OUTPUT_PORT),
                direction: PortDirection::Output,
                connection_id: None,
            },
            egui::Rect::from_center_size(alternate_position, egui::vec2(14.0, 14.0)),
        )])));
        let mut state = NodeEditorState::default();
        let edits = run_wire_interaction_frames(
            &project,
            &edge,
            &rendered_ports,
            &mut state,
            vec![
                vec![egui::Event::PointerMoved(edge.start)],
                vec![egui::Event::PointerButton {
                    pos: edge.start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                vec![egui::Event::PointerMoved(alternate_position)],
                vec![egui::Event::PointerButton {
                    pos: alternate_position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            ],
        );
        let [QueuedNodeEdit::Atomic(NodeEdit::ReconnectConnection {
            connection_id,
            from,
            to,
        })] = edits.as_slice()
        else {
            panic!("endpoint drag did not queue one reconnect: {edits:?}");
        };
        assert_eq!(*connection_id, connection.id);
        assert_eq!(from.owner, PortOwner::Node(alternate_id));
        assert_eq!(*to, connection.to);
        assert!(apply_edit(
            &mut project,
            NodeEdit::ReconnectConnection {
                connection_id: *connection_id,
                from: from.clone(),
                to: to.clone(),
            },
        ));
        let reconnected = project
            .connections
            .iter()
            .find(|candidate| candidate.id == connection.id)
            .unwrap();
        assert_eq!(reconnected.from.owner, PortOwner::Node(alternate_id));
        assert_eq!(reconnected.to, connection.to);
        assert_eq!(reconnected.order, connection.order);
    }

    #[test]
    fn operation_node_splice_preserves_downstream_uuid_order_and_target() {
        let (mut project, composition_id, _, clip_id, solid_id, merge_id) = fixture();
        let connection = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap()
            .clone();
        let plugins = PluginManager::default();
        let mut blur = plugins.create_effect_operation_node("blur").unwrap();
        blur.ui_position = [610.0, 500.0];
        let blur_id = blur.id;
        project.add_node(blur);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), blur_id)
            .unwrap();
        assert!(splice_existing_node_on_connection(
            &mut project,
            connection.id,
            blur_id,
        ));
        let downstream = project
            .connections
            .iter()
            .find(|candidate| candidate.id == connection.id)
            .unwrap();
        assert_eq!(downstream.from.owner, PortOwner::Node(blur_id));
        assert_eq!(downstream.to, connection.to);
        assert_eq!(downstream.order, connection.order);

        let second_connection = project
            .connections
            .iter()
            .find(|candidate| candidate.to.owner == PortOwner::Node(blur_id))
            .unwrap()
            .clone();
        let second_blur = plugins.create_effect_operation_node("blur").unwrap();
        let second_blur_id = second_blur.id;
        assert!(insert_node_on_connection(
            &mut project,
            second_connection.id,
            second_blur,
            egui::pos2(560.0, 440.0),
            composition_id,
        ));
        assert_eq!(
            project
                .connections
                .iter()
                .find(|candidate| candidate.id == second_connection.id)
                .unwrap()
                .to,
            second_connection.to
        );
        assert_eq!(
            project.find_node_container(second_blur_id),
            Some(NodeContainer::Clip(clip_id))
        );
    }

    fn queued_property_edit(
        owner: PortOwner,
        key: &str,
        time: f64,
        value: PropertyValue,
        finished: bool,
    ) -> QueuedNodeEdit {
        QueuedNodeEdit::Continuous {
            pending: NodeEditorPendingEdit {
                owner,
                key: key.to_string(),
            },
            edit: Some(NodeEdit::SetProperty {
                owner,
                key: key.to_string(),
                time,
                value,
            }),
            finished,
        }
    }

    fn queued_finish(owner: PortOwner, key: &str) -> QueuedNodeEdit {
        QueuedNodeEdit::Continuous {
            pending: NodeEditorPendingEdit {
                owner,
                key: key.to_string(),
            },
            edit: None,
            finished: true,
        }
    }

    fn assert_single_gesture_undo_redo(
        history: &mut HistoryManager,
        initial: &Project,
        edited: &Project,
    ) {
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(edited), Some(initial.clone()));
        assert_eq!(history.redo(initial), Some(edited.clone()));
    }

    #[derive(Clone, Copy)]
    struct HierarchyFixtureIds {
        composition: Uuid,
        track: Uuid,
        empty_track: Uuid,
        clip: Uuid,
        sibling_clip: Uuid,
        empty_clip: Uuid,
        solid: Uuid,
        merge: Uuid,
        text: Uuid,
        text_fill: Uuid,
        shape: Uuid,
        shape_fill: Uuid,
        composition_node: Uuid,
    }

    fn adversarial_hierarchy_fixture() -> (Project, HierarchyFixtureIds) {
        let (mut project, composition, track, clip, solid, merge) = fixture();
        let overlapping_position = [420.0, 300.0];
        project.get_node_mut(solid).unwrap().ui_position = overlapping_position;
        project.get_node_mut(merge).unwrap().ui_position = overlapping_position;

        let sibling_clip = Uuid::from_u128(0x7_001);
        let mut clip_b = library::model::Clip::new("Sibling Clip", 0.0, 5.0);
        clip_b.id = sibling_clip;
        clip_b.ui_position = [260.0, 260.0];
        clip_b.ui_size = [360.0, 220.0];
        project.add_clip(clip_b);
        project.attach_clip_to_track(track, sibling_clip).unwrap();

        let text = Uuid::from_u128(0x7_002);
        let mut text_node = generator_node(
            "Tall Text",
            GeneratorNodeRequest::Text {
                text: "A deliberately tall text Node".to_string(),
                font: "Arial".to_string(),
            },
        );
        text_node.id = text;
        text_node.ui_position = overlapping_position;
        project.add_node(text_node);
        project
            .attach_node_to_container(NodeContainer::Clip(sibling_clip), text)
            .unwrap();
        let text_fill = Uuid::from_u128(0x7_007);
        let mut text_fill_node = PluginManager::default()
            .create_style_operation_node("fill")
            .unwrap();
        text_fill_node.id = text_fill;
        text_fill_node.ui_position = overlapping_position;
        project.add_node(text_fill_node);
        project
            .attach_node_to_container(NodeContainer::Clip(sibling_clip), text_fill)
            .unwrap();
        project.get_clip_mut(sibling_clip).unwrap().output_node_id = Some(text_fill);

        let empty_clip = Uuid::from_u128(0x7_003);
        let mut collapsed_clip = library::model::Clip::new("Collapsed Empty", 0.0, 5.0);
        collapsed_clip.id = empty_clip;
        collapsed_clip.ui_collapsed = true;
        collapsed_clip.ui_position = [260.0, 260.0];
        project.add_clip(collapsed_clip);
        project.attach_clip_to_track(track, empty_clip).unwrap();

        let shape = Uuid::from_u128(0x7_004);
        let mut shape_node = generator_node(
            "Track Shape",
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 100 V 100 Z".to_string(),
            },
        );
        shape_node.id = shape;
        shape_node.ui_position = overlapping_position;
        project.add_node(shape_node);
        project
            .attach_node_to_container(NodeContainer::Track(track), shape)
            .unwrap();
        let shape_fill = Uuid::from_u128(0x7_008);
        let mut shape_fill_node = PluginManager::default()
            .create_style_operation_node("fill")
            .unwrap();
        shape_fill_node.id = shape_fill;
        shape_fill_node.ui_position = overlapping_position;
        project.add_node(shape_fill_node);
        project
            .attach_node_to_container(NodeContainer::Track(track), shape_fill)
            .unwrap();

        let composition_node = Uuid::from_u128(0x7_005);
        let mut root_merge = Node::new_merge("Composition Merge");
        root_merge.id = composition_node;
        root_merge.ui_position = overlapping_position;
        project.add_node(root_merge);
        project
            .attach_node_to_container(NodeContainer::Composition(composition), composition_node)
            .unwrap();

        let empty_track = Uuid::from_u128(0x7_006);
        let mut collapsed_track = library::model::Track::new("Collapsed Empty Track");
        collapsed_track.id = empty_track;
        collapsed_track.ui_collapsed = true;
        collapsed_track.ui_position = [110.0, 140.0];
        project.add_track(collapsed_track);
        project
            .attach_track_to_composition(composition, empty_track)
            .unwrap();

        for (source, fill) in [(text, text_fill), (shape, shape_fill)] {
            project
                .connect_ports(
                    PortAddress::new(PortOwner::Node(source), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(fill), SHAPE_INPUT_PORT),
                )
                .unwrap();
            project
                .connect_ports(
                    PortAddress::new(PortOwner::Node(fill), IMAGE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(merge), MERGE_IMAGES_PORT),
                )
                .unwrap();
        }
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(merge), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(composition_node), MERGE_IMAGES_PORT),
            )
            .unwrap();

        (
            project,
            HierarchyFixtureIds {
                composition,
                track,
                empty_track,
                clip,
                sibling_clip,
                empty_clip,
                solid,
                merge,
                text,
                text_fill,
                shape,
                shape_fill,
                composition_node,
            },
        )
    }

    fn render_test_graph_with_context_menu_exclusions(
        project: &Project,
        composition_id: Uuid,
    ) -> (
        HashMap<String, egui::Rect>,
        Vec<egui::Rect>,
        egui::emath::TSTransform,
        Vec<ContainerVisual>,
    ) {
        let context = egui::Context::default();
        let (mut snarl, containers) = build_snarl(project, composition_id);
        let mut final_context_menu_exclusion_rects = Vec::new();
        let mut final_to_global = egui::emath::TSTransform::default();
        reset_test_rects();

        for frame in 0..5 {
            let raw_input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1800.0, 1200.0),
                )),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            };
            drop(context.run(raw_input, |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut selection = None;
                    let mut wire_context_request = None;
                    let mut context_menu_exclusion_rects = Vec::new();
                    let mut to_global = egui::emath::TSTransform::default();
                    let mut canvas_clip = ui.clip_rect();
                    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
                    let mut merge_layer_reorder = None;
                    let mut viewer = ProjectNodeViewer {
                        project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut edits,
                        pending_navigation: &mut navigation,
                        pending_selection: &mut selection,
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut context_menu_exclusion_rects,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: false,
                        locked_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::clone(&rendered_ports),
                        merge_layer_reorder: &mut merge_layer_reorder,
                        rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                    };
                    let style = SnarlStyle {
                        collapsible: Some(false),
                        min_scale: Some(0.05),
                        max_scale: Some(1.0),
                        ..Default::default()
                    };
                    snarl.show(
                        &mut viewer,
                        &style,
                        egui::Id::new(("hierarchical-layout-test", composition_id)),
                        ui,
                    );
                    drop(viewer);
                    final_context_menu_exclusion_rects = context_menu_exclusion_rects;
                    final_to_global = to_global;
                    for container in &containers {
                        register_container_chrome(container, to_global, canvas_clip, project, 0.0);
                    }
                    register_rendered_edges(project, &rendered_ports, canvas_clip, None);
                });
            }));
        }
        (
            test_rects(),
            final_context_menu_exclusion_rects,
            final_to_global,
            containers,
        )
    }

    fn render_test_graph(project: &Project, composition_id: Uuid) -> HashMap<String, egui::Rect> {
        render_test_graph_with_context_menu_exclusions(project, composition_id).0
    }

    #[test]
    fn text_add_inserts_one_clean_graph_without_replacing_the_clip_output() {
        let (mut project, composition_id, _, clip_id, _, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .unwrap();
        let initial_layout = compute_full_composition_layout(&project, composition_id).unwrap();
        assert!(apply_auto_layout(
            &mut project,
            composition_id,
            &initial_layout
        ));
        assert!(!layout_needs_reflow(&project, composition_id));

        let factory = style_graph_factory();
        let graph = factory
            .create_text_graph("Hello", "Arial", 1920, 1080)
            .unwrap();
        let consumer_id = graph.output_node_id.expect("Text factory sink");
        let source_id = graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    node.content(),
                    NodeContent::Generator(GeneratorContent::Text)
                )
            })
            .unwrap()
            .id;
        let fill_id = graph
            .nodes
            .iter()
            .find(|node| plugin_operation_component(node) == Some("fill"))
            .unwrap()
            .id;
        let connection = graph.connections[0].clone();
        let bundled_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();

        let mut laid_out = graph.clone();
        layout_detached_node_graph(&project, &mut laid_out);
        assert_detached_graph_has_clean_ltr_layout(&project, &laid_out);
        let relative_positions = laid_out
            .nodes
            .iter()
            .map(|node| (node.id, node.ui_position))
            .collect::<HashMap<_, _>>();

        let clip = project.get_clip(clip_id).unwrap();
        let desired = egui::pos2(
            clip.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
            clip.ui_position[1] + AUTO_LAYOUT_CLIP_TOP,
        );
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        assert!(insert_prebuilt_graph(
            &mut project,
            desired,
            graph,
            composition_id
        ));
        history.push_project_state(project.clone());

        let clip = project.get_clip(clip_id).unwrap();
        assert_eq!(clip.output_node_id, Some(merge_id));
        assert_eq!(
            &clip.node_ids[clip.node_ids.len() - bundled_ids.len()..],
            bundled_ids.as_slice()
        );
        assert_eq!(
            project.find_node_container(consumer_id),
            Some(NodeContainer::Clip(clip_id))
        );
        assert_eq!(
            project
                .connections
                .iter()
                .find(|candidate| candidate.id == connection.id),
            Some(&connection)
        );

        let shape_output = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .unwrap();
        let shape_input = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
                PortDirection::Input,
            )
            .unwrap();
        let image_output = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(fill_id), IMAGE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .unwrap();
        assert_eq!(shape_output.data_type, PortDataType::Shape);
        assert_eq!(shape_input.data_type, PortDataType::Shape);
        assert_eq!(image_output.data_type, PortDataType::Image);

        let first_id = bundled_ids[0];
        let first = project.get_node(first_id).unwrap().ui_position;
        let first_relative = relative_positions[&first_id];
        let translation = [first[0] - first_relative[0], first[1] - first_relative[1]];
        for node_id in &bundled_ids {
            let inserted = project.get_node(*node_id).unwrap().ui_position;
            let relative = relative_positions[node_id];
            assert!((inserted[0] - relative[0] - translation[0]).abs() < 0.01);
            assert!((inserted[1] - relative[1] - translation[1]).abs() < 0.01);
        }
        assert!(!layout_needs_reflow(&project, composition_id));

        let rects = render_test_graph(&project, composition_id);
        for id in [
            format!("node_editor.port.node:{source_id}.output:{SHAPE_OUTPUT_PORT}"),
            format!("node_editor.port.node:{fill_id}.input:{SHAPE_INPUT_PORT}"),
            format!("node_editor.port.node:{fill_id}.output:{IMAGE_OUTPUT_PORT}"),
            format!("node_editor.edge:{}", connection.id),
        ] {
            assert!(
                rects.get(&id).is_some_and(egui::Rect::is_positive),
                "missing visible typed Shape/Image graph component {id}"
            );
        }

        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    #[test]
    fn shape_add_preserves_none_output_and_fill_stroke_order_without_overlap() {
        let (mut project, composition_id, _, clip_id, _, _) = fixture();
        let initial_layout = compute_full_composition_layout(&project, composition_id).unwrap();
        assert!(apply_auto_layout(
            &mut project,
            composition_id,
            &initial_layout
        ));
        assert_eq!(project.get_clip(clip_id).unwrap().output_node_id, None);

        let factory = style_graph_factory();
        let graph = factory
            .create_shape_graph("M0 0 H100 V100 Z", 1920, 1080, 100, 100)
            .unwrap();
        let consumer_id = graph.output_node_id.expect("Shape factory sink");
        let bundled_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        let connection_ids = graph
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        let mut laid_out = graph.clone();
        layout_detached_node_graph(&project, &mut laid_out);
        assert_detached_graph_has_clean_ltr_layout(&project, &laid_out);

        let clip = project.get_clip(clip_id).unwrap();
        let desired = egui::pos2(
            clip.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
            clip.ui_position[1] + AUTO_LAYOUT_CLIP_TOP,
        );
        assert!(insert_prebuilt_graph(
            &mut project,
            desired,
            graph,
            composition_id
        ));

        let clip = project.get_clip(clip_id).unwrap();
        assert_eq!(clip.output_node_id, None);
        assert_eq!(
            &clip.node_ids[clip.node_ids.len() - bundled_ids.len()..],
            bundled_ids.as_slice()
        );
        let appended = project
            .connections
            .iter()
            .filter(|connection| {
                connection_ids.contains(&connection.id)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(consumer_id), MERGE_IMAGES_PORT)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            appended
                .iter()
                .map(|connection| connection.order)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            appended
                .iter()
                .map(|connection| {
                    let PortOwner::Node(node_id) = connection.from.owner else {
                        panic!("Style source must be a Node")
                    };
                    plugin_operation_component(project.get_node(node_id).unwrap()).unwrap()
                })
                .collect::<Vec<_>>(),
            vec!["fill", "stroke"]
        );
        assert!(!layout_needs_reflow(&project, composition_id));
    }

    #[test]
    fn standalone_style_add_has_shape_input_image_output_and_failed_graph_add_is_atomic() {
        let (mut project, composition_id, _, clip_id, _, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .unwrap();
        let factory = style_graph_factory();
        let plugins = factory.get_plugin_manager();

        let fill = plugins.create_style_operation_node("fill").unwrap();
        let fill_id = fill.id;
        assert!(insert_prebuilt_graph(
            &mut project,
            egui::pos2(500.0, 350.0),
            NodeGraphBundle::new(vec![fill], Vec::new(), None),
            composition_id,
        ));
        assert_eq!(
            project.get_clip(clip_id).unwrap().output_node_id,
            Some(merge_id)
        );
        assert_eq!(
            project.find_node_container(fill_id),
            Some(NodeContainer::Clip(clip_id))
        );
        assert_eq!(
            project
                .port_definition(
                    &PortAddress::new(PortOwner::Node(fill_id), IMAGE_OUTPUT_PORT),
                    PortDirection::Output,
                )
                .unwrap()
                .data_type,
            PortDataType::Image
        );
        assert_eq!(
            project
                .port_definition(
                    &PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
                    PortDirection::Input,
                )
                .unwrap()
                .data_type,
            PortDataType::Shape
        );

        let stroke = plugins.create_style_operation_node("stroke").unwrap();
        let stroke_id = stroke.id;
        let width = node_property_definition(Some(&plugins), &stroke, "width")
            .expect("runtime descriptor width metadata");
        assert!(matches!(
            width.ui_type(),
            PropertyUiType::Float {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix,
                ..
            } if suffix == "px"
        ));
        let join = node_property_definition(Some(&plugins), &stroke, "join")
            .expect("runtime descriptor enum metadata");
        assert!(matches!(
            join.ui_type(),
            PropertyUiType::Dropdown { options }
                if options.iter().map(String::as_str).eq(["Miter", "Round", "Bevel"])
        ));
        assert!(insert_prebuilt_graph(
            &mut project,
            egui::pos2(500.0, 350.0),
            NodeGraphBundle::new(vec![stroke], Vec::new(), None),
            composition_id,
        ));
        assert_eq!(
            project.get_clip(clip_id).unwrap().output_node_id,
            Some(merge_id)
        );
        assert_eq!(
            project.find_node_container(stroke_id),
            Some(NodeContainer::Clip(clip_id))
        );
        let rects = render_test_graph(&project, composition_id);
        for node_id in [fill_id, stroke_id] {
            let output = format!("node_editor.port.node:{node_id}.output:{IMAGE_OUTPUT_PORT}");
            assert!(rects.get(&output).is_some_and(egui::Rect::is_positive));
        }

        let mut invalid = factory
            .create_text_graph("duplicate", "Arial", 1920, 1080)
            .unwrap();
        invalid.nodes[0].id = merge_id;
        let before = project.clone();
        assert!(!insert_prebuilt_graph(
            &mut project,
            egui::pos2(500.0, 350.0),
            invalid,
            composition_id,
        ));
        assert_eq!(
            project, before,
            "failed insertion must not partially mutate Project"
        );
    }

    #[test]
    fn effect_operation_add_is_ltr_typed_atomic_and_preserves_the_clip_output() {
        let (mut project, composition_id, _, clip_id, _, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .unwrap();
        let initial_layout = compute_full_composition_layout(&project, composition_id).unwrap();
        assert!(apply_auto_layout(
            &mut project,
            composition_id,
            &initial_layout
        ));

        let factory = style_graph_factory();
        let plugins = factory.get_plugin_manager();
        assert!(plugins
            .get_available_effects()
            .iter()
            .any(|(id, _, _)| id == "blur"));
        let source = factory
            .create_solid_node(
                library::model::frame::color::Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
                1920,
                1080,
            )
            .unwrap();
        let effect = plugins.create_effect_operation_node("blur").unwrap();
        let source_id = source.id;
        let effect_id = effect.id;
        let sigma_x = node_property_definition(Some(&plugins), &effect, "sigma_x")
            .expect("Blur numeric metadata");
        assert!(matches!(
            sigma_x.ui_type(),
            PropertyUiType::Float {
                min: 0.0,
                max: 100.0,
                step: 0.1,
                suffix,
                min_hard_limit: true,
                max_hard_limit: false,
            } if suffix == "px"
        ));
        let tile_mode = node_property_definition(Some(&plugins), &effect, "tile_mode")
            .expect("Blur enum metadata");
        assert!(matches!(
            tile_mode.ui_type(),
            PropertyUiType::Dropdown { options }
                if options.iter().map(String::as_str).eq(["clamp", "repeat", "mirror", "decal"])
        ));

        let wire = ProjectConnection::new(
            PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
            0,
        );
        let graph = NodeGraphBundle::new(vec![source, effect], vec![wire.clone()], Some(effect_id));
        let bundled_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        let mut laid_out = graph.clone();
        layout_detached_node_graph(&project, &mut laid_out);
        assert_detached_graph_has_clean_ltr_layout(&project, &laid_out);

        let clip = project.get_clip(clip_id).unwrap();
        let desired = egui::pos2(
            clip.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
            clip.ui_position[1] + AUTO_LAYOUT_CLIP_TOP,
        );
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        assert!(insert_prebuilt_graph(
            &mut project,
            desired,
            graph,
            composition_id,
        ));
        history.push_project_state(project.clone());

        let clip = project.get_clip(clip_id).unwrap();
        assert_eq!(clip.output_node_id, Some(merge_id));
        assert_eq!(
            &clip.node_ids[clip.node_ids.len() - bundled_ids.len()..],
            bundled_ids.as_slice()
        );
        assert_eq!(
            project
                .connections
                .iter()
                .find(|connection| connection.id == wire.id),
            Some(&wire)
        );
        for (port, direction) in [
            (IMAGE_INPUT_PORT, PortDirection::Input),
            (IMAGE_OUTPUT_PORT, PortDirection::Output),
        ] {
            assert_eq!(
                project
                    .port_definition(
                        &PortAddress::new(PortOwner::Node(effect_id), port),
                        direction,
                    )
                    .unwrap()
                    .data_type,
                PortDataType::Image
            );
        }
        assert!(!layout_needs_reflow(&project, composition_id));

        let rects = render_test_graph(&project, composition_id);
        for id in [
            format!("node_editor.port.node:{effect_id}.input:{IMAGE_INPUT_PORT}"),
            format!("node_editor.port.node:{effect_id}.output:{IMAGE_OUTPUT_PORT}"),
            format!("node_editor.edge:{}", wire.id),
        ] {
            assert!(
                rects.get(&id).is_some_and(egui::Rect::is_positive),
                "missing visible Effect graph component {id}"
            );
        }
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);

        let mut duplicate = plugins.create_effect_operation_node("blur").unwrap();
        duplicate.id = merge_id;
        let before_failure = project.clone();
        assert!(!insert_prebuilt_graph(
            &mut project,
            desired,
            NodeGraphBundle::new(vec![duplicate], Vec::new(), None),
            composition_id,
        ));
        assert_eq!(project, before_failure);
    }

    #[test]
    fn effector_operation_nodes_and_menu_use_the_authoritative_descriptor() {
        let factory = style_graph_factory();
        let plugins = factory.get_plugin_manager();
        let menu_entries = available_effector_menu_entries(plugins.as_ref());
        assert!(
            menu_entries.contains(&("transform".to_string(), "Transform Modulation".to_string()))
        );
        assert!(menu_entries.contains(&("opacity".to_string(), "Opacity Modulation".to_string())));
        assert!(menu_entries
            .windows(2)
            .all(|entries| entries[0].1 <= entries[1].1));

        for component_id in ["transform", "opacity"] {
            let descriptor = plugins
                .operation_descriptor(EFFECTOR_CATEGORY, component_id, EFFECTOR_APPLY_OPERATION)
                .unwrap();
            let node = plugins
                .create_effector_operation_node(component_id)
                .unwrap();
            assert_eq!(
                node.properties()
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<BTreeSet<_>>(),
                descriptor
                    .properties()
                    .iter()
                    .map(PropertyDefinition::name)
                    .collect::<BTreeSet<_>>()
            );
            for definition in descriptor.properties() {
                assert_eq!(
                    node.properties()
                        .get(definition.name())
                        .and_then(|property| property.evaluate_at(0.0).ok()),
                    Some(definition.default_value().clone()),
                    "{component_id}.{} was not initialized by its descriptor factory",
                    definition.name(),
                );
            }
        }
        let transform = plugins.create_effector_operation_node("transform").unwrap();
        assert_eq!(
            transform
                .properties()
                .get("target")
                .and_then(|property| property.evaluate_at(0.0).ok()),
            Some(PropertyValue::String("Block".to_string()))
        );
        let opacity = plugins.create_effector_operation_node("opacity").unwrap();
        assert_eq!(
            opacity
                .properties()
                .get("mode")
                .and_then(|property| property.evaluate_at(0.0).ok()),
            Some(PropertyValue::String("Set".to_string()))
        );
        assert_eq!(
            opacity
                .properties()
                .get("target")
                .and_then(|property| property.evaluate_at(0.0).ok()),
            Some(PropertyValue::String("Block".to_string()))
        );
    }

    #[test]
    fn node_editor_effector_control_responds_to_real_pointer_drag() {
        let (mut project, composition_id, _, clip_id, _, _) = fixture();
        let plugins = PluginManager::default();
        let mut effector = plugins.create_effector_operation_node("transform").unwrap();
        effector.ui_position = [520.0, 390.0];
        let effector_id = effector.id;
        project.add_node(effector);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), effector_id)
            .unwrap();
        let layout = compute_full_composition_layout(&project, composition_id).unwrap();
        assert!(apply_auto_layout(&mut project, composition_id, &layout));
        assert!(!layout_needs_reflow(&project, composition_id));
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let component_id = format!("node_editor.property.node:{effector_id}:tx");
        let mut queued = Vec::new();
        reset_test_rects();

        let mut frames = vec![Vec::new(); 5];
        for (frame, events) in frames.drain(..).enumerate() {
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut merge_layer_reorder = None;
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: Some(&plugins),
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            merge_layer_reorder: &mut merge_layer_reorder,
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                            rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("effector-real-event", composition_id)),
                            ui,
                        );
                        queued.extend(edits);
                    });
                },
            );
            drop(output);
        }
        let rect = test_rect(&component_id).expect("rendered Transform tx control");
        assert!(rect.is_positive());
        let start = rect.center();
        let end = start + egui::vec2(52.0, 0.0);
        let event_frames = [
            vec![egui::Event::PointerMoved(start)],
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        for (offset, events) in event_frames.into_iter().enumerate() {
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some((offset + 5) as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut merge_layer_reorder = None;
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: Some(&plugins),
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            merge_layer_reorder: &mut merge_layer_reorder,
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                            rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("effector-real-event", composition_id)),
                            ui,
                        );
                        queued.extend(edits);
                    });
                },
            );
            drop(output);
        }

        assert!(
            queued.iter().any(|edit| matches!(
                edit,
                QueuedNodeEdit::Continuous {
                    edit: Some(NodeEdit::SetProperty {
                        owner: PortOwner::Node(id),
                        key,
                        value: PropertyValue::Number(value),
                        ..
                    }),
                    ..
                } if *id == effector_id && key == "tx" && value.into_inner() > 0.0
            )),
            "real pointer drag over {rect:?} did not edit tx: {queued:#?}"
        );
        assert!(queued.iter().any(|edit| matches!(
            edit,
            QueuedNodeEdit::Continuous {
                pending,
                finished: true,
                ..
            } if pending.owner == PortOwner::Node(effector_id) && pending.key == "tx"
        )));
    }

    #[test]
    fn extreme_zoom_transform_and_adaptive_grid_remain_finite_and_bounded() {
        let style = node_editor_snarl_style();
        assert_eq!(style.min_scale, Some(0.0065));
        assert_eq!(style.max_scale, Some(1.25));

        let mut corrupt =
            egui::emath::TSTransform::new(egui::vec2(f32::NAN, f32::INFINITY), f32::NEG_INFINITY);
        sanitize_node_editor_transform(&mut corrupt);
        assert_eq!(corrupt, egui::emath::TSTransform::IDENTITY);
        assert!(corrupt.is_valid());
        assert!(corrupt.translation.y.is_finite());

        let mut extreme = egui::emath::TSTransform::new(
            egui::vec2(20_000_000.0, -20_000_000.0),
            NODE_EDITOR_MIN_SCALE / 10.0,
        );
        sanitize_node_editor_transform(&mut extreme);
        assert_eq!(extreme.scaling, NODE_EDITOR_MIN_SCALE);
        assert_eq!(extreme.translation.x, NODE_EDITOR_MAX_TRANSLATION);
        assert_eq!(extreme.translation.y, -NODE_EDITOR_MAX_TRANSLATION);

        let transform =
            egui::emath::TSTransform::new(egui::vec2(347.0, -73.0), NODE_EDITOR_MIN_SCALE);
        let graph_position = egui::pos2(500_000.0, -250_000.0);
        let screen_position = transform * graph_position;
        let round_trip = transform.inverse() * screen_position;
        assert!(screen_position.x.is_finite() && screen_position.y.is_finite());
        assert!(round_trip.distance(graph_position) < 0.1);
        assert!(!node_editor_details_visible(NODE_EDITOR_MIN_SCALE));
        assert!(node_editor_details_visible(NODE_EDITOR_DETAIL_SCALE));
        assert!(
            (screen_stroke_in_graph_units(1.65, NODE_EDITOR_MIN_SCALE) * NODE_EDITOR_MIN_SCALE
                - 1.65)
                .abs()
                < 1.0e-5
        );

        let screen_wire = [
            egui::pos2(10.0, 20.0),
            egui::pos2(35.0, 20.0),
            egui::pos2(65.0, 80.0),
            egui::pos2(90.0, 80.0),
        ];
        let graph_wire =
            overview_wire_graph_points(screen_wire, transform).expect("finite overview wire");
        for (screen, graph) in screen_wire.into_iter().zip(graph_wire) {
            assert!((transform * graph).distance(screen) < 0.001);
        }
        assert!(overview_wire_graph_points(
            screen_wire,
            egui::emath::TSTransform::new(egui::Vec2::ZERO, 0.0),
        )
        .is_none());

        for scale in [
            NODE_EDITOR_MIN_SCALE,
            0.01,
            0.02,
            0.1,
            NODE_EDITOR_DETAIL_SCALE,
            1.0,
            NODE_EDITOR_MAX_SCALE,
        ] {
            let spacing = adaptive_grid_spacing(scale);
            let screen_spacing = spacing * scale;
            assert!(spacing.is_finite() && spacing > 0.0);
            assert!(
                (GRID_TARGET_SCREEN_SPACING..=GRID_TARGET_SCREEN_SPACING * 2.5 + 0.01)
                    .contains(&screen_spacing),
                "scale={scale}, spacing={spacing}, screen_spacing={screen_spacing}"
            );
            let approximate_line_count =
                (1800.0 / screen_spacing).ceil() + (1200.0 / screen_spacing).ceil();
            assert!(approximate_line_count < 70.0);
        }
    }

    #[test]
    fn overview_wire_survives_the_real_egui_layer_transform_in_screen_space() {
        let context = egui::Context::default();
        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let to_global =
            egui::emath::TSTransform::new(egui::vec2(420.0, 310.0), NODE_EDITOR_MIN_SCALE);
        let start = egui::pos2(250.0, 300.0);
        let end = egui::pos2(750.0, 500.0);
        let from = PortAddress::new(PortOwner::Node(Uuid::from_u128(0x901)), "image");
        let to = PortAddress::new(PortOwner::Node(Uuid::from_u128(0x902)), "image");
        let ports = HashMap::from([
            (
                RenderedPortKey {
                    address: from.clone(),
                    direction: PortDirection::Output,
                    connection_id: None,
                },
                egui::Rect::from_center_size(start, egui::Vec2::ZERO),
            ),
            (
                RenderedPortKey {
                    address: to.clone(),
                    direction: PortDirection::Input,
                    connection_id: None,
                },
                egui::Rect::from_center_size(end, egui::Vec2::ZERO),
            ),
        ]);
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(canvas),
                ..Default::default()
            },
            |context| {
                let layer = egui::LayerId::new(
                    egui::Order::Middle,
                    egui::Id::new("overview-wire-transform-test"),
                );
                context.set_transform_layer(layer, to_global);
                let painter =
                    egui::Painter::new(context.clone(), layer, to_global.inverse() * canvas);
                let _ = register_edge_component(
                    EdgeComponent {
                        id: "node_editor.edge:overview-wire-transform-test".to_string(),
                        kind: RenderedEdgeKind::ProjectConnection {
                            connection_id: Uuid::from_u128(0x8_001),
                        },
                        from: &from,
                        to: &to,
                        wire_color: pin_color(PortDataType::Image),
                        authored_order: None,
                        back_to_front_index: None,
                        layer_count: None,
                        physical_merge_target: false,
                        authored_blend_mode: None,
                        authored_blend_available: false,
                        runtime_first_produced_may_be_normal: false,
                    },
                    &ports,
                    canvas,
                    Some(OverviewWirePainter {
                        painter: &painter,
                        to_global,
                    }),
                );
            },
        );

        let (clip_rect, wire) = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::CubicBezier(wire) => Some((clipped.clip_rect, wire)),
                _ => None,
            })
            .expect("overview CubicBezier in final egui output");
        let expected_frame = ((end.x - start.x).abs() * 0.45).clamp(2.0, 110.0);
        let expected = [
            start,
            start + egui::vec2(expected_frame, 0.0),
            end - egui::vec2(expected_frame, 0.0),
            end,
        ];
        for (actual, expected) in wire.points.iter().zip(expected) {
            assert!(actual.distance(expected) < 0.01);
        }
        assert!((wire.stroke.width - 1.65).abs() < 0.001);
        assert!(clip_rect.min.distance(canvas.min) < 0.01);
        assert!(clip_rect.max.distance(canvas.max) < 0.01);
    }

    #[test]
    fn overview_canvas_keeps_pan_gestures_while_precision_controls_are_disabled() {
        let (project, composition_id, _, _, _, _) = fixture();
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let zoom_position = egui::pos2(500.0, 400.0);
        let drag_start = egui::pos2(100.0, 100.0);
        let drag_end = egui::pos2(220.0, 160.0);
        let command_modifiers = egui::Modifiers {
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ..egui::Modifiers::NONE
        };
        let frames = [
            (Vec::new(), egui::Modifiers::NONE),
            (
                vec![
                    egui::Event::PointerMoved(zoom_position),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -10_000.0),
                        modifiers: command_modifiers,
                    },
                ],
                command_modifiers,
            ),
            (
                vec![
                    egui::Event::PointerMoved(drag_start),
                    egui::Event::PointerButton {
                        pos: drag_start,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
            ),
            (
                vec![egui::Event::PointerMoved(drag_end)],
                egui::Modifiers::NONE,
            ),
            (
                vec![egui::Event::PointerButton {
                    pos: drag_end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                egui::Modifiers::NONE,
            ),
        ];
        let mut transforms = Vec::new();
        let mut node_editor_state = NodeEditorState::default();

        for (frame, (events, modifiers)) in frames.into_iter().enumerate() {
            let mut rendered_transform = egui::emath::TSTransform::IDENTITY;
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    modifiers,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
                        let mut merge_layer_reorder = None;
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports,
                            merge_layer_reorder: &mut merge_layer_reorder,
                            rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                            rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("overview-pan-test", composition_id)),
                            ui,
                        );
                        drop(viewer);
                        let resize_edits = container_resize_interactions(
                            ui,
                            &project,
                            &containers,
                            to_global,
                            canvas_clip,
                            &mut node_editor_state,
                        );
                        assert!(edits.is_empty());
                        assert!(resize_edits.is_empty());
                        rendered_transform = to_global;
                    });
                },
            );
            assert!(!output.shapes.is_empty());
            transforms.push(rendered_transform);
        }

        let zoomed = transforms[1];
        let dragged = transforms[3];
        assert!((zoomed.scaling - NODE_EDITOR_MIN_SCALE).abs() < f32::EPSILON);
        assert_eq!(dragged.scaling, zoomed.scaling);
        assert!(!node_editor_resize_interactions_enabled(zoomed.scaling));
        assert!(!node_editor_port_interactions_enabled(zoomed.scaling));
        let pan_delta = dragged.translation - zoomed.translation;
        assert!((pan_delta.x - (drag_end.x - drag_start.x)).abs() < 1.0);
        assert!((pan_delta.y - (drag_end.y - drag_start.y)).abs() < 1.0);
        assert!(node_editor_state.container_resize.is_none());
    }

    #[test]
    fn canvas_qa_metadata_exposes_the_final_clamped_transform_and_lod_gates() {
        let composition_id = Uuid::from_u128(0xCA_11_A5);
        let metadata = node_editor_canvas_metadata(
            composition_id,
            egui::emath::TSTransform::new(egui::vec2(321.5, -87.25), NODE_EDITOR_MIN_SCALE / 10.0),
        );
        assert_eq!(metadata["composition_id"], composition_id.to_string());
        assert_eq!(metadata["scale"], NODE_EDITOR_MIN_SCALE);
        assert_eq!(metadata["translation"]["x"], 321.5);
        assert_eq!(metadata["translation"]["y"], -87.25);
        assert_eq!(metadata["min_scale"], NODE_EDITOR_MIN_SCALE);
        assert_eq!(metadata["max_scale"], NODE_EDITOR_MAX_SCALE);
        assert_eq!(metadata["detail_enabled"], false);
        assert_eq!(metadata["port_interaction_enabled"], false);
        assert_eq!(metadata["resize_interaction_enabled"], false);
    }

    #[test]
    fn active_knife_owns_the_canvas_transform_instead_of_panning_the_scene() {
        let locked = egui::emath::TSTransform::new(egui::vec2(120.0, 240.0), 0.25);
        let mut scene_pan =
            egui::emath::TSTransform::new(egui::vec2(1_920.0, -480.0), locked.scaling);
        resolve_node_editor_transform(&mut scene_pan, Some(locked));
        assert_eq!(scene_pan, locked);

        let mut normal_pan =
            egui::emath::TSTransform::new(egui::vec2(1_920.0, -480.0), locked.scaling);
        resolve_node_editor_transform(&mut normal_pan, None);
        assert_eq!(normal_pan.translation, egui::vec2(1_920.0, -480.0));
    }

    #[test]
    fn overview_port_qa_rect_matches_the_real_reconnect_drop_hit_test() {
        let scale = NODE_EDITOR_DETAIL_SCALE * 0.5;
        assert!(!node_editor_port_interactions_enabled(scale));
        let to_global = egui::emath::TSTransform::new(egui::vec2(410.0, 290.0), scale);
        let graph_position = egui::pos2(120.0, 80.0);
        let rendered_port_rect =
            to_global * egui::Rect::from_center_size(graph_position, egui::Vec2::ZERO);
        let drop_rect = wire_port_drop_rect(rendered_port_rect);
        let position = rendered_port_rect.center();
        assert_eq!(drop_rect.size(), egui::vec2(10.0, 10.0));
        assert!(drop_rect.contains(position));

        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        assert!(clipped_qa_rect(drop_rect, canvas).is_positive());
        let address = PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT);
        let ports = HashMap::from([(
            RenderedPortKey {
                address: address.clone(),
                direction: PortDirection::Output,
                connection_id: None,
            },
            rendered_port_rect,
        )]);
        assert_eq!(
            rendered_port_at_position(&ports, PortDirection::Output, position, canvas),
            Some(address)
        );
        assert!(
            rendered_normal_port_at_position(&ports, position, canvas).is_none(),
            "overview reconnect drop targets must not steal normal Snarl connection gestures"
        );
        let detailed_ports = HashMap::from([(
            RenderedPortKey {
                address: PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT),
                direction: PortDirection::Output,
                connection_id: None,
            },
            egui::Rect::from_center_size(position, egui::vec2(13.0, 13.0)),
        )]);
        assert!(rendered_normal_port_at_position(&detailed_ports, position, canvas).is_some());

        let offscreen = egui::Rect::from_center_size(
            egui::pos2(canvas.right() + 10.0, 400.0),
            egui::Vec2::ZERO,
        );
        assert!(!clipped_qa_rect(wire_port_drop_rect(offscreen), canvas).is_positive());
        let offscreen_ports = HashMap::from([(
            RenderedPortKey {
                address: PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT),
                direction: PortDirection::Output,
                connection_id: None,
            },
            offscreen,
        )]);
        assert!(rendered_port_at_position(
            &offscreen_ports,
            PortDirection::Output,
            offscreen.center(),
            canvas,
        )
        .is_none());
    }

    #[test]
    fn edge_endpoint_qa_metadata_exposes_screen_position_and_unclipped_rect() {
        let connection_id = Uuid::from_u128(0xED6E);
        let position = egui::pos2(321.5, 654.25);
        let rect = egui::Rect::from_center_size(position, egui::vec2(18.0, 18.0));
        let metadata = edge_endpoint_qa_metadata(connection_id, "source", position, rect);
        assert_eq!(metadata["action"], "reconnect");
        assert_eq!(metadata["connection_id"], connection_id.to_string());
        assert_eq!(metadata["endpoint"], "source");
        assert_eq!(metadata["position"]["x"], position.x);
        assert_eq!(metadata["position"]["y"], position.y);
        assert_eq!(metadata["unclipped_rect"]["min_x"], rect.min.x);
        assert_eq!(metadata["unclipped_rect"]["min_y"], rect.min.y);
        assert_eq!(metadata["unclipped_rect"]["max_x"], rect.max.x);
        assert_eq!(metadata["unclipped_rect"]["max_y"], rect.max.y);
    }

    #[test]
    fn extreme_zoom_disables_precision_hits_without_expanding_node_hit_area() {
        assert!(!node_editor_port_interactions_enabled(
            NODE_EDITOR_MIN_SCALE
        ));
        assert!(!node_editor_resize_interactions_enabled(
            NODE_EDITOR_MIN_SCALE
        ));
        assert!(node_editor_resize_interactions_enabled(
            NODE_EDITOR_RESIZE_INTERACTION_SCALE
        ));
        assert!(!node_editor_port_interactions_enabled(
            NODE_EDITOR_RESIZE_INTERACTION_SCALE
        ));
        assert!(node_editor_port_interactions_enabled(
            NODE_EDITOR_DETAIL_SCALE
        ));

        let graph_node = egui::Rect::from_min_size(
            egui::pos2(250_000.0, 80_000.0),
            egui::vec2(NODE_HEADER_WIDTH, 100.0),
        );
        let scale = NODE_EDITOR_MIN_SCALE;
        let desired_center = egui::pos2(500.0, 400.0);
        let translation = desired_center.to_vec2() - graph_node.center().to_vec2() * scale;
        let to_global = egui::emath::TSTransform::new(translation, scale);
        let screen_node = to_global * graph_node;
        assert!(screen_node.width() < 2.0);
        assert!(screen_node.height() < 1.0);

        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let exclusions = [graph_node];
        let mut state = None;
        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(screen_node.center()),
            canvas,
            &exclusions,
            to_global,
            1.0,
        );
        assert!(
            state.is_none(),
            "the actual tiny node still owns its pixels"
        );

        let nearby_empty_screen = screen_node.center() + egui::vec2(4.0, 0.0);
        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(nearby_empty_screen),
            canvas,
            &exclusions,
            to_global,
            2.0,
        );
        assert_eq!(
            state
                .expect("nearby overview space remains canvas")
                .position,
            nearby_empty_screen
        );

        let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
        let pin = QaPin {
            info: pin_info(PortDataType::Image, false),
            component_id: "node_editor.port.extreme_zoom_test".to_string(),
            to_global,
            graph_center: Some(graph_node.center()),
            address: None,
            data_type: PortDataType::Image,
            direction: PortDirection::Input,
            connected: false,
            connection_id: None,
            canvas_clip: canvas,
            rendered_ports,
        };
        let pin_rect = pin.pin_rect(0.0, 0.0, 20.0, 13.0);
        assert!(
            !pin_rect.is_positive(),
            "overview sockets cannot steal a drag"
        );

        // At this scale every fixed-width screen resize region overlaps this
        // tiny container. The resize dispatcher therefore gates all of them
        // with the same precision-interaction threshold.
        let tiny_container = egui::Rect::from_center_size(
            desired_center,
            egui::vec2(MIN_CONTAINER_SIZE.x * scale, MIN_CONTAINER_SIZE.y * scale),
        );
        assert!(resize_regions(tiny_container)
            .iter()
            .any(|(_, _, rect, _)| rect.contains(desired_center)));
        assert!(!node_editor_resize_interactions_enabled(scale));
    }

    #[test]
    fn graph_is_derived_from_three_container_levels_and_canonical_connections() {
        let (project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        let (snarl, containers) = build_snarl(&project, composition_id);
        let items = snarl.nodes().copied().collect::<Vec<_>>();

        assert_eq!(containers.len(), 3);
        for owner in [
            PortOwner::Composition(composition_id),
            PortOwner::Track(track_id),
            PortOwner::Clip(clip_id),
        ] {
            assert!(items.contains(&GraphItem::Container(owner)));
            assert!(items.contains(&GraphItem::PortAnchor {
                owner,
                kind: PortAnchorKind::ExternalInputs,
            }));
            assert!(items.contains(&GraphItem::PortAnchor {
                owner,
                kind: PortAnchorKind::InternalMetadata,
            }));
            assert!(items.contains(&GraphItem::PortAnchor {
                owner,
                kind: PortAnchorKind::OutputSinks,
            }));
            assert!(items.contains(&GraphItem::PortAnchor {
                owner,
                kind: PortAnchorKind::ExternalOutputs,
            }));
            assert_eq!(
                input_definitions(
                    &project,
                    GraphItem::PortAnchor {
                        owner,
                        kind: PortAnchorKind::ExternalInputs,
                    },
                )
                .len(),
                3
            );
        }
        assert!(items.contains(&GraphItem::Node(solid_id)));
        assert!(items.contains(&GraphItem::Node(merge_id)));
        assert!(snarl.wires().count() >= 2);

        let merge_inputs = input_definitions(&project, GraphItem::Node(merge_id));
        assert!(merge_inputs.iter().any(|pin| pin.key == MERGE_IMAGES_PORT));
        let metadata = output_definitions(
            &project,
            GraphItem::PortAnchor {
                owner: PortOwner::Clip(clip_id),
                kind: PortAnchorKind::InternalMetadata,
            },
        );
        assert_eq!(metadata[0].key, TIME_PORT);
        assert_eq!(metadata[1].key, FRAME_PORT);
    }

    #[test]
    fn transformed_container_chrome_exclusively_owns_secondary_click_but_empty_body_does_not() {
        let (project, composition_id, track_id, _, _, _) = fixture();
        let (rects, exclusion_rects, rendered_to_global, containers) =
            render_test_graph_with_context_menu_exclusions(&project, composition_id);
        let header_screen_rect = rects
            .get(&format!(
                "node_editor.container_header.{}",
                qa_container_key(PortOwner::Track(track_id))
            ))
            .copied()
            .expect("rendered Track header");
        let header_graph_position = rendered_to_global.inverse() * header_screen_rect.center();
        assert!(
            exclusion_rects
                .iter()
                .any(|rect| rect.contains(header_graph_position)),
            "the rendered Track header must be claimed by a Snarl hit rect"
        );

        let track_visual = containers
            .iter()
            .find(|container| container.owner == PortOwner::Track(track_id))
            .expect("Track visual");
        let track_rect = track_visual.rect();
        let empty_body_graph_position =
            egui::pos2(track_rect.right() - 40.0, track_rect.bottom() - 40.0);
        let track_body = egui::Rect::from_min_max(
            egui::pos2(
                track_rect.left(),
                track_rect.top() + CONTAINER_HEADER_HEIGHT,
            ),
            track_rect.max,
        );
        assert!(track_body.contains(empty_body_graph_position));
        assert!(
            !exclusion_rects
                .iter()
                .any(|rect| rect.contains(empty_body_graph_position)),
            "empty container body must not be claimed by Snarl chrome"
        );

        // Exercise the same inverse-transform path as a panned and zoomed
        // Node Editor canvas, rather than comparing screen-space rectangles.
        let to_global = egui::emath::TSTransform::new(egui::vec2(347.0, -73.0), 0.72);
        let canvas_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let header_screen_position = to_global * header_graph_position;
        let empty_body_screen_position = to_global * empty_body_graph_position;
        assert!(canvas_rect.contains(header_screen_position));
        assert!(canvas_rect.contains(empty_body_screen_position));

        let mut state = None;
        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(header_screen_position),
            canvas_rect,
            &exclusion_rects,
            to_global,
            1.0,
        );
        assert!(
            state.is_none(),
            "Track header click must not open global Create menu"
        );

        state = Some(ContextMenuState::new(egui::pos2(1.0, 1.0), 0.0));
        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(header_screen_position),
            canvas_rect,
            &exclusion_rects,
            to_global,
            2.0,
        );
        assert!(
            state.is_none(),
            "Track header click must close a stale global Create menu"
        );

        update_global_context_menu_for_secondary_click(
            &mut state,
            true,
            Some(empty_body_screen_position),
            canvas_rect,
            &exclusion_rects,
            to_global,
            3.0,
        );
        let context = state.expect("empty Track body should open global Create menu");
        assert_eq!(context.position, empty_body_screen_position);
    }

    #[test]
    fn rendered_edges_include_explicit_output_binding_and_derived_dependencies() {
        let (mut project, composition_id, track_id, clip_id, _solid_id, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .unwrap();
        let rects = render_test_graph(&project, composition_id);

        for connection in &project.connections {
            let id = format!("node_editor.edge:{}", connection.id);
            let rect = rects
                .get(&id)
                .unwrap_or_else(|| panic!("missing rendered explicit edge {id}"));
            assert!(rect.is_positive(), "explicit edge has empty bbox: {rect:?}");
        }

        let binding = format!(
            "node_editor.edge.output_binding:{}:image:{merge_id}",
            qa_container_key(PortOwner::Clip(clip_id))
        );
        assert!(
            rects.get(&binding).is_some_and(egui::Rect::is_positive),
            "missing Clip output binding edge"
        );

        let track_dependency = format!(
            "node_editor.edge.derived:{}:image:{}",
            qa_container_key(PortOwner::Track(track_id)),
            qa_container_key(PortOwner::Clip(clip_id))
        );
        assert!(
            rects
                .get(&track_dependency)
                .is_some_and(egui::Rect::is_positive),
            "missing evaluator-derived Clip → Track edge"
        );
    }

    #[test]
    fn clip_activity_and_disabled_state_have_distinct_inactive_reasons() {
        let (mut project, _, _, clip_id, solid_id, _) = fixture();
        let clip = project.get_clip(clip_id).unwrap();

        assert!(!clip_is_active(clip, 0.999));
        assert!(clip_is_active(clip, 1.0));
        assert!(clip_is_active(clip, 5.999));
        assert!(!clip_is_active(clip, 6.0));
        assert_eq!(
            graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 0.5),
            Some(GraphItemInactiveReason::OutsideClipRange)
        );
        assert_eq!(
            graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 1.0),
            None
        );

        project.get_node_mut(solid_id).unwrap().enabled = false;
        assert_eq!(
            graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 1.0),
            Some(GraphItemInactiveReason::Disabled)
        );
        // Disabled is the primary authored reason even when the Clip is also
        // outside its half-open active range.
        assert_eq!(
            graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 0.5),
            Some(GraphItemInactiveReason::Disabled)
        );
        assert!(graph_item_inactive(
            &project,
            GraphItem::Node(solid_id),
            0.5
        ));
    }

    #[test]
    fn alt_drag_knife_batches_explicit_and_output_binding_but_preserves_derived_wires(
    ) -> Result<(), String> {
        let (mut project, _, track_id, clip_id, _, merge_id) = fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .map_err(|error| error.to_string())?;
        let connection_ids = project
            .connections
            .iter()
            .map(|connection| connection.id)
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(connection_ids.len(), 2);
        let edges = vec![
            RenderedEdge {
                kind: RenderedEdgeKind::ProjectConnection {
                    connection_id: connection_ids[0],
                },
                start: egui::pos2(100.0, 160.0),
                control_a: egui::pos2(180.0, 120.0),
                control_b: egui::pos2(320.0, 200.0),
                end: egui::pos2(400.0, 160.0),
            },
            RenderedEdge {
                kind: RenderedEdgeKind::ProjectConnection {
                    connection_id: connection_ids[1],
                },
                start: egui::pos2(100.0, 230.0),
                control_a: egui::pos2(180.0, 190.0),
                control_b: egui::pos2(320.0, 270.0),
                end: egui::pos2(400.0, 230.0),
            },
            RenderedEdge {
                kind: RenderedEdgeKind::OutputBinding {
                    owner: PortOwner::Clip(clip_id),
                    node_id: merge_id,
                    data_type: PortDataType::Image,
                },
                start: egui::pos2(100.0, 300.0),
                control_a: egui::pos2(180.0, 260.0),
                control_b: egui::pos2(320.0, 340.0),
                end: egui::pos2(400.0, 300.0),
            },
            RenderedEdge {
                kind: RenderedEdgeKind::DerivedOutput {
                    owner: PortOwner::Track(track_id),
                    source: PortOwner::Clip(clip_id),
                    data_type: PortDataType::Image,
                },
                start: egui::pos2(100.0, 350.0),
                control_a: egui::pos2(180.0, 310.0),
                control_b: egui::pos2(320.0, 390.0),
                end: egui::pos2(400.0, 350.0),
            },
        ];
        assert!(knife_segment_hits_edge(
            egui::pos2(250.0, 100.0),
            egui::pos2(250.0, 290.0),
            &edges[0],
        ));
        assert!(!knife_segment_hits_edge(
            egui::pos2(460.0, 100.0),
            egui::pos2(460.0, 290.0),
            &edges[0],
        ));

        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
        let start = egui::pos2(250.0, 100.0);
        let end = egui::pos2(250.0, 390.0);
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let frames = vec![
            vec![egui::Event::PointerMoved(start)],
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: alt,
            }],
            vec![egui::Event::PointerMoved(egui::pos2(250.0, 245.0))],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: alt,
            }],
        ];
        let ports = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let mut queued = Vec::new();
        for (frame, events) in frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    modifiers: alt,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        queued.extend(wire_interactions(
                            ui,
                            &mut state,
                            WireInteractionFrame {
                                project: &project,
                                edges: &edges,
                                rendered_ports: &ports,
                                canvas_clip: screen,
                                graph_item_rects: &[],
                                to_global: egui::emath::TSTransform::IDENTITY,
                            },
                        ));
                    });
                },
            ));
        }
        let [QueuedNodeEdit::Atomic(NodeEdit::DisconnectWires { wires: crossed })] =
            queued.as_slice()
        else {
            return Err(format!("knife did not emit one batch: {queued:?}"));
        };
        let mut expected = connection_ids
            .iter()
            .copied()
            .map(|connection_id| NodeEditorEditableWire::ProjectConnection { connection_id })
            .chain(std::iter::once(NodeEditorEditableWire::OutputBinding {
                owner: PortOwner::Clip(clip_id),
                node_id: merge_id,
                data_type: PortDataType::Image,
            }))
            .collect::<Vec<_>>();
        expected.sort_by_key(|target| editable_wire_sort_key(*target));
        assert_eq!(crossed, &expected);
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        assert!(apply_queued_node_edits(
            &mut project,
            queued,
            &mut history,
            &mut state,
        ));
        assert!(project
            .connections
            .iter()
            .all(|connection| !connection_ids.contains(&connection.id)));
        let edited_clip = project
            .get_clip(clip_id)
            .ok_or_else(|| "knife removed its Clip".to_string())?;
        assert_eq!(edited_clip.output_node_id, None);
        assert_eq!(project.find_track_for_clip(clip_id), Some(track_id));
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
        Ok(())
    }

    #[test]
    fn zero_hit_knife_stroke_emits_no_edit_and_keeps_history_clean() {
        let (project, _, _, _, solid_id, merge_id) = fixture();
        let connection_id = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .unwrap()
            .id;
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection { connection_id },
            start: egui::pos2(100.0, 180.0),
            control_a: egui::pos2(200.0, 180.0),
            control_b: egui::pos2(300.0, 180.0),
            end: egui::pos2(400.0, 180.0),
        };
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let start = egui::pos2(500.0, 80.0);
        let end = egui::pos2(560.0, 120.0);
        let frames = vec![
            vec![egui::Event::PointerMoved(start)],
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: alt,
            }],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: alt,
            }],
        ];
        let ports = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let mut queued = Vec::new();
        for (frame, events) in frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    modifiers: alt,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        queued.extend(wire_interactions(
                            ui,
                            &mut state,
                            WireInteractionFrame {
                                project: &project,
                                edges: std::slice::from_ref(&edge),
                                rendered_ports: &ports,
                                canvas_clip: screen,
                                graph_item_rects: &[],
                                to_global: egui::emath::TSTransform::IDENTITY,
                            },
                        ));
                    });
                },
            ));
        }
        assert!(queued.is_empty());
        assert!(state.wire_knife.is_none());
    }

    #[test]
    fn moving_composition_moves_track_clip_and_nodes_without_changing_containment() {
        let (mut project, composition_id, track_id, clip_id, solid_id, _) = fixture();
        let original_clip_ids = project.get_track(track_id).unwrap().clip_ids.clone();
        let original_node_ids = project.get_clip(clip_id).unwrap().node_ids.clone();

        assert!(translate_container(
            &mut project,
            PortOwner::Composition(composition_id),
            [25.0, -15.0]
        ));

        assert_eq!(
            project.get_track(track_id).unwrap().ui_position,
            [135.0, 125.0]
        );
        assert_eq!(
            project.get_clip(clip_id).unwrap().ui_position,
            [285.0, 245.0]
        );
        assert_eq!(
            project.get_node(solid_id).unwrap().ui_position,
            [475.0, 375.0]
        );
        assert_eq!(
            project.get_track(track_id).unwrap().clip_ids,
            original_clip_ids
        );
        assert_eq!(
            project.get_clip(clip_id).unwrap().node_ids,
            original_node_ids
        );
        assert_eq!(project.connections.len(), 2);
    }

    #[test]
    fn clip_resize_and_timing_edits_persist_on_the_clip_only() {
        let (mut project, _, _, clip_id, solid_id, _) = fixture();
        assert!(set_container_size(
            &mut project,
            PortOwner::Clip(clip_id),
            [720.0, 520.0]
        ));
        assert!(apply_edit(
            &mut project,
            NodeEdit::SetProperty {
                owner: PortOwner::Clip(clip_id),
                key: "start_time".into(),
                time: 0.0,
                value: PropertyValue::Number(OrderedFloat(2.5)),
            }
        ));
        assert_eq!(project.get_clip(clip_id).unwrap().ui_size, [720.0, 520.0]);
        assert_eq!(
            project.get_clip(clip_id).unwrap().start_time.into_inner(),
            2.5
        );
        assert!(project.get_node(solid_id).is_some());
    }

    #[test]
    fn clip_owned_node_properties_use_local_time_for_evaluation_and_keyframe_edits() {
        let (mut project, composition_id, _, clip_id, solid_id, _) = fixture();
        {
            let clip = project.get_clip_mut(clip_id).unwrap();
            clip.start_time = OrderedFloat(4.0);
            clip.duration = OrderedFloat(10.0);
            clip.trim_in = OrderedFloat(1.25);
            clip.time_stretch = OrderedFloat(1.5);
        }
        let animated = Property::keyframe(vec![
            Keyframe::new(
                0.0,
                PropertyValue::Number(OrderedFloat(0.0)),
                EasingFunction::Linear,
            ),
            Keyframe::new(
                10.0,
                PropertyValue::Number(OrderedFloat(100.0)),
                EasingFunction::Linear,
            ),
        ]);
        project
            .get_node_mut(solid_id)
            .unwrap()
            .set_property("opacity".to_string(), animated.clone())
            .expect("solid factory initializes opacity");

        let global_time = 6.0;
        let inspector_and_renderer_time =
            project.get_clip(clip_id).unwrap().local_time(global_time);
        assert_eq!(inspector_and_renderer_time, 4.25);
        assert_eq!(
            node_property_time(&project, solid_id, global_time),
            inspector_and_renderer_time
        );
        assert_eq!(
            project
                .get_node(solid_id)
                .unwrap()
                .properties()
                .get("opacity")
                .unwrap()
                .evaluate_at(node_property_time(&project, solid_id, global_time))
                .unwrap(),
            PropertyValue::Number(OrderedFloat(42.5))
        );

        assert!(apply_edit(
            &mut project,
            NodeEdit::SetProperty {
                owner: PortOwner::Node(solid_id),
                key: "opacity".into(),
                time: inspector_and_renderer_time,
                value: PropertyValue::Number(OrderedFloat(91.0)),
            }
        ));
        let clip_node_property = project
            .get_node(solid_id)
            .unwrap()
            .properties()
            .get("opacity")
            .unwrap();
        assert_eq!(
            clip_node_property
                .evaluate_at(inspector_and_renderer_time)
                .unwrap(),
            PropertyValue::Number(OrderedFloat(91.0))
        );
        assert!(clip_node_property.has_keyframe_at(inspector_and_renderer_time, 0.001));
        assert!(!clip_node_property.has_keyframe_at(global_time, 0.001));

        let root_id = Uuid::from_u128(0x9_101);
        let mut root = generator_node(
            "Root",
            GeneratorNodeRequest::Solid {
                color: Color::default(),
            },
        );
        root.id = root_id;
        root.set_property("opacity".to_string(), animated)
            .expect("solid factory initializes opacity");
        project.add_node(root);
        project
            .attach_node_to_container(NodeContainer::Composition(composition_id), root_id)
            .unwrap();
        assert_eq!(
            node_property_time(&project, root_id, global_time),
            global_time,
            "Composition-owned Node time stays in the global domain"
        );
        let root_property_time = node_property_time(&project, root_id, global_time);
        assert!(apply_edit(
            &mut project,
            NodeEdit::SetProperty {
                owner: PortOwner::Node(root_id),
                key: "opacity".into(),
                time: root_property_time,
                value: PropertyValue::Number(OrderedFloat(55.0)),
            }
        ));
        let root_property = project
            .get_node(root_id)
            .unwrap()
            .properties()
            .get("opacity")
            .unwrap();
        assert!(root_property.has_keyframe_at(global_time, 0.001));
        assert_eq!(
            root_property.evaluate_at(global_time).unwrap(),
            PropertyValue::Number(OrderedFloat(55.0))
        );
    }

    #[test]
    fn numeric_drag_text_typing_and_color_popup_each_commit_one_undoable_gesture() {
        let (mut numeric_project, _, _, clip_id, solid_id, _) = fixture();
        {
            let clip = numeric_project.get_clip_mut(clip_id).unwrap();
            clip.start_time = OrderedFloat(4.0);
            clip.trim_in = OrderedFloat(1.25);
            clip.time_stretch = OrderedFloat(1.5);
        }
        let numeric_initial = numeric_project.clone();
        let numeric_time = node_property_time(&numeric_project, solid_id, 6.0);
        assert_eq!(numeric_time, 4.25);
        let mut numeric_history = HistoryManager::new();
        numeric_history.push_project_state(numeric_initial.clone());
        let mut numeric_state = NodeEditorState::default();
        for value in [10.0, 20.0, 30.0] {
            assert!(apply_queued_node_edits(
                &mut numeric_project,
                vec![queued_property_edit(
                    PortOwner::Node(solid_id),
                    "opacity",
                    numeric_time,
                    PropertyValue::Number(OrderedFloat(value)),
                    false,
                )],
                &mut numeric_history,
                &mut numeric_state,
            ));
            assert_eq!(numeric_history.undo_depth(), 1);
        }
        assert!(!apply_queued_node_edits(
            &mut numeric_project,
            vec![queued_finish(PortOwner::Node(solid_id), "opacity")],
            &mut numeric_history,
            &mut numeric_state,
        ));
        assert!(numeric_state.pending_continuous_edit.is_none());
        let numeric_edited = numeric_project.clone();
        assert_single_gesture_undo_redo(&mut numeric_history, &numeric_initial, &numeric_edited);

        let (mut text_project, _, _, _, text_node_id, _) = fixture();
        let text_initial = text_project.clone();
        let mut text_history = HistoryManager::new();
        text_history.push_project_state(text_initial.clone());
        let mut text_state = NodeEditorState::default();
        for name in ["S", "So", "Solid renamed"] {
            assert!(apply_queued_node_edits(
                &mut text_project,
                vec![QueuedNodeEdit::Continuous {
                    pending: NodeEditorPendingEdit {
                        owner: PortOwner::Node(text_node_id),
                        key: "$name".into(),
                    },
                    edit: Some(NodeEdit::Rename {
                        node_id: text_node_id,
                        name: name.into(),
                    }),
                    finished: false,
                }],
                &mut text_history,
                &mut text_state,
            ));
            assert_eq!(text_history.undo_depth(), 1);
        }
        apply_queued_node_edits(
            &mut text_project,
            vec![queued_finish(PortOwner::Node(text_node_id), "$name")],
            &mut text_history,
            &mut text_state,
        );
        let text_edited = text_project.clone();
        assert_single_gesture_undo_redo(&mut text_history, &text_initial, &text_edited);

        let (mut color_project, _, _, _, color_node_id, _) = fixture();
        let color_initial = color_project.clone();
        let mut color_history = HistoryManager::new();
        color_history.push_project_state(color_initial.clone());
        let mut color_state = NodeEditorState::default();
        for color in [
            library::model::frame::color::Color {
                r: 20,
                g: 30,
                b: 40,
                a: 255,
            },
            library::model::frame::color::Color {
                r: 80,
                g: 90,
                b: 100,
                a: 220,
            },
        ] {
            assert!(apply_queued_node_edits(
                &mut color_project,
                vec![queued_property_edit(
                    PortOwner::Node(color_node_id),
                    "color",
                    0.0,
                    PropertyValue::Color(color),
                    false,
                )],
                &mut color_history,
                &mut color_state,
            ));
            assert_eq!(color_history.undo_depth(), 1);
        }
        apply_queued_node_edits(
            &mut color_project,
            vec![queued_finish(PortOwner::Node(color_node_id), "color")],
            &mut color_history,
            &mut color_state,
        );
        let color_edited = color_project.clone();
        assert_single_gesture_undo_redo(&mut color_history, &color_initial, &color_edited);
    }

    #[test]
    fn owner_or_control_switch_flushes_previous_edit_and_atomic_checkbox_commits_immediately() {
        let (mut project, _, _, _, node_id, _) = fixture();
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial);
        let mut state = NodeEditorState::default();

        apply_queued_node_edits(
            &mut project,
            vec![queued_property_edit(
                PortOwner::Node(node_id),
                "opacity",
                0.0,
                PropertyValue::Number(OrderedFloat(25.0)),
                false,
            )],
            &mut history,
            &mut state,
        );
        let after_numeric = project.clone();
        assert_eq!(history.undo_depth(), 1);

        apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Continuous {
                pending: NodeEditorPendingEdit {
                    owner: PortOwner::Node(node_id),
                    key: "$name".into(),
                },
                edit: Some(NodeEdit::Rename {
                    node_id,
                    name: "switched control".into(),
                }),
                finished: false,
            }],
            &mut history,
            &mut state,
        );
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(&project), Some(after_numeric.clone()));
        assert_eq!(history.redo(&after_numeric), Some(project.clone()));

        let before_owner_switch = project.clone();
        let project_lock = Arc::new(RwLock::new(project));
        assert!(flush_pending_continuous_edit(
            &project_lock,
            &mut history,
            &mut state,
        ));
        assert_eq!(history.undo_depth(), 3);
        assert!(state.pending_continuous_edit.is_none());
        assert_eq!(
            history.undo(&before_owner_switch),
            Some(after_numeric.clone())
        );
        assert_eq!(
            history.redo(&after_numeric),
            Some(before_owner_switch.clone())
        );

        let mut project = project_lock.read().unwrap().clone();
        apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Atomic(NodeEdit::SetEnabled {
                node_id,
                enabled: false,
            })],
            &mut history,
            &mut state,
        );
        assert_eq!(history.undo_depth(), 4);
        assert!(state.pending_continuous_edit.is_none());
    }

    #[test]
    fn zero_time_stretch_is_preserved_as_freeze_and_negative_input_is_rejected() {
        let (mut project, _, _, clip_id, _, _) = fixture();
        {
            let clip = project.get_clip_mut(clip_id).unwrap();
            clip.trim_in = OrderedFloat(2.25);
            clip.time_stretch = OrderedFloat(0.0);
        }
        let serialized = serde_json::to_string(&project).unwrap();
        let mut loaded: Project = serde_json::from_str(&serialized).unwrap();
        let loaded_value = loaded.get_clip(clip_id).unwrap().time_stretch.into_inner();
        assert_eq!(
            Clip::validate_timing_property_value(
                "time_stretch",
                &PropertyValue::Number(OrderedFloat(loaded_value)),
            )
            .unwrap(),
            0.0
        );
        assert!(Clip::validate_timing_property_value(
            "time_stretch",
            &PropertyValue::Number(OrderedFloat(-0.5)),
        )
        .is_err());

        assert!(apply_edit(
            &mut loaded,
            NodeEdit::SetProperty {
                owner: PortOwner::Clip(clip_id),
                key: "time_stretch".into(),
                time: 9.0,
                value: PropertyValue::Number(OrderedFloat(loaded_value)),
            }
        ));
        let clip = loaded.get_clip(clip_id).unwrap();
        assert_eq!(clip.time_stretch, OrderedFloat(0.0));
        assert_eq!(clip.local_time(clip.start_time.into_inner()), 2.25);
        assert_eq!(clip.local_time(clip.start_time.into_inner() + 100.0), 2.25);
    }

    #[test]
    fn deleting_a_clip_removes_only_its_owned_leaf_nodes() {
        let (mut project, _, track_id, clip_id, solid_id, merge_id) = fixture();
        assert!(apply_edit(
            &mut project,
            NodeEdit::Delete {
                owner: PortOwner::Clip(clip_id),
            }
        ));
        assert!(project.get_clip(clip_id).is_none());
        assert!(project.get_node(solid_id).is_none());
        assert!(project.get_node(merge_id).is_none());
        assert!(project.get_track(track_id).is_some());
        assert!(project.connections.is_empty());
    }

    #[test]
    fn scc_ranking_is_deterministic_and_keeps_cycles_in_one_rank() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let c = Uuid::from_u128(3);
        let nodes = vec![c, b, a];
        let edges = vec![(a, b), (b, a), (b, c)];
        let ranks = rank_nodes_by_scc(&nodes, &edges);
        assert_eq!(ranks[&a], ranks[&b]);
        assert!(ranks[&b] < ranks[&c]);
    }

    #[test]
    fn full_layout_preserves_hierarchy_non_overlap_and_global_ltr_in_rendered_rects() {
        let (mut project, ids) = adversarial_hierarchy_fixture();
        assert!(layout_needs_reflow(&project, ids.composition));

        let full = compute_full_composition_layout(&project, ids.composition).unwrap();
        for scope in [
            AutoLayoutScope::All,
            AutoLayoutScope::Container(PortOwner::Composition(ids.composition)),
        ] {
            assert_eq!(
                compute_auto_layout(&project, ids.composition, scope).unwrap(),
                full
            );
        }

        assert!(apply_auto_layout(&mut project, ids.composition, &full));
        assert!(!layout_needs_reflow(&project, ids.composition));

        let rects = render_test_graph(&project, ids.composition);
        let composition = project.get_composition(ids.composition).unwrap();
        let composition_rect = rects
            .get(&format!(
                "node_editor.container.{}",
                qa_container_key(PortOwner::Composition(ids.composition))
            ))
            .copied()
            .unwrap();
        let scale = composition_rect.width() / composition.ui_size[0];
        let screen_padding = AUTO_LAYOUT_NODE_PADDING * scale;

        let node_ids = [
            ids.solid,
            ids.merge,
            ids.text,
            ids.text_fill,
            ids.shape,
            ids.shape_fill,
            ids.composition_node,
        ];
        let rendered_nodes = node_ids
            .iter()
            .map(|node_id| {
                let rect = rects
                    .get(&format!("node_editor.node:{node_id}"))
                    .copied()
                    .unwrap();
                let estimated = estimated_node_size(&project, *node_id);
                assert!(
                    rect.width() <= estimated.x * scale + 1.0,
                    "{} ({node_id}): {rect:?}, estimated={estimated:?}, scale={scale}",
                    node_title(&project, *node_id),
                );
                assert!(
                    rect.height() <= estimated.y * scale + 1.0,
                    "{} ({node_id}): {rect:?}, estimated={estimated:?}, scale={scale}",
                    node_title(&project, *node_id),
                );
                (*node_id, rect)
            })
            .collect::<HashMap<_, _>>();

        for (index, left_id) in node_ids.iter().enumerate() {
            for right_id in &node_ids[index + 1..] {
                let left = rendered_nodes[left_id].expand(screen_padding * 0.5);
                let right = rendered_nodes[right_id].expand(screen_padding * 0.5);
                assert!(
                    !left.intersects(right),
                    "rendered Nodes overlap: {left_id} {left:?}, {right_id} {right:?}"
                );
            }
        }

        let immediate_owners = [
            (ids.solid, PortOwner::Clip(ids.clip)),
            (ids.merge, PortOwner::Clip(ids.clip)),
            (ids.text, PortOwner::Clip(ids.sibling_clip)),
            (ids.text_fill, PortOwner::Clip(ids.sibling_clip)),
            (ids.shape, PortOwner::Track(ids.track)),
            (ids.shape_fill, PortOwner::Track(ids.track)),
            (
                ids.composition_node,
                PortOwner::Composition(ids.composition),
            ),
        ];
        for (node_id, owner) in immediate_owners {
            let owner_key = qa_container_key(owner);
            let owner_rect = rects
                .get(&format!("node_editor.container.{owner_key}"))
                .copied()
                .unwrap();
            let node_rect = rendered_nodes[&node_id];
            assert!(
                rect_contains_rect(owner_rect.expand(0.5), node_rect),
                "{node_id} escaped immediate owner {owner_key}: {node_rect:?} not in {owner_rect:?}"
            );
            assert!(
                node_rect.top() >= owner_rect.top() + CONTAINER_HEADER_HEIGHT * scale - 1.0,
                "{node_id} overlaps integrated header of {owner_key}"
            );
        }

        let sibling_clips = [ids.clip, ids.sibling_clip, ids.empty_clip];
        for (index, left_id) in sibling_clips.iter().enumerate() {
            let left = rects
                .get(&format!(
                    "node_editor.container.{}",
                    qa_container_key(PortOwner::Clip(*left_id))
                ))
                .copied()
                .unwrap()
                .expand(screen_padding * 0.5);
            for right_id in &sibling_clips[index + 1..] {
                let right = rects
                    .get(&format!(
                        "node_editor.container.{}",
                        qa_container_key(PortOwner::Clip(*right_id))
                    ))
                    .copied()
                    .unwrap()
                    .expand(screen_padding * 0.5);
                assert!(
                    !left.intersects(right),
                    "Clip overlap: {left:?} / {right:?}"
                );
            }
        }

        let track = rects
            .get(&format!(
                "node_editor.container.{}",
                qa_container_key(PortOwner::Track(ids.track))
            ))
            .copied()
            .unwrap();
        let empty_track = rects
            .get(&format!(
                "node_editor.container.{}",
                qa_container_key(PortOwner::Track(ids.empty_track))
            ))
            .copied()
            .unwrap();
        assert!(!track.intersects(empty_track));
        assert!(rect_contains_rect(composition_rect.expand(0.5), track));
        assert!(rect_contains_rect(
            composition_rect.expand(0.5),
            empty_track
        ));

        let all_nodes = composition_graph_node_ids(&project, ids.composition);
        let edges = canonical_edges(&project, &all_nodes);
        let ranks = rank_nodes_by_scc(&all_nodes, &edges);
        for (from, to) in edges {
            if ranks[&from] == ranks[&to] {
                continue;
            }
            let from_rect = rendered_nodes[&from];
            let to_rect = rendered_nodes[&to];
            assert!(
                from_rect.right() + screen_padding <= to_rect.left() + 1.0,
                "canonical edge is not LTR: {from} {from_rect:?} -> {to} {to_rect:?}"
            );
        }
    }

    #[test]
    fn scoped_layouts_leave_every_out_of_scope_entity_byte_identical() {
        let (mut base, ids) = adversarial_hierarchy_fixture();
        let full = compute_full_composition_layout(&base, ids.composition).unwrap();
        apply_auto_layout(&mut base, ids.composition, &full);
        assert!(!layout_needs_reflow(&base, ids.composition));

        let mut track_project = base.clone();
        let unrelated_track_before = track_project.get_track(ids.empty_track).unwrap().clone();
        let root_node_before = track_project
            .get_node(ids.composition_node)
            .unwrap()
            .clone();
        let track_plan = compute_auto_layout(
            &track_project,
            ids.composition,
            AutoLayoutScope::Container(PortOwner::Track(ids.track)),
        )
        .unwrap();
        assert!(!track_plan.track_layouts.contains_key(&ids.empty_track));
        assert!(!track_plan
            .node_positions
            .contains_key(&ids.composition_node));
        apply_auto_layout(&mut track_project, ids.composition, &track_plan);
        assert_eq!(
            track_project.get_track(ids.empty_track).unwrap(),
            &unrelated_track_before
        );
        assert_eq!(
            track_project.get_node(ids.composition_node).unwrap(),
            &root_node_before
        );
        assert!(!layout_needs_reflow(&track_project, ids.composition));

        let mut clip_project = base.clone();
        let sibling_before = clip_project.get_clip(ids.sibling_clip).unwrap().clone();
        let text_before = clip_project.get_node(ids.text).unwrap().clone();
        let shape_before = clip_project.get_node(ids.shape).unwrap().clone();
        let clip_plan = compute_auto_layout(
            &clip_project,
            ids.composition,
            AutoLayoutScope::Container(PortOwner::Clip(ids.clip)),
        )
        .unwrap();
        assert!(!clip_plan.clip_layouts.contains_key(&ids.sibling_clip));
        assert!(!clip_plan.node_positions.contains_key(&ids.text));
        assert!(!clip_plan.node_positions.contains_key(&ids.shape));
        apply_auto_layout(&mut clip_project, ids.composition, &clip_plan);
        assert_eq!(
            clip_project.get_clip(ids.sibling_clip).unwrap(),
            &sibling_before
        );
        assert_eq!(clip_project.get_node(ids.text).unwrap(), &text_before);
        assert_eq!(clip_project.get_node(ids.shape).unwrap(), &shape_before);
        assert!(
            !layout_needs_reflow(&clip_project, ids.composition),
            "Clip-scoped plan violated layout invariants: {clip_plan:#?}"
        );

        let mut selection_project = base.clone();
        let merge_position = selection_project.get_node(ids.merge).unwrap().ui_position;
        selection_project
            .get_node_mut(ids.solid)
            .unwrap()
            .ui_position = merge_position;
        let merge_before = selection_project.get_node(ids.merge).unwrap().clone();
        let text_before = selection_project.get_node(ids.text).unwrap().clone();
        let selection_plan = compute_auto_layout(
            &selection_project,
            ids.composition,
            AutoLayoutScope::Selection(vec![ids.solid]),
        )
        .unwrap();
        assert_eq!(
            selection_plan
                .node_positions
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![ids.solid]
        );
        apply_auto_layout(&mut selection_project, ids.composition, &selection_plan);
        assert_eq!(
            selection_project.get_node(ids.merge).unwrap(),
            &merge_before
        );
        assert_eq!(selection_project.get_node(ids.text).unwrap(), &text_before);
        assert!(!layout_needs_reflow(&selection_project, ids.composition));
    }

    #[test]
    fn new_nodes_use_a_dependency_near_free_slot_inside_clip() {
        let (mut project, _, _, clip_id, existing_id, _) = fixture();
        let node = Node::new_merge("New");
        let node_id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();
        let placed = place_node_in_free_slot(
            &mut project,
            node_id,
            NodeContainer::Clip(clip_id),
            egui::Pos2::ZERO,
            &[existing_id],
        )
        .unwrap();
        let existing = project.get_node(existing_id).unwrap();
        assert!(
            placed.x >= existing.ui_position[0] + estimated_node_width() + AUTO_LAYOUT_COLUMN_GAP
        );
        let placed_rect = egui::Rect::from_min_size(placed, estimated_node_size(&project, node_id));
        let clip = project.get_clip(clip_id).unwrap();
        let clip_rect = container_rect(clip.ui_position, clip.ui_size);
        assert!(clip_rect.contains(placed_rect.min));
        assert!(clip_rect.contains(placed_rect.max));
    }

    #[test]
    fn collapsed_reparent_excludes_both_the_header_and_stored_body() {
        let (mut project, composition_id, track_id, clip_id, _, _) = fixture();
        let clip_header = egui::pos2(500.0, 280.0);
        let hidden_clip_body = egui::pos2(500.0, 400.0);

        assert_eq!(
            node_container_at_position(&project, composition_id, clip_header),
            Some(NodeContainer::Clip(clip_id))
        );

        let clip = project.get_clip_mut(clip_id).unwrap();
        clip.ui_collapsed = true;
        assert!(container_rect(clip.ui_position, clip.ui_size).contains(hidden_clip_body));
        let collapsed_geometry = reparent_container_geometries(&project, composition_id)
            .into_iter()
            .find(|geometry| geometry.container == NodeContainer::Clip(clip_id));
        assert!(collapsed_geometry.is_some_and(|geometry| {
            reparent_ineligible_reason(geometry) == Some("collapsed_hidden_content")
        }));

        let node_id = project.get_clip(clip_id).unwrap().node_ids[0];
        // A collapsed owner has no valid content surface. Neither its visible
        // header nor its stored hidden body may become a semantic destination;
        // an explicit expand-on-hover policy can add that later without moving
        // the header or violating exact drop coordinates.
        let node_size = estimated_node_size(&project, node_id);
        let hidden_rect = egui::Rect::from_center_size(hidden_clip_body, node_size);
        assert_eq!(
            deepest_legal_reparent_target(&project, composition_id, hidden_rect, hidden_clip_body,)
                .map(|target| target.container),
            Some(NodeContainer::Track(track_id))
        );
        let header_rect = egui::Rect::from_center_size(clip_header, node_size);
        assert_eq!(
            deepest_legal_reparent_target(&project, composition_id, header_rect, clip_header,)
                .map(|target| target.container),
            Some(NodeContainer::Track(track_id))
        );

        assert!(reparent_test_node_at_drop(
            &mut project,
            composition_id,
            node_id,
            hidden_clip_body,
        ));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Track(track_id))
        );
        project.get_clip_mut(clip_id).unwrap().ui_collapsed = false;
        assert!(project
            .attach_node_to_container(NodeContainer::Track(track_id), node_id)
            .is_ok());
        let expanded_drop = project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
            .center()
        });
        assert!(expanded_drop.is_some());
        let Some(expanded_drop) = expanded_drop else {
            return;
        };
        assert!(reparent_test_node_at_drop(
            &mut project,
            composition_id,
            node_id,
            expanded_drop,
        ));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );

        project.get_track_mut(track_id).unwrap().ui_collapsed = true;
        let track = project.get_track(track_id).unwrap();
        let track_header = egui::pos2(
            track.ui_position[0] + 24.0,
            track.ui_position[1] + CONTAINER_HEADER_HEIGHT * 0.5,
        );
        let node_size = estimated_node_size(&project, node_id);
        let track_header_rect = egui::Rect::from_center_size(track_header, node_size);
        assert_eq!(
            deepest_legal_reparent_target(
                &project,
                composition_id,
                track_header_rect,
                track_header,
            )
            .map(|target| target.container),
            Some(NodeContainer::Composition(composition_id))
        );
        let hidden_track_body = egui::pos2(track_header.x, track_header.y + 180.0);
        let hidden_track_rect = egui::Rect::from_center_size(hidden_track_body, node_size);
        assert_eq!(
            deepest_legal_reparent_target(
                &project,
                composition_id,
                hidden_track_rect,
                hidden_track_body,
            )
            .map(|target| target.container),
            Some(NodeContainer::Composition(composition_id))
        );
        assert!(project.validate_containment().is_empty());
    }

    #[test]
    fn reparent_overlap_uses_deepest_legal_content_and_authoritative_stacking_order() {
        let (mut project, composition_id, track_id, clip_id, node_id, _) = fixture();
        let Some(first_clip) = project.get_clip(clip_id).cloned() else {
            assert!(project.get_clip(clip_id).is_some());
            return;
        };
        let mut second_clip = Clip::new("Overlapping Clip", 0.0, 5.0);
        second_clip.ui_position = first_clip.ui_position;
        second_clip.ui_size = first_clip.ui_size;
        let second_clip_id = second_clip.id;
        project.add_clip(second_clip);
        assert!(project
            .attach_clip_to_track(track_id, second_clip_id)
            .is_ok());

        let first_rect = container_rect(first_clip.ui_position, first_clip.ui_size);
        let content = nested_content_rect(first_rect, AUTO_LAYOUT_CLIP_TOP);
        let node_size = estimated_node_size(&project, node_id);
        let node_position = content.center() - node_size * 0.5;
        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        let Some(node) = project.get_node_mut(node_id) else {
            assert!(project.get_node(node_id).is_some());
            return;
        };
        node.ui_position = [node_position.x, node_position.y];
        let node_rect = egui::Rect::from_min_size(node_position, node_size);
        let drop_point = node_rect.center();
        let selected =
            deepest_legal_reparent_target(&project, composition_id, node_rect, drop_point);
        assert_eq!(
            selected.map(|target| target.container),
            Some(NodeContainer::Clip(second_clip_id))
        );
        assert!(selected.is_some_and(|target| target.depth == 2 && target.center_inside));
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, node_rect)]),
            &HashMap::from([(node_id, [node_position.x, node_position.y])]),
            drop_point,
            1.0,
        );
        assert_eq!(intents.len(), 1);
        assert!(intents.first().is_some_and(|intent| {
            intent.target.container == NodeContainer::Clip(second_clip_id)
        }));

        let Some(second_clip) = project.get_clip_mut(second_clip_id) else {
            assert!(project.get_clip(second_clip_id).is_some());
            return;
        };
        second_clip.ui_collapsed = true;
        assert_eq!(
            deepest_legal_reparent_target(&project, composition_id, node_rect, drop_point,)
                .map(|target| target.container),
            Some(NodeContainer::Clip(clip_id))
        );
    }

    #[test]
    fn reparent_intent_keeps_origin_across_header_padding_until_node_fully_exits() {
        let (mut project, composition_id, track_id, clip_id, node_id, _) = fixture();
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [1_800.0, 1_000.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_200.0, 1_400.0];
        }
        let exact_node_size = egui::vec2(120.0, 60.0);
        let origin_position = project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
            .min + egui::vec2(32.0, 32.0)
        });
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [origin_position.x, origin_position.y];
        }
        let Some(origin_node) = project.get_node(node_id) else {
            assert!(project.get_node(node_id).is_some());
            return;
        };
        let origin = NodeEditorNodeDragOrigin {
            container: NodeContainer::Clip(clip_id),
            position: origin_node.ui_position,
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(node_id, origin)]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let small_min = egui::pos2(origin.position[0] + 4.0, origin.position[1] + 3.0);
        let small_rect = egui::Rect::from_min_size(small_min, exact_node_size);
        assert!(node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, small_rect)]),
            &HashMap::from([(node_id, [small_min.x, small_min.y])]),
            small_rect.center(),
            1.0,
        )
        .is_empty());
        let mut non_node_state = NodeEditorState::default();
        record_node_reparent_origins(
            &project,
            &[
                LayoutEdit::MoveContainer {
                    owner: PortOwner::Track(track_id),
                    delta: [48.0, 24.0],
                },
                LayoutEdit::ResizeContainer {
                    owner: PortOwner::Clip(clip_id),
                    position: [200.0, 200.0],
                    size: [800.0, 500.0],
                },
            ],
            &mut non_node_state,
            true,
        );
        assert!(non_node_state.node_reparent.is_none());
        let Some(clip) = project.get_clip(clip_id) else {
            return;
        };
        let clip_rect = container_rect(clip.ui_position, clip.ui_size);
        let padding_min = clip_rect.min + egui::vec2(12.0, 12.0);
        let padding_rect = egui::Rect::from_min_size(padding_min, exact_node_size);
        let padding_positions = HashMap::from([(node_id, [padding_min.x, padding_min.y])]);
        let padding_rects = HashMap::from([(node_id, padding_rect)]);
        let padding_intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &padding_rects,
            &padding_positions,
            padding_rect.center(),
            1.0,
        );
        assert_eq!(padding_intents.len(), 1);
        assert!(padding_intents.first().is_some_and(|intent| {
            intent.target.container == NodeContainer::Clip(clip_id)
                && intent.target.kind == ReparentTargetKind::RetainedOrigin
        }));
        let mut retained_project = project.clone();
        if let Some(node) = retained_project.get_node_mut(node_id) {
            node.ui_position = [padding_min.x, padding_min.y];
        }
        assert!(reparent_nodes_from_intents(
            &mut retained_project,
            &padding_intents,
        ));
        assert_eq!(
            retained_project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );
        let retained_content = retained_project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
        });
        assert!(
            retained_content.is_some_and(|content| { rect_contains_rect(content, padding_rect) })
        );
        assert!(retained_project.validate_containment().is_empty());

        let exited_min = egui::pos2(1_120.0, 470.0);
        let exited_rect = egui::Rect::from_min_size(exited_min, exact_node_size);
        let exited_positions = HashMap::from([(node_id, [exited_min.x, exited_min.y])]);
        let exited_rects = HashMap::from([(node_id, exited_rect)]);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &exited_rects,
            &exited_positions,
            exited_rect.center(),
            1.0,
        );
        assert_eq!(intents.len(), 1);
        assert!(intents
            .first()
            .is_some_and(|intent| intent.target.container == NodeContainer::Track(track_id)));
    }

    #[test]
    fn subthreshold_screen_drag_repairs_containment_without_changing_owner() {
        let (mut project, composition_id, _, clip_id, _, node_id) = fixture();
        let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
        assert!(plan.is_some());
        let Some(plan) = plan else {
            return;
        };
        apply_auto_layout(&mut project, composition_id, &plan);
        assert!(!layout_needs_reflow(&project, composition_id));
        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let screen_delta = NODE_REPARENT_DRAG_THRESHOLD - 1.0;
        let graph_delta = screen_delta / NODE_EDITOR_MIN_SCALE;
        let final_min = egui::pos2(origin_position[0] + graph_delta, origin_position[1]);
        let exact_rect =
            egui::Rect::from_min_size(final_min, estimated_node_size(&project, node_id) * 0.4);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, exact_rect)]),
            &HashMap::from([(node_id, [final_min.x, final_min.y])]),
            exact_rect.center(),
            NODE_EDITOR_MIN_SCALE,
        );
        assert_eq!(intents.len(), 1);
        assert!(intents.first().is_some_and(|intent| {
            intent.target.container == NodeContainer::Clip(clip_id)
                && intent.target.kind == ReparentTargetKind::RetainedOrigin
        }));
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        assert!(reparent_nodes_from_intents(&mut project, &intents));
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );
        let estimated_rect = estimated_node_rect(&project, node_id);
        let content = project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
        });
        assert!(content
            .zip(estimated_rect)
            .is_some_and(|(content, estimated)| rect_contains_rect(content, estimated)));
        assert!(!layout_needs_reflow(&project, composition_id));
    }

    #[test]
    fn retained_growth_rolls_back_when_it_would_overlap_a_sibling_container() {
        let (mut project, composition_id, track_id, clip_id, _, node_id) = fixture();
        let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
        assert!(plan.is_some());
        let Some(plan) = plan else {
            return;
        };
        apply_auto_layout(&mut project, composition_id, &plan);
        let first_clip = project.get_clip(clip_id).cloned();
        assert!(first_clip.is_some());
        let Some(first_clip) = first_clip else {
            return;
        };
        let first_rect = container_rect(first_clip.ui_position, first_clip.ui_size);
        let mut sibling = Clip::new("Sibling", 0.0, 5.0);
        sibling.ui_position = [
            first_rect.right() + AUTO_LAYOUT_CLIP_GAP,
            first_clip.ui_position[1],
        ];
        sibling.ui_size = first_clip.ui_size;
        let sibling_id = sibling.id;
        project.add_clip(sibling);
        assert!(project.attach_clip_to_track(track_id, sibling_id).is_ok());
        let sibling_rect = project
            .get_clip(sibling_id)
            .map(|clip| container_rect(clip.ui_position, clip.ui_size));
        assert!(sibling_rect.is_some());
        let Some(sibling_rect) = sibling_rect else {
            return;
        };
        ensure_container_hierarchy_contains(
            &mut project,
            NodeContainer::Track(track_id),
            sibling_rect,
        );
        assert!(!layout_needs_reflow(&project, composition_id));

        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let graph_delta = (NODE_REPARENT_DRAG_THRESHOLD - 1.0) / NODE_EDITOR_MIN_SCALE;
        let final_min = egui::pos2(origin_position[0] + graph_delta, origin_position[1]);
        let final_rect =
            egui::Rect::from_min_size(final_min, estimated_node_size(&project, node_id) * 0.4);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, final_rect)]),
            &HashMap::from([(node_id, [final_min.x, final_min.y])]),
            final_rect.center(),
            NODE_EDITOR_MIN_SCALE,
        );
        assert_eq!(intents.len(), 1);
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        let initial_clip_geometry = project
            .get_clip(clip_id)
            .map(|clip| (clip.ui_position, clip.ui_size));
        assert_eq!(
            finish_node_reparent(&mut project, &intents, Some(&gesture)),
            ReparentReleaseOutcome::Rejected
        );
        assert_eq!(
            project.get_node(node_id).map(|node| node.ui_position),
            Some(origin_position)
        );
        assert_eq!(
            project
                .get_clip(clip_id)
                .map(|clip| (clip.ui_position, clip.ui_size)),
            initial_clip_geometry
        );
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );
        assert!(!layout_needs_reflow(&project, composition_id));
    }

    #[test]
    fn reparent_applies_when_only_node_overlap_requires_optional_auto_layout() {
        let (mut project, composition_id, track_id, clip_id, _, node_id) = fixture();
        let overlap_node = Node::new_merge("Intentional overlap");
        let overlap_node_id = overlap_node.id;
        project.add_node(overlap_node);
        assert!(project
            .attach_node_to_container(NodeContainer::Track(track_id), overlap_node_id)
            .is_ok());
        let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
        assert!(plan.is_some());
        let Some(plan) = plan else {
            return;
        };
        assert!(apply_auto_layout(&mut project, composition_id, &plan));
        assert!(!container_hierarchy_needs_reflow(&project, composition_id));

        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        let final_position = project
            .get_node(overlap_node_id)
            .map(|node| node.ui_position);
        assert!(origin_position.is_some());
        assert!(final_position.is_some());
        let (Some(origin_position), Some(final_position)) = (origin_position, final_position)
        else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let final_rect = egui::Rect::from_min_size(
            egui::pos2(final_position[0], final_position[1]),
            estimated_node_size(&project, node_id),
        );
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &HashMap::from([(node_id, final_rect)]),
            &HashMap::from([(node_id, final_position)]),
            final_rect.center(),
            1.0,
        );
        assert!(intents
            .first()
            .is_some_and(|intent| { intent.target.container == NodeContainer::Track(track_id) }));
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = final_position;
        }

        assert_eq!(
            finish_node_reparent(&mut project, &intents, Some(&gesture)),
            ReparentReleaseOutcome::Applied
        );
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project.get_node(node_id).map(|node| node.ui_position),
            Some(final_position)
        );
        assert!(layout_needs_reflow(&project, composition_id));
        assert!(!container_hierarchy_needs_reflow(&project, composition_id));
    }

    #[test]
    fn multi_node_drag_uses_primary_target_without_splitting_the_group() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [2_100.0, 1_000.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_500.0, 1_400.0];
        }
        let Some(solid) = project.get_node(solid_id) else {
            return;
        };
        let Some(merge) = project.get_node(merge_id) else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([
                (
                    solid_id,
                    NodeEditorNodeDragOrigin {
                        container: NodeContainer::Clip(clip_id),
                        position: solid.ui_position,
                    },
                ),
                (
                    merge_id,
                    NodeEditorNodeDragOrigin {
                        container: NodeContainer::Clip(clip_id),
                        position: merge.ui_position,
                    },
                ),
            ]),
            primary_node_id: Some(solid_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let solid_min = egui::pos2(1_120.0, 470.0);
        let merge_min = egui::pos2(1_440.0, 470.0);
        let solid_rect =
            egui::Rect::from_min_size(solid_min, estimated_node_size(&project, solid_id));
        let merge_rect =
            egui::Rect::from_min_size(merge_min, estimated_node_size(&project, merge_id));
        let final_positions = HashMap::from([
            (solid_id, [solid_min.x, solid_min.y]),
            (merge_id, [merge_min.x, merge_min.y]),
        ]);
        let final_rects = HashMap::from([(solid_id, solid_rect), (merge_id, merge_rect)]);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &final_rects,
            &final_positions,
            solid_rect.center(),
            1.0,
        );
        assert_eq!(intents.len(), 2);
        assert!(intents
            .iter()
            .all(|intent| intent.target.container == NodeContainer::Track(track_id)));

        if let Some(node) = project.get_node_mut(solid_id) {
            node.ui_position = [solid_min.x, solid_min.y];
        }
        if let Some(node) = project.get_node_mut(merge_id) {
            node.ui_position = [merge_min.x, merge_min.y];
        }
        let connection_ids = project
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        assert!(reparent_nodes_from_intents(&mut project, &intents));
        assert_eq!(
            project.find_node_container(solid_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project.find_node_container(merge_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project
                .connections
                .iter()
                .map(|connection| connection.id)
                .collect::<Vec<_>>(),
            connection_ids
        );
        assert_eq!(
            project
                .get_node(merge_id)
                .map(|node| node.ui_position[0])
                .zip(project.get_node(solid_id).map(|node| node.ui_position[0]))
                .map(|(merge_x, solid_x)| merge_x - solid_x),
            Some(320.0)
        );
    }

    #[test]
    fn reparent_min_edge_growth_keeps_node_drop_position_and_contains_every_ancestor() {
        let (mut project, composition_id, track_id, clip_id, node_id, _) = fixture();
        let other_node_ids = project
            .get_clip(clip_id)
            .map(|clip| {
                clip.node_ids
                    .iter()
                    .copied()
                    .filter(|candidate| *candidate != node_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for other_node_id in other_node_ids {
            project.remove_node(other_node_id);
        }
        let Some(composition) = project.get_composition_mut(composition_id) else {
            assert!(project.get_composition(composition_id).is_some());
            return;
        };
        composition.ui_position = [100.0, 100.0];
        composition.ui_size = [1_700.0, 1_300.0];
        let Some(track) = project.get_track_mut(track_id) else {
            assert!(project.get_track(track_id).is_some());
            return;
        };
        track.ui_position = [300.0, 240.0];
        track.ui_size = [1_100.0, 900.0];
        let Some(clip) = project.get_clip_mut(clip_id) else {
            assert!(project.get_clip(clip_id).is_some());
            return;
        };
        clip.ui_position = [490.0, 380.0];
        clip.ui_size = [800.0, 600.0];
        assert!(project
            .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
            .is_ok());

        let clip_before = project.get_clip(clip_id).map(|clip| clip.ui_position);
        let track_before = project.get_track(track_id).map(|track| track.ui_position);
        let composition_before = project
            .get_composition(composition_id)
            .map(|composition| composition.ui_position);
        assert!(clip_before.is_some() && track_before.is_some() && composition_before.is_some());
        let Some(clip) = project.get_clip(clip_id) else {
            return;
        };
        let clip_content = nested_content_rect(
            container_rect(clip.ui_position, clip.ui_size),
            AUTO_LAYOUT_CLIP_TOP,
        );
        let node_size = estimated_node_size(&project, node_id);
        let exact_snarl_size = node_size * 0.45;
        let final_min = clip_content.min - exact_snarl_size * 0.4;
        let final_rect = egui::Rect::from_min_size(final_min, exact_snarl_size);
        let Some(node) = project.get_node_mut(node_id) else {
            return;
        };
        node.ui_position = [final_min.x, final_min.y];
        let target = deepest_legal_reparent_target(
            &project,
            composition_id,
            final_rect,
            clip_content.min + egui::vec2(2.0, 2.0),
        );
        assert!(
            target.is_some(),
            "partially overlapping final Node rect had no legal target"
        );
        let Some(target) = target else {
            return;
        };
        assert_eq!(target.container, NodeContainer::Clip(clip_id));
        assert!(reparent_nodes_from_intents(
            &mut project,
            &[NodeDropIntent {
                node_id,
                final_rect,
                target,
            }],
        ));

        assert_eq!(
            project.get_node(node_id).map(|node| node.ui_position),
            Some([final_min.x, final_min.y])
        );
        assert!(project
            .get_clip(clip_id)
            .zip(clip_before)
            .is_some_and(|(clip, before)| {
                clip.ui_position[0] < before[0] && clip.ui_position[1] < before[1]
            }));
        assert_eq!(
            project.get_track(track_id).map(|track| track.ui_position),
            track_before,
            "the reduced port rail leaves enough track content space to contain the grown clip"
        );
        assert_eq!(
            project
                .get_composition(composition_id)
                .map(|composition| composition.ui_position),
            composition_before,
            "the composition must not drift when its content already contains the track"
        );

        let Some(clip) = project.get_clip(clip_id) else {
            return;
        };
        let clip_rect = container_rect(clip.ui_position, clip.ui_size);
        let persisted_rect = egui::Rect::from_min_size(final_min, node_size);
        assert!(rect_contains_rect(
            nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP),
            final_rect,
        ));
        assert!(rect_contains_rect(
            nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP),
            persisted_rect,
        ));
        let Some(track) = project.get_track(track_id) else {
            return;
        };
        let track_rect = container_rect(track.ui_position, track.ui_size);
        assert!(rect_contains_rect(
            nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP),
            clip_rect,
        ));
        let Some(composition) = project.get_composition(composition_id) else {
            return;
        };
        assert!(rect_contains_rect(
            composition_content_rect(container_rect(composition.ui_position, composition.ui_size,)),
            track_rect,
        ));
        assert!(!layout_needs_reflow(&project, composition_id));
    }

    #[test]
    fn composition_root_fallback_expands_same_owner_on_left_and_top() {
        let (mut project, composition_id, _, _, node_id, _) = fixture();
        assert!(project
            .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
            .is_ok());
        let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
        assert!(plan.is_some());
        let Some(plan) = plan else {
            return;
        };
        assert!(apply_auto_layout(&mut project, composition_id, &plan));
        assert!(!container_hierarchy_needs_reflow(&project, composition_id));
        let Some(origin_node) = project.get_node(node_id) else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Composition(composition_id),
                    position: origin_node.ui_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let final_min = egui::pos2(-800.0, -260.0);
        let estimated_size = estimated_node_size(&project, node_id);
        let final_rect = egui::Rect::from_min_size(final_min, estimated_size * 0.4);
        let final_positions = HashMap::from([(node_id, [final_min.x, final_min.y])]);
        let final_rects = HashMap::from([(node_id, final_rect)]);
        let intents = node_drop_intents(
            &project,
            composition_id,
            &gesture,
            &final_rects,
            &final_positions,
            final_rect.center(),
            1.0,
        );
        assert_eq!(intents.len(), 1);
        assert!(intents.first().is_some_and(|intent| {
            intent.target.container == NodeContainer::Composition(composition_id)
                && intent.target.root_fallback
                && intent.target.kind == ReparentTargetKind::RootFallback
        }));
        let reported = intents
            .first()
            .copied()
            .and_then(|active| reported_reparent_target(None, true, active));
        assert!(reported.is_some_and(|target| {
            target.kind == ReparentTargetKind::RootFallback
                && (target.score + 1.0).abs() <= f32::EPSILON
        }));
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        let before = project
            .get_composition(composition_id)
            .map(|composition| composition.ui_position);
        assert!(reparent_nodes_from_intents(&mut project, &intents));
        assert!(project
            .get_composition(composition_id)
            .zip(before)
            .is_some_and(|(composition, before)| {
                composition.ui_position[0] < before[0]
                    && composition.ui_position[1] < before[1]
                    && rect_contains_rect(
                        composition_content_rect(container_rect(
                            composition.ui_position,
                            composition.ui_size,
                        )),
                        final_rect,
                    )
            }));
        assert_eq!(
            project.get_node(node_id).map(|node| node.ui_position),
            Some([final_min.x, final_min.y])
        );
        let root_content = project.get_composition(composition_id).map(|composition| {
            composition_content_rect(container_rect(composition.ui_position, composition.ui_size))
        });
        assert!(root_content.is_some_and(|content| {
            rect_contains_rect(
                content,
                egui::Rect::from_min_size(final_min, estimated_size),
            )
        }));
        assert!(!container_hierarchy_needs_reflow(&project, composition_id));
    }

    #[test]
    fn reparent_clears_only_old_output_binding_and_preserves_typed_wire_identity_and_history() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        let mut track_output = Node::new_merge("Track Output");
        track_output.ui_position = [1_350.0, 420.0];
        let track_output_id = track_output.id;
        project.add_node(track_output);
        assert!(project
            .attach_node_to_container(NodeContainer::Track(track_id), track_output_id)
            .is_ok());
        assert!(project
            .set_output_node(NodeContainer::Track(track_id), Some(track_output_id))
            .is_ok());
        assert!(project
            .set_output_node(NodeContainer::Clip(clip_id), Some(solid_id))
            .is_ok());
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [1_800.0, 1_000.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_200.0, 1_400.0];
        }
        let time_before = project
            .connections
            .iter()
            .find(|connection| {
                connection.from == PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT)
                    && connection.to.owner == PortOwner::Node(solid_id)
            })
            .cloned();
        let image_before = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .cloned();
        assert!(time_before.is_some() && image_before.is_some());
        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());

        let final_min = egui::pos2(1_120.0, 470.0);
        let final_rect =
            egui::Rect::from_min_size(final_min, estimated_node_size(&project, solid_id));
        if let Some(node) = project.get_node_mut(solid_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        let target = deepest_legal_reparent_target(
            &project,
            composition_id,
            final_rect,
            final_rect.center(),
        );
        assert!(
            target.is_some(),
            "Track content did not resolve as a drop target"
        );
        let Some(target) = target else {
            return;
        };
        assert_eq!(target.container, NodeContainer::Track(track_id));
        assert!(reparent_nodes_from_intents(
            &mut project,
            &[NodeDropIntent {
                node_id: solid_id,
                final_rect,
                target,
            }],
        ));
        history.push_project_state(project.clone());

        assert_eq!(
            project.find_node_container(solid_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project
                .get_clip(clip_id)
                .and_then(|clip| clip.output_node_id),
            None
        );
        assert_eq!(
            project
                .get_track(track_id)
                .and_then(|track| track.output_node_id),
            Some(track_output_id)
        );
        assert_eq!(
            image_before.as_ref().and_then(|connection| {
                project
                    .connections
                    .iter()
                    .find(|candidate| candidate.id == connection.id)
            }),
            image_before.as_ref(),
        );
        let time_after = time_before.as_ref().and_then(|connection| {
            project
                .connections
                .iter()
                .find(|candidate| candidate.id == connection.id)
        });
        assert!(time_after.is_some_and(|connection| {
            connection.id == time_before.as_ref().map_or(Uuid::nil(), |before| before.id)
                && connection.from == PortAddress::new(PortOwner::Track(track_id), TIME_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(solid_id), TIME_PORT)
        }));
        assert_eq!(project.connections.len(), initial.connections.len());
        assert!(project.validate_containment().is_empty());
        assert!(project.validate_connections().is_empty());
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    #[test]
    fn rejected_reparent_restores_all_live_positions_without_history_or_wire_mutation() {
        let (mut project, composition_id, _, clip_id, node_id, _) = fixture();
        let initial = project.clone();
        let origin_position = project.get_node(node_id).map(|node| node.ui_position);
        assert!(origin_position.is_some());
        let Some(origin_position) = origin_position else {
            return;
        };
        let gesture = NodeEditorReparentGesture {
            origins: HashMap::from([(
                node_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: origin_position,
                },
            )]),
            primary_node_id: Some(node_id),
            hovered_target: None,
            hovered_node_id: None,
            hovered_score: None,
        };
        let final_min = egui::pos2(origin_position[0] + 700.0, origin_position[1] + 120.0);
        let final_rect =
            egui::Rect::from_min_size(final_min, estimated_node_size(&project, node_id) * 0.5);
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [final_min.x, final_min.y];
        }
        let target = deepest_legal_reparent_target(
            &project,
            composition_id,
            final_rect,
            final_rect.center(),
        );
        assert!(target.is_some());
        let Some(mut target) = target else {
            return;
        };
        target.container = NodeContainer::Clip(Uuid::from_u128(0xdead_beef));
        let outcome = finish_node_reparent(
            &mut project,
            &[NodeDropIntent {
                node_id,
                final_rect,
                target,
            }],
            Some(&gesture),
        );
        assert_eq!(outcome, ReparentReleaseOutcome::Rejected);
        let moved_node_ids = HashSet::from([node_id]);
        assert_eq!(
            splice_node_for_release(outcome, &moved_node_ids, Some(node_id)),
            None
        );
        assert_eq!(
            splice_node_for_release(ReparentReleaseOutcome::NoIntent, &moved_node_ids, None,),
            None
        );
        assert_eq!(
            splice_node_for_release(
                ReparentReleaseOutcome::NoIntent,
                &moved_node_ids,
                Some(node_id),
            ),
            Some(node_id)
        );
        assert_eq!(project, initial);
        let mut history = HistoryManager::new();
        history.push_project_state(initial);
        history.push_project_state(project);
        assert_eq!(history.undo_depth(), 1);
    }

    #[test]
    fn real_egui_capture_selects_the_top_overlapping_node_for_a_multi_drag() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        assert!(project.remove_node(solid_id).is_some());
        assert!(project.remove_node(merge_id).is_some());
        if let Some(clip) = project.get_clip_mut(clip_id) {
            clip.ui_size = [1_300.0, 760.0];
        }
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [1_800.0, 1_050.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_200.0, 1_400.0];
        }
        let clip_content = project.get_clip(clip_id).map(|clip| {
            nested_content_rect(
                container_rect(clip.ui_position, clip.ui_size),
                AUTO_LAYOUT_CLIP_TOP,
            )
        });
        assert!(clip_content.is_some());
        let Some(clip_content) = clip_content else {
            return;
        };
        let mut lower = Node::new_merge("Capture Lower");
        lower.id = Uuid::from_u128(1);
        lower.ui_position = [clip_content.min.x + 40.0, clip_content.min.y + 40.0];
        let lower_id = lower.id;
        let mut upper = Node::new_merge("Capture Upper");
        upper.id = Uuid::from_u128(2);
        upper.ui_position = [clip_content.min.x + 520.0, clip_content.min.y + 40.0];
        let upper_id = upper.id;
        project.add_node(lower);
        project.add_node(upper);
        assert!(project
            .attach_node_to_container(NodeContainer::Clip(clip_id), lower_id)
            .is_ok());
        assert!(project
            .attach_node_to_container(NodeContainer::Clip(clip_id), upper_id)
            .is_ok());

        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let graph_id = egui::Id::new(("captured-overlap-drag", composition_id));
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let rendered_node_rects = Arc::new(Mutex::new(HashMap::new()));
        reset_test_rects();
        let render_frame = |project: &Project,
                            snarl: &mut Snarl<GraphItem>,
                            frame: usize,
                            events: Vec<egui::Event>,
                            modifiers: egui::Modifiers| {
            let mut layout_edits = Vec::new();
            let mut captured = None;
            let mut persistent_snarl_id = None;
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    modifiers,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut merge_layer_reorder = None;
                        let mut viewer = ProjectNodeViewer {
                            project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            merge_layer_reorder: &mut merge_layer_reorder,
                            rendered_node_rects: Arc::clone(&rendered_node_rects),
                            rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                        };
                        snarl.show(&mut viewer, &node_editor_snarl_style(), graph_id, ui);
                        drop(viewer);
                        let snarl_id = ui.make_persistent_id(graph_id);
                        persistent_snarl_id = Some(snarl_id);
                        captured = captured_snarl_drag_node(ui.ctx(), snarl, snarl_id);
                        layout_edits = collect_layout_edits(project, snarl);
                    });
                },
            ));
            (layout_edits, captured, persistent_snarl_id)
        };

        let mut persistent_snarl_id = None;
        for frame in 0..4 {
            let (_, _, frame_snarl_id) = render_frame(
                &project,
                &mut snarl,
                frame,
                Vec::new(),
                egui::Modifiers::NONE,
            );
            persistent_snarl_id = frame_snarl_id;
        }
        let lower_rect = test_rect(&format!("node_editor.node:{lower_id}"));
        let upper_rect = test_rect(&format!("node_editor.node:{upper_id}"));
        assert!(lower_rect.is_some_and(|rect| rect.is_positive()));
        assert!(upper_rect.is_some_and(|rect| rect.is_positive()));
        assert!(test_rect(&format!("node_editor.node_header:{upper_id}"))
            .is_some_and(|rect| rect.is_positive()));
        let Some(persistent_snarl_id) = persistent_snarl_id else {
            return;
        };
        let snarl_node_ids = snarl
            .node_ids()
            .filter_map(|(snarl_id, item)| match item {
                GraphItem::Node(project_id) if [lower_id, upper_id].contains(project_id) => {
                    Some((*project_id, snarl_id))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let (Some(lower_snarl_id), Some(upper_snarl_id)) = (
            snarl_node_ids.get(&lower_id).copied(),
            snarl_node_ids.get(&upper_id).copied(),
        ) else {
            return;
        };
        // Establish the group through Snarl's real rectangle-selection path.
        // Starting in the canvas margin avoids invisible container controls;
        // selecting the other graph items too is harmless for this capture
        // test and exercises the same group-drag path.
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut frame = 4;
        let selection_start = screen.min + egui::vec2(20.0, 20.0);
        let selection_drag_start = selection_start + egui::vec2(10.0, 10.0);
        let selection_end = screen.max - egui::vec2(20.0, 20.0);
        for events in [
            vec![egui::Event::PointerMoved(selection_start)],
            vec![egui::Event::PointerButton {
                pos: selection_start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: shift,
            }],
            vec![egui::Event::PointerMoved(selection_drag_start)],
            vec![egui::Event::PointerMoved(selection_end)],
            vec![egui::Event::PointerButton {
                pos: selection_end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: shift,
            }],
        ] {
            render_frame(&project, &mut snarl, frame, events, shift);
            frame += 1;
        }
        let selected = egui_snarl::ui::get_selected_nodes(persistent_snarl_id, &context);
        assert!(
            selected.contains(&lower_snarl_id),
            "lower {lower_snarl_id:?} selection missing from {selected:?}; upper is {upper_snarl_id:?}"
        );
        assert!(
            selected.contains(&upper_snarl_id),
            "upper selection missing from {selected:?}"
        );
        // A newly visible body changes Snarl's measured Node width while its
        // open animation settles. Coordinate input must use geometry from a
        // settled frame, just like the HTTP QA client does.
        let mut previous_upper_header = None;
        let mut stable_header_frames = 0;
        let mut settled_upper_header = None;
        for _ in 0..30 {
            render_frame(
                &project,
                &mut snarl,
                frame,
                Vec::new(),
                egui::Modifiers::NONE,
            );
            frame += 1;
            let current = test_rect(&format!("node_editor.node_header:{upper_id}"));
            if current.is_some_and(|rect| rect.is_positive()) && current == previous_upper_header {
                stable_header_frames += 1;
                if stable_header_frames >= 2 {
                    settled_upper_header = current;
                    break;
                }
            } else {
                stable_header_frames = 0;
            }
            previous_upper_header = current;
        }
        let Some(upper_header) = settled_upper_header else {
            panic!("Merge header geometry did not settle before coordinate input");
        };
        assert!(upper_header.is_positive());
        let upper_header_center = upper_header.center();
        let command = egui::Modifiers {
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        };
        for events in [
            vec![egui::Event::PointerMoved(upper_header_center)],
            vec![egui::Event::PointerButton {
                pos: upper_header_center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: command,
            }],
            vec![egui::Event::PointerButton {
                pos: upper_header_center,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: command,
            }],
        ] {
            render_frame(&project, &mut snarl, frame, events, command);
            frame += 1;
        }
        let selected_after_command =
            egui_snarl::ui::get_selected_nodes(persistent_snarl_id, &context);
        assert!(selected_after_command.contains(&lower_snarl_id));
        assert!(!selected_after_command.contains(&upper_snarl_id));
        let post_show_project_ids = selected_after_command
            .iter()
            .filter_map(|snarl_id| match snarl.get_node(*snarl_id) {
                Some(GraphItem::Node(node_id)) => Some(*node_id),
                Some(GraphItem::Container(_) | GraphItem::PortAnchor { .. }) | None => None,
            })
            .collect::<Vec<_>>();
        let (typed_targets, typed_primary) = node_selection_after_snarl_click(
            &[
                SelectionTarget::Node(lower_id),
                SelectionTarget::Node(upper_id),
            ],
            Some(SelectionTarget::Node(upper_id)),
            &post_show_project_ids,
            upper_id,
            command,
        );
        assert_eq!(typed_targets, vec![SelectionTarget::Node(lower_id)]);
        assert_eq!(typed_primary, Some(SelectionTarget::Node(lower_id)));

        // Restore the group for the overlapping multi-drag assertion below.
        let upper_header = test_rect(&format!("node_editor.node_header:{upper_id}"));
        assert!(upper_header.is_some_and(|rect| rect.is_positive()));
        let Some(upper_header) = upper_header else {
            return;
        };
        let upper_header_center = upper_header.center();
        for events in [
            vec![egui::Event::PointerMoved(upper_header_center)],
            vec![egui::Event::PointerButton {
                pos: upper_header_center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: shift,
            }],
            vec![egui::Event::PointerButton {
                pos: upper_header_center,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: shift,
            }],
        ] {
            render_frame(&project, &mut snarl, frame, events, shift);
            frame += 1;
        }

        let overlap_position = egui::pos2(clip_content.min.x + 260.0, clip_content.min.y + 220.0);
        for node_id in [lower_id, upper_id] {
            if let Some(node) = project.get_node_mut(node_id) {
                node.ui_position = [overlap_position.x, overlap_position.y];
            }
        }
        for node_id in [lower_id, upper_id] {
            if let Some(snarl_id) = snarl_node_ids.get(&node_id).copied() {
                if let Some(node) = snarl.get_node_info_mut(snarl_id) {
                    node.pos = overlap_position;
                }
            }
        }
        for _ in 0..2 {
            render_frame(
                &project,
                &mut snarl,
                frame,
                Vec::new(),
                egui::Modifiers::NONE,
            );
            frame += 1;
        }
        let top_header = test_rect(&format!("node_editor.node_header:{upper_id}"));
        assert!(top_header.is_some_and(|rect| rect.is_positive()));
        let Some(top_header) = top_header else {
            return;
        };
        let start = top_header.center();
        render_frame(
            &project,
            &mut snarl,
            frame,
            vec![egui::Event::PointerMoved(start)],
            egui::Modifiers::NONE,
        );
        frame += 1;
        render_frame(
            &project,
            &mut snarl,
            frame,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
        );
        frame += 1;
        let end = start + egui::vec2(48.0, 24.0);
        let (layout_edits, captured, _) = render_frame(
            &project,
            &mut snarl,
            frame,
            vec![egui::Event::PointerMoved(end)],
            egui::Modifiers::NONE,
        );
        assert_eq!(captured, Some(upper_id));
        assert_ne!(captured, Some(lower_id));
        let moved_nodes = layout_edits
            .iter()
            .filter_map(|edit| match edit {
                LayoutEdit::MoveNode { node_id, .. }
                    if *node_id == lower_id || *node_id == upper_id =>
                {
                    Some(*node_id)
                }
                _ => None,
            })
            .collect::<HashSet<_>>();
        assert_eq!(moved_nodes, HashSet::from([lower_id, upper_id]));
        let mut state = NodeEditorState::default();
        record_node_reparent_origins(&project, &layout_edits, &mut state, true);
        if let Some(gesture) = state.node_reparent.as_mut() {
            gesture.primary_node_id =
                captured.filter(|node_id| gesture.origins.contains_key(node_id));
        }
        assert_eq!(
            state
                .node_reparent
                .as_ref()
                .and_then(|gesture| gesture.primary_node_id),
            Some(upper_id)
        );
    }

    #[test]
    fn cmd_deselect_uses_post_snarl_state_without_reselecting_clicked_node() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let current = [SelectionTarget::Node(first), SelectionTarget::Node(second)];
        let command = egui::Modifiers {
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        };

        let (targets, primary) = node_selection_after_snarl_click(
            &current,
            Some(SelectionTarget::Node(second)),
            &[first],
            second,
            command,
        );

        assert_eq!(targets, vec![SelectionTarget::Node(first)]);
        assert_eq!(primary, Some(SelectionTarget::Node(first)));
    }

    #[test]
    fn shift_node_selection_keeps_same_uuid_non_node_target() {
        let shared_id = Uuid::from_u128(1);
        let other_node_id = Uuid::from_u128(2);
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };

        let (targets, primary) = node_selection_after_snarl_click(
            &[SelectionTarget::Clip(shared_id)],
            Some(SelectionTarget::Clip(shared_id)),
            &[other_node_id, shared_id, shared_id],
            shared_id,
            shift,
        );

        assert_eq!(
            targets,
            vec![
                SelectionTarget::Clip(shared_id),
                SelectionTarget::Node(other_node_id),
                SelectionTarget::Node(shared_id),
            ]
        );
        assert_eq!(primary, Some(SelectionTarget::Node(shared_id)));
    }

    #[test]
    fn real_egui_node_header_drag_reparents_once_from_final_snarl_rect() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        if let Some(track) = project.get_track_mut(track_id) {
            track.ui_size = [1_800.0, 1_000.0];
        }
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_size = [2_200.0, 1_400.0];
        }
        assert!(project
            .set_output_node(NodeContainer::Clip(clip_id), Some(solid_id))
            .is_ok());
        let explicit_wire = project
            .connections
            .iter()
            .find(|connection| {
                connection.from.owner == PortOwner::Node(solid_id)
                    && connection.to.owner == PortOwner::Node(merge_id)
            })
            .cloned();
        assert!(explicit_wire.is_some());
        let initial = project.clone();
        let initial_position = project.get_node(solid_id).map(|node| node.ui_position);
        assert!(initial_position.is_some());
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());

        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
        let (mut snarl, containers) = build_snarl(&project, composition_id);
        let rendered_node_rects = Arc::new(Mutex::new(HashMap::new()));
        let mut state = NodeEditorState::default();
        let mut final_transform = egui::emath::TSTransform::IDENTITY;
        reset_test_rects();

        for frame in 0..5 {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut merge_layer_reorder = None;
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            merge_layer_reorder: &mut merge_layer_reorder,
                            rendered_node_rects: Arc::clone(&rendered_node_rects),
                            rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                        };
                        snarl.show(
                            &mut viewer,
                            &node_editor_snarl_style(),
                            egui::Id::new(("real-reparent-drag", composition_id)),
                            ui,
                        );
                        final_transform = to_global;
                    });
                },
            ));
        }

        let header = test_rect(&format!("node_editor.node_header:{solid_id}"));
        assert!(header.is_some_and(|rect| rect.is_positive()));
        let Some(header) = header else {
            return;
        };
        let Some(initial_position) = initial_position else {
            return;
        };
        let start = header.center();
        let desired_position = [1_120.0, 470.0];
        let graph_delta = egui::vec2(
            desired_position[0] - initial_position[0],
            desired_position[1] - initial_position[1],
        );
        let end = start + graph_delta * final_transform.scaling;
        assert!(screen.contains(end));
        let drag_start = start + (end - start).normalized() * 12.0;
        let event_frames = [
            vec![egui::Event::PointerMoved(start)],
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(drag_start)],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        let mut history_commits = 0;
        let mut release_outcome = ReparentReleaseOutcome::NoIntent;
        for (offset, events) in event_frames.into_iter().enumerate() {
            let mut frame_layout_edits = Vec::new();
            let mut frame_drop_intents = Vec::new();
            let mut frame_released = false;
            if let Ok(mut rects) = rendered_node_rects.lock() {
                rects.clear();
            }
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some((offset + 5) as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let mut edits = Vec::new();
                        let mut navigation = None;
                        let mut selection = None;
                        let mut wire_context_request = None;
                        let mut exclusions = Vec::new();
                        let mut to_global = egui::emath::TSTransform::IDENTITY;
                        let mut canvas_clip = ui.clip_rect();
                        let mut merge_layer_reorder = None;
                        let mut viewer = ProjectNodeViewer {
                            project: &project,
                            plugin_manager: None,
                            containers: &containers,
                            edits: &mut edits,
                            pending_navigation: &mut navigation,
                            pending_selection: &mut selection,
                            current_time: 0.0,
                            context_menu_exclusion_rects: &mut exclusions,
                            wire_context_request: &mut wire_context_request,
                            suppress_wire_connect: false,
                            locked_canvas_transform: None,
                            to_global: &mut to_global,
                            canvas_clip: &mut canvas_clip,
                            rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                            merge_layer_reorder: &mut merge_layer_reorder,
                            rendered_node_rects: Arc::clone(&rendered_node_rects),
                            rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                        };
                        let graph_id = egui::Id::new(("real-reparent-drag", composition_id));
                        snarl.show(&mut viewer, &node_editor_snarl_style(), graph_id, ui);
                        drop(viewer);
                        let captured_drag_node_id = captured_snarl_drag_node(
                            ui.ctx(),
                            &snarl,
                            ui.make_persistent_id(graph_id),
                        );
                        frame_layout_edits = collect_layout_edits(&project, &snarl);
                        let (primary_down, primary_released, pointer) = ui.input(|input| {
                            (
                                input.pointer.primary_down(),
                                input.pointer.primary_released(),
                                input.pointer.interact_pos(),
                            )
                        });
                        frame_released = primary_released;
                        record_node_reparent_origins(
                            &project,
                            &frame_layout_edits,
                            &mut state,
                            primary_down || primary_released,
                        );
                        let Some(pointer) = pointer else {
                            return;
                        };
                        let graph_point = to_global.inverse() * pointer;
                        let Ok(rects) = rendered_node_rects.lock() else {
                            return;
                        };
                        if let Some(gesture) = state.node_reparent.as_mut() {
                            if gesture.primary_node_id.is_none() {
                                gesture.primary_node_id = captured_drag_node_id
                                    .filter(|node_id| gesture.origins.contains_key(node_id));
                            }
                        }
                        if let Some(gesture) = state.node_reparent.as_ref() {
                            frame_drop_intents = node_drop_intents(
                                &project,
                                composition_id,
                                gesture,
                                &rects,
                                &final_node_positions(&project, gesture, &frame_layout_edits),
                                graph_point,
                                to_global.scaling,
                            );
                        }
                    });
                },
            ));
            let mut frame_changed = false;
            for edit in frame_layout_edits {
                frame_changed |= apply_layout_edit(&mut project, edit);
            }
            if frame_released {
                let reparent_gesture = state.node_reparent.take();
                release_outcome = finish_node_reparent(
                    &mut project,
                    &frame_drop_intents,
                    reparent_gesture.as_ref(),
                );
                frame_changed |= release_outcome != ReparentReleaseOutcome::NoIntent;
                state.moved_node_ids.clear();
                if frame_changed {
                    history.push_project_state(project.clone());
                    history_commits += 1;
                }
            }
        }

        assert_eq!(release_outcome, ReparentReleaseOutcome::Applied);
        assert_eq!(history_commits, 1);
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(
            project.find_node_container(solid_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project
                .get_clip(clip_id)
                .and_then(|clip| clip.output_node_id),
            None
        );
        assert_eq!(
            explicit_wire.as_ref().and_then(|wire| {
                project
                    .connections
                    .iter()
                    .find(|connection| connection.id == wire.id)
            }),
            explicit_wire.as_ref(),
        );
        assert!(project
            .get_node(solid_id)
            .is_some_and(|node| node.ui_position != initial_position));
        assert!(project.validate_containment().is_empty());
        assert!(project.validate_connections().is_empty());
        let edited = project.clone();
        assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    }

    #[test]
    fn root_scoped_creation_expands_a_collapsed_composition() {
        let (mut project, composition_id, track_id, clip_id, _, _) = fixture();
        project.get_track_mut(track_id).unwrap().ui_collapsed = true;
        let hidden_track_body = egui::pos2(500.0, 400.0);
        project
            .get_composition_mut(composition_id)
            .unwrap()
            .ui_collapsed = true;
        let node = Node::new_merge("Root Node");
        let node_id = node.id;
        create_prebuilt_node(&mut project, hidden_track_body, node, composition_id);

        let composition = project.get_composition(composition_id).unwrap();
        assert!(!composition.ui_collapsed);
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Composition(composition_id))
        );
        assert!(project.validate_containment().is_empty());
        let (snarl, _) = build_snarl(&project, composition_id);
        assert!(snarl.nodes().any(|item| *item == GraphItem::Node(node_id)));

        // The original Clip remains owned by the collapsed Track; creation did
        // not mutate hierarchy outside the new Node's single owner.
        assert_eq!(project.find_track_for_clip(clip_id), Some(track_id));
    }

    #[test]
    fn locally_created_node_preserves_every_existing_position_and_stays_visible() {
        let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
        project.get_clip_mut(clip_id).unwrap().ui_collapsed = true;

        let composition_before = project.get_composition(composition_id).unwrap().clone();
        let track_before = project.get_track(track_id).unwrap().clone();
        let clip_before = project.get_clip(clip_id).unwrap().clone();
        let solid_before = project.get_node(solid_id).unwrap().clone();
        let merge_before = project.get_node(merge_id).unwrap().clone();
        let connections_before = project.connections.clone();
        let desired = egui::pos2(500.0, 400.0);
        let node = generator_node(
            "Locally Placed Text",
            GeneratorNodeRequest::Text {
                text: "Locally Placed Text".to_string(),
                font: "Arial".to_string(),
            },
        );
        let node_id = node.id;

        create_prebuilt_node(&mut project, desired, node, composition_id);

        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Track(track_id))
        );
        assert_eq!(
            project.get_composition(composition_id).unwrap().ui_position,
            composition_before.ui_position
        );
        assert_eq!(
            project.get_track(track_id).unwrap().ui_position,
            track_before.ui_position
        );
        assert_eq!(
            project.get_clip(clip_id).unwrap().ui_position,
            clip_before.ui_position
        );
        assert_eq!(
            project.get_node(solid_id).unwrap().ui_position,
            solid_before.ui_position
        );
        assert_eq!(
            project.get_node(merge_id).unwrap().ui_position,
            merge_before.ui_position
        );
        assert_eq!(project.connections, connections_before);

        let (snarl, _) = build_snarl(&project, composition_id);
        assert!(snarl.nodes().any(|item| *item == GraphItem::Node(node_id)));
    }

    #[test]
    fn rendered_ports_stay_vertical_on_their_left_and_right_sides_with_bounded_width() {
        let (project, composition_id, _, _, node_id, _) = fixture();
        let context = egui::Context::default();
        reset_test_rects();

        for frame in 0..3 {
            let raw_input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1800.0, 1200.0),
                )),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            };
            drop(context.run(raw_input, |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let (mut snarl, containers) = build_snarl(&project, composition_id);
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut selection = None;
                    let mut wire_context_request = None;
                    let mut context_menu_exclusion_rects = Vec::new();
                    let mut to_global = egui::emath::TSTransform::default();
                    let mut canvas_clip = ui.clip_rect();
                    let mut merge_layer_reorder = None;
                    let mut viewer = ProjectNodeViewer {
                        project: &project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut edits,
                        pending_navigation: &mut navigation,
                        pending_selection: &mut selection,
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut context_menu_exclusion_rects,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: false,
                        locked_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                        merge_layer_reorder: &mut merge_layer_reorder,
                        rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                    };
                    let style = SnarlStyle {
                        collapsible: Some(false),
                        max_scale: Some(1.0),
                        ..Default::default()
                    };
                    snarl.show(
                        &mut viewer,
                        &style,
                        egui::Id::new(("node-layout-test", composition_id)),
                        ui,
                    );
                });
            }));
        }

        let node = test_rect(&format!("node_editor.node:{node_id}")).expect("rendered node rect");
        let time_input = test_rect(&format!(
            "node_editor.port.node:{node_id}.input:{TIME_PORT}"
        ))
        .expect("time input pin rect");
        let image_output = test_rect(&format!(
            "node_editor.port.node:{node_id}.output:{IMAGE_OUTPUT_PORT}"
        ))
        .expect("image output pin rect");

        assert!(time_input.center().x < node.center().x);
        assert!(image_output.center().x > node.center().x);
        assert!(node.width() <= 500.0, "{node:?}");
    }

    #[test]
    fn qa_geometry_is_clipped_to_the_actual_node_editor_canvas() {
        let canvas = egui::Rect::from_min_max(egui::pos2(100.0, 200.0), egui::pos2(500.0, 400.0));
        let partial = egui::Rect::from_min_max(egui::pos2(80.0, 250.0), egui::pos2(180.0, 450.0));
        assert_eq!(
            clipped_qa_rect(partial, canvas),
            egui::Rect::from_min_max(egui::pos2(100.0, 250.0), egui::pos2(180.0, 400.0))
        );

        let outside = egui::Rect::from_min_max(egui::pos2(700.0, 20.0), egui::pos2(760.0, 80.0));
        let clipped = clipped_qa_rect(outside, canvas);
        assert!(!clipped.is_positive());
        assert_eq!(clipped.min, egui::pos2(canvas.right(), canvas.top()));
        assert_eq!(clipped.max, clipped.min);
        assert!(canvas.contains(clipped.center()));
    }
}
