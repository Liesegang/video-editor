//! Interaction handling for the node editor, split from the monolithic handle_interactions().

use egui::{self, Pos2, Rect, Vec2};
use std::collections::HashMap;
use uuid::Uuid;

use crate::drawing::bezier_distance_to_point;
use crate::state::{
    BoxSelectState, ConnectingState, ContextMenuState, DragState, EdgeContextMenuState,
    NodeContextMenuState, NodeEditorState, ResizeState,
};
use crate::theme::NodeEditorTheme;
use crate::traits::NodeEditorMutator;
use crate::types::{ConnectionView, PinDataType, are_types_compatible};
use crate::widget::{NodeInteraction, PendingActions, PinScreen};

/// Context passed to interaction handlers (avoids threading many parameters).
pub(crate) struct InteractionContext<'a> {
    pub ui: &'a egui::Ui,
    pub canvas_response: &'a egui::Response,
    pub nodes: &'a [NodeInteraction],
    pub pin_screens: &'a [PinScreen],
    pub connections: &'a [ConnectionView],
    pub pin_pos_map: &'a HashMap<(Uuid, &'a str, bool), Pos2>,
    pub mutator: &'a dyn NodeEditorMutator,
    pub theme: &'a NodeEditorTheme,
    pub zoom: f32,
    pub hit_radius: f32,
}

/// Main entry point — replaces handle_interactions on NodeEditorWidget.
pub(crate) fn handle_interactions(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
) -> PendingActions {
    let mut pending = PendingActions::default();
    let pointer_pos = ctx.ui.input(|i| i.pointer.hover_pos());
    // Use press_origin for drag start — hover_pos may have drifted from the thin
    // resize edge by the time egui fires drag_started (after the click-threshold).
    let press_origin = ctx.ui.input(|i| i.pointer.press_origin());

    handle_active_drag(state, ctx, &mut pending);
    handle_drag_stop(state, ctx, pointer_pos, &mut pending);
    handle_drag_start(state, ctx, press_origin.or(pointer_pos), &mut pending);
    handle_connecting_update(state, pointer_pos);
    handle_double_click(state, ctx, pointer_pos);
    handle_single_click(state, ctx, pointer_pos, &mut pending);
    handle_right_click(state, ctx, pointer_pos);
    render_context_menu(state, ctx, &mut pending);
    render_node_context_menu(state, ctx, &mut pending);
    render_edge_context_menu(state, ctx, &mut pending);
    handle_delete_key(state, ctx, &mut pending);

    pending
}

// ---------------------------------------------------------------------------
// Hit-testing helpers
// ---------------------------------------------------------------------------

/// Find the closest pin within hit_radius of pos.
pub(crate) fn find_nearest_pin<'a>(
    pin_screens: &'a [PinScreen],
    pos: Pos2,
    hit_radius: f32,
    exclude_node: Option<Uuid>,
    require_output: Option<bool>,
) -> Option<&'a PinScreen> {
    let mut best: Option<(f32, &PinScreen)> = None;
    for ps in pin_screens {
        if let Some(excl) = exclude_node {
            if ps.node_id == excl {
                continue;
            }
        }
        if let Some(req) = require_output {
            if ps.is_output != req {
                continue;
            }
        }
        let d = pos.distance(ps.pos);
        if d < hit_radius {
            if best.is_none() || d < best.unwrap().0 {
                best = Some((d, ps));
            }
        }
    }
    best.map(|(_, ps)| ps)
}

/// Find a connection (edge) near the given point. Returns the connection ID if hit.
fn find_edge_at_point(ctx: &InteractionContext, pos: Pos2) -> Option<Uuid> {
    let edge_hit_threshold = 5.0;
    let mut best: Option<(f32, Uuid)> = None;

    for conn in ctx.connections {
        let from_pos = ctx
            .pin_pos_map
            .get(&(conn.from_node, conn.from_pin.as_str(), true))
            .or_else(|| {
                ctx.pin_pos_map
                    .get(&(conn.from_node, conn.from_pin.as_str(), false))
            });
        let to_pos = ctx
            .pin_pos_map
            .get(&(conn.to_node, conn.to_pin.as_str(), false))
            .or_else(|| {
                ctx.pin_pos_map
                    .get(&(conn.to_node, conn.to_pin.as_str(), true))
            });

        if let (Some(&from_p), Some(&to_p)) = (from_pos, to_pos) {
            let dist = bezier_distance_to_point(from_p, to_p, pos);
            if dist < edge_hit_threshold {
                if best.is_none() || dist < best.unwrap().0 {
                    best = Some((dist, conn.id));
                }
            }
        }
    }

    best.map(|(_, id)| id)
}

/// Find existing connection to a specific input pin.
fn find_existing_connection_to_input(
    connections: &[ConnectionView],
    to_node: Uuid,
    to_pin: &str,
) -> Option<Uuid> {
    connections
        .iter()
        .find(|c| c.to_node == to_node && c.to_pin == to_pin)
        .map(|c| c.id)
}

/// Get pin data type from pin_screens.
fn get_pin_data_type(
    pin_screens: &[PinScreen],
    node_id: Uuid,
    pin_name: &str,
    is_output: bool,
) -> PinDataType {
    pin_screens
        .iter()
        .find(|ps| ps.node_id == node_id && ps.name == pin_name && ps.is_output == is_output)
        .map(|ps| ps.data_type.clone())
        .unwrap_or(PinDataType::Any)
}

/// Get the container_id for a pin from pin_screens.
fn get_pin_container_id(
    pin_screens: &[PinScreen],
    node_id: Uuid,
    pin_name: &str,
    is_output: bool,
) -> Option<Uuid> {
    pin_screens
        .iter()
        .find(|ps| ps.node_id == node_id && ps.name == pin_name && ps.is_output == is_output)
        .and_then(|ps| ps.container_id)
}

// ---------------------------------------------------------------------------
// Individual interaction handlers
// ---------------------------------------------------------------------------

fn handle_active_drag(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    _pending: &mut PendingActions,
) {
    if !ctx.canvas_response.dragged_by(egui::PointerButton::Primary) {
        return;
    }
    let delta = ctx.canvas_response.drag_delta();
    let inv_zoom = 1.0 / ctx.zoom;

    if state.dragging.is_some() {
        let drag_ids: Vec<Uuid> = state
            .dragging
            .as_ref()
            .map(|d| d.node_ids.clone())
            .unwrap_or_default();
        for nid in &drag_ids {
            if let Some(pos) = state.node_positions.get_mut(nid) {
                *pos += delta * inv_zoom;
            }
        }
    } else if state.resizing.is_some() {
        if let Some(ref rs) = state.resizing {
            let nid = rs.node_id;
            let new_size = rs.start_size + delta * inv_zoom;
            let min_w = ctx.theme.node_width;
            let min_h = ctx.theme.header_height + 20.0;
            state
                .container_sizes
                .insert(nid, Vec2::new(new_size.x.max(min_w), new_size.y.max(min_h)));
        }
        if let Some(ref mut rs) = state.resizing {
            rs.start_size = state
                .container_sizes
                .get(&rs.node_id)
                .copied()
                .unwrap_or(rs.start_size);
        }
    } else if let Some(ref mut bs) = state.box_selecting {
        if let Some(pos) = ctx.ui.input(|i| i.pointer.hover_pos()) {
            bs.current = pos;
        }
    }
}

fn handle_drag_stop(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    pointer_pos: Option<Pos2>,
    pending: &mut PendingActions,
) {
    if !ctx
        .canvas_response
        .drag_stopped_by(egui::PointerButton::Primary)
    {
        return;
    }

    // Finish connection
    if let Some(connecting) = state.connecting.take() {
        if let Some(pos) = pointer_pos {
            let target = find_nearest_pin(
                ctx.pin_screens,
                pos,
                ctx.hit_radius,
                Some(connecting.from_node),
                Some(!connecting.is_output),
            );
            if let Some(target) = target {
                // Determine which is output and which is input
                let (from_node, from_pin, to_node, to_pin) = if connecting.is_output {
                    (
                        connecting.from_node,
                        connecting.from_pin.as_str(),
                        target.node_id,
                        target.name.as_str(),
                    )
                } else {
                    (
                        target.node_id,
                        target.name.as_str(),
                        connecting.from_node,
                        connecting.from_pin.as_str(),
                    )
                };

                // Type validation
                let from_type = get_pin_data_type(ctx.pin_screens, from_node, from_pin, true);
                let to_type = get_pin_data_type(ctx.pin_screens, to_node, to_pin, false);

                if are_types_compatible(&from_type, &to_type) {
                    // Container validation: pins must be in the same container scope
                    let from_container =
                        get_pin_container_id(ctx.pin_screens, from_node, from_pin, true);
                    let to_container =
                        get_pin_container_id(ctx.pin_screens, to_node, to_pin, false);
                    if from_container != to_container {
                        // Pins are in different container scopes — reject
                        return;
                    }

                    // Edge overwrite: remove existing connection to the input pin
                    if let Some(existing_id) =
                        find_existing_connection_to_input(ctx.connections, to_node, to_pin)
                    {
                        pending.connections_to_remove.push(existing_id);
                    }

                    pending.connections_to_add.push((
                        from_node,
                        from_pin.to_string(),
                        to_node,
                        to_pin.to_string(),
                    ));
                }
            }
        }
    }

    // Finish box selection
    if let Some(bs) = state.box_selecting.take() {
        let sel_rect = Rect::from_two_pos(bs.start, bs.current);
        if !ctx.ui.input(|i| i.modifiers.shift) {
            state.selected_nodes.clear();
        }
        for node in ctx.nodes {
            if sel_rect.intersects(node.rect) {
                state.selected_nodes.insert(node.id);
            }
        }
    }

    // Finish node drag — check for reparent
    if let Some(ref drag) = state.dragging {
        if let Some(pos) = pointer_pos {
            let drag_ids = drag.node_ids.clone();
            let mut target_container: Option<Uuid> = None;
            for node in ctx.nodes.iter().rev() {
                if node.is_container && node.rect.contains(pos) && !drag_ids.contains(&node.id) {
                    target_container = Some(node.id);
                    break;
                }
            }
            if let Some(target) = target_container {
                for &nid in &drag_ids {
                    let current_parent = ctx
                        .nodes
                        .iter()
                        .find(|n| {
                            n.is_container && n.id != nid && n.rect.contains(pos) && n.id != target
                        })
                        .map(|n| n.id);
                    let from = current_parent.or(state.current_container).unwrap_or(target);
                    if from != target {
                        pending.nodes_to_move.push((nid, from, target));
                    }
                }
            }
        }
    }

    state.dragging = None;
    state.resizing = None;
}

fn handle_drag_start(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    pointer_pos: Option<Pos2>,
    pending: &mut PendingActions,
) {
    if !ctx
        .canvas_response
        .drag_started_by(egui::PointerButton::Primary)
    {
        return;
    }
    let Some(pos) = pointer_pos else { return };

    // 1. Check pin hit
    if let Some(ps) = find_nearest_pin(ctx.pin_screens, pos, ctx.hit_radius, None, None) {
        state.connecting = Some(ConnectingState {
            from_node: ps.node_id,
            from_pin: ps.name.clone(),
            is_output: ps.is_output,
            mouse_pos: pos,
        });
        return;
    }

    // 2. Check resize handle (container edges + corner)
    let edge_width = 6.0 * ctx.zoom;
    let handle_size = 16.0 * ctx.zoom;
    let header_h = ctx.theme.header_height * ctx.zoom;
    for node in ctx.nodes.iter().rev() {
        if node.is_container {
            // Corner handle (bottom-right)
            let handle_rect = Rect::from_min_size(
                Pos2::new(node.rect.max.x - handle_size, node.rect.max.y - handle_size),
                Vec2::new(handle_size, handle_size),
            );
            // Right edge (below header)
            let right_edge = Rect::from_min_max(
                Pos2::new(node.rect.max.x - edge_width, node.rect.min.y + header_h),
                node.rect.max,
            );
            // Bottom edge
            let bottom_edge = Rect::from_min_max(
                Pos2::new(node.rect.min.x, node.rect.max.y - edge_width),
                node.rect.max,
            );
            if handle_rect.contains(pos) || right_edge.contains(pos) || bottom_edge.contains(pos) {
                let current_size = state
                    .container_sizes
                    .get(&node.id)
                    .copied()
                    .unwrap_or(node.rect.size() / ctx.zoom);
                state.resizing = Some(ResizeState {
                    node_id: node.id,
                    start_size: current_size,
                    mouse_start: pos,
                });
                return;
            }
        }
    }

    // 3. Check node header for dragging
    for node in ctx.nodes.iter().rev() {
        let header_rect =
            Rect::from_min_size(node.rect.min, Vec2::new(node.rect.width(), header_h));
        if header_rect.contains(pos) {
            if !ctx.ui.input(|i| i.modifiers.shift) {
                state.selected_nodes.clear();
            }
            state.selected_nodes.insert(node.id);
            pending.selected_node = Some(node.id);

            let drag_ids: Vec<Uuid> = state.selected_nodes.iter().copied().collect();
            let start_positions: Vec<Pos2> = drag_ids
                .iter()
                .map(|id| state.node_positions.get(id).copied().unwrap_or(Pos2::ZERO))
                .collect();
            state.dragging = Some(DragState {
                node_ids: drag_ids,
                start_positions,
                mouse_start: pos,
            });
            return;
        }
    }

    // 4. Empty space → box selection
    state.box_selecting = Some(BoxSelectState {
        start: pos,
        current: pos,
    });
}

fn handle_connecting_update(state: &mut NodeEditorState, pointer_pos: Option<Pos2>) {
    if let Some(ref mut connecting) = state.connecting {
        if let Some(pos) = pointer_pos {
            connecting.mouse_pos = pos;
        }
    }
}

fn handle_double_click(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    pointer_pos: Option<Pos2>,
) {
    if !ctx.canvas_response.double_clicked() {
        return;
    }
    let Some(pos) = pointer_pos else { return };

    // Find container whose header was double-clicked (not just anywhere in the body)
    let header_h = ctx.theme.header_height * ctx.zoom;
    for node in ctx.nodes.iter().rev() {
        if node.is_container {
            let header_rect =
                Rect::from_min_size(node.rect.min, Vec2::new(node.rect.width(), header_h));
            if header_rect.contains(pos) {
                if !state.expanded_containers.remove(&node.id) {
                    state.expanded_containers.insert(node.id);
                }
                return;
            }
        }
    }
}

fn handle_single_click(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    pointer_pos: Option<Pos2>,
    pending: &mut PendingActions,
) {
    if !ctx.canvas_response.clicked() {
        return;
    }
    let Some(pos) = pointer_pos else { return };

    // Check node hit first
    let mut hit_node = false;
    for node in ctx.nodes.iter().rev() {
        if node.rect.contains(pos) {
            if !ctx.ui.input(|i| i.modifiers.shift) {
                state.selected_nodes.clear();
            }
            state.selected_nodes.insert(node.id);
            pending.selected_node = Some(node.id);
            hit_node = true;
            break;
        }
    }

    if !hit_node {
        // Check edge hit
        if let Some(edge_id) = find_edge_at_point(ctx, pos) {
            if !ctx.ui.input(|i| i.modifiers.shift) {
                state.selected_connections.clear();
            }
            state.selected_connections.insert(edge_id);
            state.selected_nodes.clear();
        } else {
            // Empty space click — clear all selections
            state.selected_nodes.clear();
            state.selected_connections.clear();
        }
    } else {
        state.selected_connections.clear();
    }

    state.context_menu = None;
    state.node_context_menu = None;
    state.edge_context_menu = None;
}

fn handle_right_click(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    pointer_pos: Option<Pos2>,
) {
    if !ctx.canvas_response.secondary_clicked() {
        return;
    }
    let Some(pos) = pointer_pos else { return };

    // Check node hit
    for node in ctx.nodes.iter().rev() {
        if node.rect.contains(pos) {
            state.node_context_menu = Some(NodeContextMenuState {
                screen_pos: pos,
                node_id: node.id,
            });
            state.context_menu = None;
            state.edge_context_menu = None;
            return;
        }
    }

    // Check edge hit
    if let Some(edge_id) = find_edge_at_point(ctx, pos) {
        state.edge_context_menu = Some(EdgeContextMenuState {
            screen_pos: pos,
            connection_id: edge_id,
        });
        state.context_menu = None;
        state.node_context_menu = None;
        return;
    }

    // Empty space — show add-node menu
    if let Some(cid) = state.current_container {
        state.context_menu = Some(ContextMenuState {
            screen_pos: pos,
            container_id: cid,
        });
        state.context_search.clear();
    }
    state.node_context_menu = None;
    state.edge_context_menu = None;
}

fn render_context_menu(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    pending: &mut PendingActions,
) {
    let Some(menu) = state.context_menu.clone() else {
        return;
    };

    let mut close = false;
    let popup_id = ctx.ui.make_persistent_id("node_editor_context_menu");
    egui::Area::new(popup_id)
        .order(egui::Order::Foreground)
        .fixed_pos(menu.screen_pos)
        .show(ctx.ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_max_width(250.0);
                ui.label("Add Node");
                ui.separator();
                let response = ui.text_edit_singleline(&mut state.context_search);
                if !response.has_focus() {
                    response.request_focus();
                }
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
                ui.separator();

                let node_types = ctx.mutator.get_available_node_types();
                let search_lower = state.context_search.to_lowercase();
                let has_search = !search_lower.is_empty();

                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        if has_search {
                            for nt in &node_types {
                                if nt.display_name.to_lowercase().contains(&search_lower)
                                    || nt.type_id.to_lowercase().contains(&search_lower)
                                {
                                    let label = format!("{} ({})", nt.display_name, nt.category);
                                    if ui.button(&label).clicked() {
                                        pending
                                            .nodes_to_add
                                            .push((menu.container_id, nt.type_id.clone()));
                                        close = true;
                                    }
                                }
                            }
                        } else {
                            let mut categories: Vec<String> = node_types
                                .iter()
                                .map(|nt| nt.category.clone())
                                .collect::<std::collections::HashSet<_>>()
                                .into_iter()
                                .collect();
                            categories.sort();
                            for category in &categories {
                                ui.menu_button(category, |ui| {
                                    for nt in &node_types {
                                        if &nt.category == category {
                                            if ui.button(&nt.display_name).clicked() {
                                                pending
                                                    .nodes_to_add
                                                    .push((menu.container_id, nt.type_id.clone()));
                                                close = true;
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    });
            });
        });
    if close || ctx.ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.context_menu = None;
    }
}

fn render_node_context_menu(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    pending: &mut PendingActions,
) {
    let Some(menu) = state.node_context_menu.clone() else {
        return;
    };

    let mut close = false;
    let sel_count = state.selected_nodes.len();
    let popup_id = ctx.ui.make_persistent_id("node_context_menu");
    egui::Area::new(popup_id)
        .order(egui::Order::Foreground)
        .fixed_pos(menu.screen_pos)
        .show(ctx.ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_max_width(180.0);
                if sel_count > 1 {
                    let label = format!("Delete Selected ({})", sel_count);
                    if ui.button(&label).clicked() {
                        let to_remove: Vec<Uuid> = state.selected_nodes.iter().copied().collect();
                        pending.nodes_to_remove.extend(to_remove);
                        state.selected_nodes.clear();
                        close = true;
                    }
                } else {
                    if ui.button("Delete Node").clicked() {
                        pending.nodes_to_remove.push(menu.node_id);
                        state.selected_nodes.remove(&menu.node_id);
                        close = true;
                    }
                }
            });
        });
    if close || ctx.ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.node_context_menu = None;
    }
}

fn render_edge_context_menu(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    pending: &mut PendingActions,
) {
    let Some(menu) = state.edge_context_menu.clone() else {
        return;
    };

    let mut close = false;
    let popup_id = ctx.ui.make_persistent_id("edge_context_menu");
    egui::Area::new(popup_id)
        .order(egui::Order::Foreground)
        .fixed_pos(menu.screen_pos)
        .show(ctx.ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_max_width(180.0);
                if ui.button("Delete Connection").clicked() {
                    pending.connections_to_remove.push(menu.connection_id);
                    state.selected_connections.remove(&menu.connection_id);
                    close = true;
                }
            });
        });
    if close || ctx.ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.edge_context_menu = None;
    }
}

fn handle_delete_key(
    state: &mut NodeEditorState,
    ctx: &InteractionContext,
    pending: &mut PendingActions,
) {
    if ctx
        .ui
        .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        let to_remove: Vec<Uuid> = state.selected_nodes.iter().copied().collect();
        pending.nodes_to_remove.extend(to_remove);
        state.selected_nodes.clear();
        let conns: Vec<Uuid> = state.selected_connections.iter().copied().collect();
        pending.connections_to_remove.extend(conns);
        state.selected_connections.clear();
    }
}

/// Clip a node interaction rect to a visible area.
/// Returns None if the node is fully outside the clip bounds.
pub(crate) fn clip_interaction_rect(node_rect: Rect, clip_rect: Rect) -> Option<Rect> {
    let clipped = node_rect.intersect(clip_rect);
    if clipped.width() <= 0.0 || clipped.height() <= 0.0 {
        None
    } else {
        Some(clipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_interaction_rect_fully_visible() {
        let node_rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(100.0, 50.0));
        let clip_rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(500.0, 500.0));
        let result = clip_interaction_rect(node_rect, clip_rect);
        assert_eq!(result, Some(node_rect));
    }

    #[test]
    fn test_clip_interaction_rect_partially_visible() {
        let node_rect = Rect::from_min_size(Pos2::new(-20.0, 10.0), Vec2::new(100.0, 50.0));
        let clip_rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(500.0, 500.0));
        let result = clip_interaction_rect(node_rect, clip_rect);
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.min.x >= 0.0);
    }

    #[test]
    fn test_clip_interaction_rect_fully_hidden() {
        let node_rect = Rect::from_min_size(Pos2::new(-200.0, -200.0), Vec2::new(100.0, 50.0));
        let clip_rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(500.0, 500.0));
        assert!(clip_interaction_rect(node_rect, clip_rect).is_none());
    }

    #[test]
    fn test_find_existing_connection_to_input() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let conn_id = Uuid::new_v4();
        let connections = vec![ConnectionView {
            id: conn_id,
            from_node: id1,
            from_pin: "image_out".to_string(),
            to_node: id2,
            to_pin: "image_in".to_string(),
        }];

        assert_eq!(
            find_existing_connection_to_input(&connections, id2, "image_in"),
            Some(conn_id)
        );
        assert_eq!(
            find_existing_connection_to_input(&connections, id2, "other_in"),
            None
        );
        assert_eq!(
            find_existing_connection_to_input(&connections, id1, "image_in"),
            None
        );
    }

    #[test]
    fn test_get_pin_data_type_found() {
        let node_id = Uuid::new_v4();
        let pin_screens = vec![
            PinScreen {
                pos: Pos2::ZERO,
                node_id,
                name: "image_out".to_string(),
                is_output: true,
                data_type: PinDataType::Image,
                container_id: None,
            },
            PinScreen {
                pos: Pos2::ZERO,
                node_id,
                name: "value_in".to_string(),
                is_output: false,
                data_type: PinDataType::Scalar,
                container_id: None,
            },
        ];

        assert_eq!(
            get_pin_data_type(&pin_screens, node_id, "image_out", true),
            PinDataType::Image
        );
        assert_eq!(
            get_pin_data_type(&pin_screens, node_id, "value_in", false),
            PinDataType::Scalar
        );
    }

    #[test]
    fn test_get_pin_data_type_not_found_returns_any() {
        let pin_screens: Vec<PinScreen> = vec![];
        assert_eq!(
            get_pin_data_type(&pin_screens, Uuid::new_v4(), "missing", true),
            PinDataType::Any
        );
    }
}
