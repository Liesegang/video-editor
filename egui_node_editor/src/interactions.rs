//! Interaction handling for the node editor, split from the monolithic handle_interactions().

use egui::{self, Pos2, Rect, Vec2};
use uuid::Uuid;

use crate::state::{
    BoxSelectState, ConnectingState, ContextMenuState, DragState, NodeContextMenuState,
    NodeEditorState, ResizeState,
};
use crate::theme::NodeEditorTheme;
use crate::traits::NodeEditorMutator;
use crate::widget::{NodeInteraction, PendingActions, PinScreen};

/// Context passed to interaction handlers (avoids threading many parameters).
pub(crate) struct InteractionContext<'a> {
    pub ui: &'a egui::Ui,
    pub canvas_response: &'a egui::Response,
    pub nodes: &'a [NodeInteraction],
    pub pin_screens: &'a [PinScreen],
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

    handle_active_drag(state, ctx, &mut pending);
    handle_drag_stop(state, ctx, pointer_pos, &mut pending);
    handle_drag_start(state, ctx, pointer_pos, &mut pending);
    handle_connecting_update(state, pointer_pos);
    handle_double_click(state, ctx, pointer_pos);
    handle_single_click(state, ctx, pointer_pos, &mut pending);
    handle_right_click(state, ctx, pointer_pos);
    render_context_menu(state, ctx, &mut pending);
    render_node_context_menu(state, ctx, &mut pending);
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
                if connecting.is_output {
                    pending.connections_to_add.push((
                        connecting.from_node,
                        connecting.from_pin.clone(),
                        target.node_id,
                        target.name.clone(),
                    ));
                } else {
                    pending.connections_to_add.push((
                        target.node_id,
                        target.name.clone(),
                        connecting.from_node,
                        connecting.from_pin.clone(),
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

    // 2. Check resize handle (container bottom-right)
    let handle_size = 8.0 * ctx.zoom;
    for node in ctx.nodes.iter().rev() {
        if node.is_container {
            let handle_rect = Rect::from_min_size(
                Pos2::new(node.rect.max.x - handle_size, node.rect.max.y - handle_size),
                Vec2::new(handle_size, handle_size),
            );
            if handle_rect.contains(pos) {
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
    let header_h = ctx.theme.header_height * ctx.zoom;
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

    // Find the smallest container at this position
    let mut best: Option<(Uuid, f32)> = None;
    for node in ctx.nodes {
        if node.is_container && node.rect.contains(pos) {
            let area = node.rect.area();
            if best.is_none() || area < best.unwrap().1 {
                best = Some((node.id, area));
            }
        }
    }
    if let Some((id, _)) = best {
        if !state.expanded_containers.remove(&id) {
            state.expanded_containers.insert(id);
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

    let mut hit = false;
    for node in ctx.nodes.iter().rev() {
        if node.rect.contains(pos) {
            if !ctx.ui.input(|i| i.modifiers.shift) {
                state.selected_nodes.clear();
            }
            state.selected_nodes.insert(node.id);
            pending.selected_node = Some(node.id);
            hit = true;
            break;
        }
    }
    if !hit {
        state.selected_nodes.clear();
        state.selected_connections.clear();
    }
    state.context_menu = None;
    state.node_context_menu = None;
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

    for node in ctx.nodes.iter().rev() {
        if node.rect.contains(pos) {
            state.node_context_menu = Some(NodeContextMenuState {
                screen_pos: pos,
                node_id: node.id,
            });
            state.context_menu = None;
            return;
        }
    }

    if let Some(cid) = state.current_container {
        state.context_menu = Some(ContextMenuState {
            screen_pos: pos,
            container_id: cid,
        });
        state.context_search.clear();
    }
    state.node_context_menu = None;
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
