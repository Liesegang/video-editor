//! Main node editor widget.

use egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, Vec2};
use std::collections::HashMap;
use uuid::Uuid;

use crate::drawing::{draw_bezier_connection, draw_grid};
use crate::state::{
    BoxSelectState, ConnectingState, ContextMenuState, DragState, NodeContextMenuState,
    NodeEditorState, ResizeState,
};
use crate::theme::NodeEditorTheme;
use crate::traits::{NodeEditorDataSource, NodeEditorMutator};
use crate::types::{NodeDisplay, PinInfo};

// ---------------------------------------------------------------------------
// PendingActions
// ---------------------------------------------------------------------------

/// Pending mutations collected during the render phase, applied after.
#[derive(Default)]
pub struct PendingActions {
    pub nodes_to_remove: Vec<Uuid>,
    pub connections_to_remove: Vec<Uuid>,
    pub connections_to_add: Vec<(Uuid, String, Uuid, String)>,
    pub nodes_to_add: Vec<(Uuid, String)>,
    /// (node_id, from_container, to_container)
    pub nodes_to_move: Vec<(Uuid, Uuid, Uuid)>,
    /// Optional: node selected in editor (for inspector sync).
    pub selected_node: Option<Uuid>,
}

impl PendingActions {
    pub fn apply(self, mutator: &mut dyn NodeEditorMutator) {
        for node_id in self.nodes_to_remove {
            let _ = mutator.remove_node(node_id);
        }
        for conn_id in self.connections_to_remove {
            let _ = mutator.remove_connection(conn_id);
        }
        for (from_node, from_pin, to_node, to_pin) in self.connections_to_add {
            let _ = mutator.add_connection(from_node, &from_pin, to_node, &to_pin);
        }
        for (container_id, type_id) in self.nodes_to_add {
            let _ = mutator.add_node(container_id, &type_id);
        }
        for (node_id, from_container, to_container) in self.nodes_to_move {
            let _ = mutator.move_node(node_id, from_container, to_container);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes_to_remove.is_empty()
            && self.connections_to_remove.is_empty()
            && self.connections_to_add.is_empty()
            && self.nodes_to_add.is_empty()
            && self.nodes_to_move.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Pin position tracking
// ---------------------------------------------------------------------------

/// Screen position of a rendered pin.
struct PinScreen {
    pos: Pos2,
    node_id: Uuid,
    name: String,
    is_output: bool,
}

// ---------------------------------------------------------------------------
// NodeEditorWidget
// ---------------------------------------------------------------------------

pub struct NodeEditorWidget<'a> {
    state: &'a mut NodeEditorState,
    theme: &'a NodeEditorTheme,
}

impl<'a> NodeEditorWidget<'a> {
    pub fn new(state: &'a mut NodeEditorState, theme: &'a NodeEditorTheme) -> Self {
        Self { state, theme }
    }

    /// Show the node editor. Returns pending actions to apply via mutator.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &dyn NodeEditorDataSource,
        mutator: &dyn NodeEditorMutator,
    ) -> PendingActions {
        // Ensure zoom is valid
        if self.state.zoom <= 0.0 {
            self.state.zoom = 1.0;
        }

        let container_id = match self.state.current_container {
            Some(id) => id,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("No container selected");
                });
                return PendingActions::default();
            }
        };

        let child_ids = source.get_container_children(container_id);
        if child_ids.is_empty() && source.get_container_name(container_id).is_none() {
            ui.label("Container not found");
            return PendingActions::default();
        }

        // Ensure all top-level children have positions
        let mut offset_y = 0.0_f32;
        for &child_id in &child_ids {
            self.state
                .node_positions
                .entry(child_id)
                .or_insert_with(|| {
                    let pos = Pos2::new(50.0, 50.0 + offset_y);
                    offset_y += 200.0;
                    pos
                });
            offset_y += 200.0;
        }

        // Breadcrumb bar
        ui.horizontal(|ui| {
            ui.label("Container:");
            if let Some(name) = source.get_container_name(container_id) {
                ui.strong(&name);
            }
            if let Some(parent) = source.find_parent_container(container_id) {
                if ui.small_button("\u{2191} Up").clicked() {
                    self.state.current_container = Some(parent);
                }
            }
        });
        ui.separator();

        // Main canvas
        let available = ui.available_rect_before_wrap();
        let (canvas_response, painter) =
            ui.allocate_painter(available.size(), egui::Sense::click_and_drag());
        let canvas_rect = canvas_response.rect;
        // Zoom via scroll wheel
        if let Some(hover) = ui.input(|i| i.pointer.hover_pos()) {
            if canvas_rect.contains(hover) {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    let old_zoom = self.state.zoom;
                    let new_zoom = (old_zoom + scroll * 0.002).clamp(0.2, 3.0);
                    // Adjust pan so the point under the cursor stays stable
                    let graph_pos = (hover - canvas_rect.min - self.state.pan) / old_zoom;
                    self.state.pan = hover - canvas_rect.min - graph_pos * new_zoom;
                    self.state.zoom = new_zoom;
                }
            }
        }
        let zoom = self.state.zoom; // re-read after potential update

        // Panning
        if canvas_response.dragged_by(egui::PointerButton::Middle) {
            self.state.pan += canvas_response.drag_delta();
        }

        // Background
        painter.rect_filled(canvas_rect, 0.0, self.theme.background_color);
        draw_grid(
            &painter,
            canvas_rect,
            self.state.pan,
            self.theme.grid_color,
            self.theme.grid_spacing * zoom,
        );

        // ---- Phase 1: Draw nodes & collect pin positions ----
        let mut pin_screens: Vec<PinScreen> = Vec::new();
        let mut node_interactions: Vec<NodeInteraction> = Vec::new();

        for &child_id in &child_ids {
            let Some(display) = source.get_node_display(child_id) else {
                continue;
            };
            let is_active = source.is_node_active(child_id);
            let is_expanded = self.state.expanded_containers.contains(&child_id);

            self.draw_node(
                source,
                &painter,
                canvas_rect.min,
                zoom,
                child_id,
                &display,
                is_active,
                is_expanded,
                &mut pin_screens,
                &mut node_interactions,
            );
        }

        // Build lookup: (node_id, pin_name, is_output) -> screen pos
        let pin_pos_map: HashMap<(Uuid, &str, bool), Pos2> = pin_screens
            .iter()
            .map(|p| ((p.node_id, p.name.as_str(), p.is_output), p.pos))
            .collect();

        // ---- Phase 2: Draw connections ON TOP of nodes ----
        let connections = source.get_connections();
        for conn in &connections {
            // from_pin is output side; to_pin is input side
            // Try exact match first, then fallback to opposite direction (for port pins)
            let from_pos = pin_pos_map
                .get(&(conn.from_node, conn.from_pin.as_str(), true))
                .or_else(|| pin_pos_map.get(&(conn.from_node, conn.from_pin.as_str(), false)));
            let to_pos = pin_pos_map
                .get(&(conn.to_node, conn.to_pin.as_str(), false))
                .or_else(|| pin_pos_map.get(&(conn.to_node, conn.to_pin.as_str(), true)));

            if let (Some(&from_p), Some(&to_p)) = (from_pos, to_pos) {
                let color = if self.state.selected_connections.contains(&conn.id) {
                    self.theme.connection_selected_color
                } else if let Some(type_id) = source.get_node_type_id(conn.from_node) {
                    (self.theme.pin_color)(&type_id)
                } else {
                    self.theme.connection_color
                };
                draw_bezier_connection(&painter, from_p, to_p, color);
            }
        }

        // Draw connecting line (drag in progress)
        if let Some(ref connecting) = self.state.connecting {
            let start = pin_pos_map
                .get(&(
                    connecting.from_node,
                    connecting.from_pin.as_str(),
                    connecting.is_output,
                ))
                .or_else(|| {
                    pin_pos_map.get(&(
                        connecting.from_node,
                        connecting.from_pin.as_str(),
                        !connecting.is_output,
                    ))
                });
            if let Some(&start_pos) = start {
                draw_bezier_connection(
                    &painter,
                    start_pos,
                    connecting.mouse_pos,
                    Color32::from_rgb(200, 200, 200),
                );
            }
        }

        // Draw box selection rectangle
        if let Some(ref bs) = self.state.box_selecting {
            let sel_rect = Rect::from_two_pos(bs.start, bs.current);
            painter.rect_filled(
                sel_rect,
                0.0,
                Color32::from_rgba_unmultiplied(100, 150, 255, 30),
            );
            painter.rect_stroke(
                sel_rect,
                0.0,
                Stroke::new(1.0, Color32::from_rgb(100, 150, 255)),
                StrokeKind::Outside,
            );
        }

        // ---- Phase 3: Handle interactions ----
        self.handle_interactions(
            ui,
            &canvas_response,
            &node_interactions,
            &pin_screens,
            mutator,
        )
    }

    // -----------------------------------------------------------------------
    // Node drawing
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn draw_node(
        &mut self,
        source: &dyn NodeEditorDataSource,
        painter: &egui::Painter,
        canvas_min: Pos2,
        zoom: f32,
        node_id: Uuid,
        display: &NodeDisplay,
        is_active: bool,
        is_expanded: bool,
        pin_screens: &mut Vec<PinScreen>,
        interactions: &mut Vec<NodeInteraction>,
    ) {
        let (node_type_id, display_name, pins) = match display {
            NodeDisplay::Graph {
                type_id,
                display_name,
                pins,
            } => (type_id.clone(), display_name.clone(), pins.clone()),
            NodeDisplay::Container { name, pins, .. } => (
                "track".to_string(),
                format!("Track: {}", name),
                pins.clone(),
            ),
            NodeDisplay::Leaf { kind_label, pins } => {
                let type_id = format!("clip.{}", kind_label);
                (type_id, format!("Clip ({})", kind_label), pins.clone())
            }
        };

        let pos = self
            .state
            .node_positions
            .get(&node_id)
            .copied()
            .unwrap_or(Pos2::ZERO);
        let screen_pos = canvas_min + (pos.to_vec2() * zoom) + self.state.pan;

        let input_pins: Vec<&PinInfo> = pins.iter().filter(|p| !p.is_output).collect();
        let output_pins: Vec<&PinInfo> = pins.iter().filter(|p| p.is_output).collect();
        let pin_count = input_pins.len().max(output_pins.len());

        // Initialize child positions early (needed for expanded height calculation)
        if let NodeDisplay::Container { child_ids, .. } = display {
            let mut next_y = 10.0f32;
            for &child_id in child_ids {
                if !self.state.node_positions.contains_key(&child_id) {
                    self.state
                        .node_positions
                        .insert(child_id, Pos2::new(10.0, next_y));
                    next_y += 100.0;
                }
            }
        }

        let header_h = self.theme.header_height * zoom;
        let pin_row_h = self.theme.pin_row_height * zoom;
        let pin_r = self.theme.pin_radius * zoom;
        let pin_margin = self.theme.pin_margin * zoom;
        let rounding = self.theme.node_rounding * zoom;

        // Calculate expanded children height
        let auto_expanded_h = if is_expanded {
            if let NodeDisplay::Container { child_ids, .. } = display {
                self.calc_expanded_h(source, child_ids, zoom, 0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        let auto_w = if is_expanded {
            self.theme.node_width * zoom * 2.0
        } else {
            self.theme.node_width * zoom
        };
        let auto_h = (header_h + pin_count as f32 * pin_row_h + 8.0 * zoom + auto_expanded_h)
            .max(header_h + 8.0 * zoom);

        // Use custom size if available (for containers), else auto
        let (node_w, node_h) = if let Some(custom) = self.state.container_sizes.get(&node_id) {
            (
                (custom.x * zoom).max(auto_w.min(self.theme.node_width * zoom)),
                (custom.y * zoom).max(header_h + 8.0 * zoom),
            )
        } else {
            (auto_w, auto_h)
        };

        let node_rect = Rect::from_min_size(screen_pos, Vec2::new(node_w, node_h));

        // Push interaction BEFORE drawing children so .rev() finds children first
        interactions.push(NodeInteraction {
            id: node_id,
            rect: node_rect,
            is_container: matches!(display, NodeDisplay::Container { .. }),
        });

        let is_selected = self.state.selected_nodes.contains(&node_id);
        let dim = if is_active { 1.0 } else { 0.4 };

        // Body
        let body_color = dim_color(
            if is_selected {
                self.theme.node_body_selected_color
            } else {
                self.theme.node_body_color
            },
            dim,
        );
        painter.rect_filled(node_rect, rounding, body_color);

        if is_selected {
            painter.rect_stroke(
                node_rect,
                rounding,
                Stroke::new(2.0 * zoom, self.theme.selection_color),
                StrokeKind::Outside,
            );
        }

        // Header
        let header_rect = Rect::from_min_size(screen_pos, Vec2::new(node_w, header_h));
        painter.rect_filled(
            header_rect,
            egui::CornerRadius {
                nw: rounding as u8,
                ne: rounding as u8,
                sw: 0,
                se: 0,
            },
            dim_color((self.theme.header_color)(&node_type_id), dim),
        );

        let header_text = if matches!(display, NodeDisplay::Container { .. }) {
            format!(
                "{} {}",
                if is_expanded { "\u{25BC}" } else { "\u{25B6}" },
                display_name
            )
        } else {
            display_name.clone()
        };
        painter.text(
            header_rect.center(),
            egui::Align2::CENTER_CENTER,
            &header_text,
            egui::FontId::proportional(12.0 * zoom),
            dim_color(Color32::WHITE, dim),
        );

        // Pins
        let pin_start_y = screen_pos.y + header_h + 4.0 * zoom;
        let pin_color = dim_color((self.theme.pin_color)(&node_type_id), dim);
        let label_color = dim_color(self.theme.pin_label_color, dim);

        for (i, pin) in input_pins.iter().enumerate() {
            let cy = pin_start_y + i as f32 * pin_row_h + pin_row_h / 2.0;
            let cx = screen_pos.x + pin_margin;
            let p = Pos2::new(cx, cy);
            painter.circle_filled(p, pin_r, pin_color);
            painter.text(
                p + Vec2::new(pin_r + 4.0 * zoom, 0.0),
                egui::Align2::LEFT_CENTER,
                &pin.display_name,
                egui::FontId::proportional(10.0 * zoom),
                label_color,
            );
            pin_screens.push(PinScreen {
                pos: p,
                node_id,
                name: pin.name.clone(),
                is_output: false,
            });
        }

        for (i, pin) in output_pins.iter().enumerate() {
            let cy = pin_start_y + i as f32 * pin_row_h + pin_row_h / 2.0;
            let cx = screen_pos.x + node_w - pin_margin;
            let p = Pos2::new(cx, cy);
            painter.circle_filled(p, pin_r, pin_color);
            painter.text(
                p + Vec2::new(-pin_r - 4.0 * zoom, 0.0),
                egui::Align2::RIGHT_CENTER,
                &pin.display_name,
                egui::FontId::proportional(10.0 * zoom),
                label_color,
            );
            pin_screens.push(PinScreen {
                pos: p,
                node_id,
                name: pin.name.clone(),
                is_output: true,
            });
        }

        let own_pins_h = pin_count as f32 * pin_row_h;

        // Expanded children — draw as full nodes inside container
        if is_expanded {
            if let NodeDisplay::Container { child_ids, .. } = display {
                let children_y = pin_start_y + own_pins_h + 8.0 * zoom;
                let pad = 8.0 * zoom;

                painter.line_segment(
                    [
                        Pos2::new(screen_pos.x + 4.0 * zoom, children_y - 4.0 * zoom),
                        Pos2::new(screen_pos.x + node_w - 4.0 * zoom, children_y - 4.0 * zoom),
                    ],
                    Stroke::new(1.0, Color32::from_rgb(60, 60, 60)),
                );

                // Children are positioned relative to the container interior
                let interior_origin = Pos2::new(screen_pos.x + pad, children_y);
                let child_canvas_min = Pos2::new(
                    interior_origin.x - self.state.pan.x,
                    interior_origin.y - self.state.pan.y,
                );

                for &child_id in child_ids {
                    let Some(child_display) = source.get_node_display(child_id) else {
                        continue;
                    };
                    let child_active = source.is_node_active(child_id);
                    let child_expanded = self.state.expanded_containers.contains(&child_id);

                    self.draw_node(
                        source,
                        painter,
                        child_canvas_min,
                        zoom,
                        child_id,
                        &child_display,
                        child_active,
                        child_expanded,
                        pin_screens,
                        interactions,
                    );
                }
            }
        }

        // Resize handle for containers (bottom-right corner)
        if matches!(display, NodeDisplay::Container { .. }) {
            let handle_size = 8.0 * zoom;
            let handle_rect = Rect::from_min_size(
                Pos2::new(node_rect.max.x - handle_size, node_rect.max.y - handle_size),
                Vec2::new(handle_size, handle_size),
            );
            // Draw small triangle
            let pts = [
                handle_rect.right_bottom(),
                Pos2::new(handle_rect.left(), handle_rect.bottom()),
                Pos2::new(handle_rect.right(), handle_rect.top()),
            ];
            painter.add(egui::Shape::convex_polygon(
                pts.to_vec(),
                Color32::from_rgb(100, 100, 120),
                Stroke::NONE,
            ));
        }

        // Port pins: when expanded, draw container output pins as inputs inside
        // and container input pins as outputs inside (bridge for internal connections)
        if is_expanded {
            if let NodeDisplay::Container { .. } = display {
                let port_y = node_rect.max.y - 16.0 * zoom;
                let port_r = pin_r * 0.8;
                let port_col = Color32::from_rgb(200, 200, 100);

                // Output pins → draw as input (right side) inside for children to connect TO
                // Children's outputs flow right → this port pin receives on the right side
                for pin in &output_pins {
                    let p = Pos2::new(screen_pos.x + node_w - pin_margin - 16.0 * zoom, port_y);
                    painter.circle_filled(p, port_r, port_col);
                    painter.text(
                        p + Vec2::new(-port_r - 3.0 * zoom, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        &format!("{} \u{2192}", pin.display_name),
                        egui::FontId::proportional(9.0 * zoom),
                        Color32::from_rgb(200, 200, 100),
                    );
                    // Register as INPUT so children's outputs can connect here
                    pin_screens.push(PinScreen {
                        pos: p,
                        node_id,
                        name: pin.name.clone(),
                        is_output: false,
                    });
                }
            }
        }
    }

    /// Calculate height needed for expanded children based on their positions.
    fn calc_expanded_h(
        &self,
        source: &dyn NodeEditorDataSource,
        child_ids: &[Uuid],
        zoom: f32,
        _depth: usize,
    ) -> f32 {
        let min_h = 150.0 * zoom;
        let mut max_bottom: f32 = 0.0;

        for &cid in child_ids {
            let pos = self
                .state
                .node_positions
                .get(&cid)
                .copied()
                .unwrap_or(Pos2::ZERO);
            let node_h = if let Some(d) = source.get_node_display(cid) {
                let pin_count = match &d {
                    NodeDisplay::Graph { pins, .. }
                    | NodeDisplay::Container { pins, .. }
                    | NodeDisplay::Leaf { pins, .. } => {
                        let ic = pins.iter().filter(|p| !p.is_output).count();
                        let oc = pins.iter().filter(|p| p.is_output).count();
                        ic.max(oc)
                    }
                };
                self.theme.header_height + pin_count as f32 * self.theme.pin_row_height + 8.0
            } else {
                self.theme.header_height + 8.0
            };
            max_bottom = max_bottom.max(pos.y + node_h);
        }

        (max_bottom * zoom + 20.0 * zoom).max(min_h)
    }

    // -----------------------------------------------------------------------
    // Interactions
    // -----------------------------------------------------------------------

    fn handle_interactions(
        &mut self,
        ui: &egui::Ui,
        canvas_response: &egui::Response,
        nodes: &[NodeInteraction],
        pin_screens: &[PinScreen],
        mutator: &dyn NodeEditorMutator,
    ) -> PendingActions {
        let mut pending = PendingActions::default();
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
        let hit_radius = self.theme.pin_radius * self.state.zoom * 4.0;
        let zoom = self.state.zoom;

        // --- Active drag: node move, resize, box select ---
        if canvas_response.dragged_by(egui::PointerButton::Primary) {
            let delta = canvas_response.drag_delta();
            if self.state.dragging.is_some() {
                let inv_zoom = 1.0 / zoom;
                let drag_ids: Vec<Uuid> = self
                    .state
                    .dragging
                    .as_ref()
                    .map(|d| d.node_ids.clone())
                    .unwrap_or_default();
                for nid in &drag_ids {
                    if let Some(pos) = self.state.node_positions.get_mut(nid) {
                        *pos += delta * inv_zoom;
                    }
                }
            } else if self.state.resizing.is_some() {
                let inv_zoom = 1.0 / zoom;
                if let Some(ref rs) = self.state.resizing {
                    let nid = rs.node_id;
                    let new_size = rs.start_size + delta * inv_zoom;
                    let min_w = self.theme.node_width;
                    let min_h = self.theme.header_height + 20.0;
                    self.state
                        .container_sizes
                        .insert(nid, Vec2::new(new_size.x.max(min_w), new_size.y.max(min_h)));
                    // Update start for next delta
                }
                // Accumulate: update start_size to current
                if let Some(ref mut rs) = self.state.resizing {
                    rs.start_size = self
                        .state
                        .container_sizes
                        .get(&rs.node_id)
                        .copied()
                        .unwrap_or(rs.start_size);
                }
            } else if let Some(ref mut bs) = self.state.box_selecting {
                if let Some(pos) = pointer_pos {
                    bs.current = pos;
                }
            }
        }

        // --- Drag stop ---
        if canvas_response.drag_stopped_by(egui::PointerButton::Primary) {
            // Finish connection
            if let Some(connecting) = self.state.connecting.take() {
                if let Some(pos) = pointer_pos {
                    let mut best: Option<(f32, &PinScreen)> = None;
                    for ps in pin_screens {
                        if ps.node_id == connecting.from_node {
                            continue;
                        }
                        if ps.is_output == connecting.is_output {
                            continue;
                        }
                        let d = pos.distance(ps.pos);
                        if d < hit_radius {
                            if best.is_none() || d < best.unwrap().0 {
                                best = Some((d, ps));
                            }
                        }
                    }
                    if let Some((_, target)) = best {
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
            if let Some(bs) = self.state.box_selecting.take() {
                let sel_rect = Rect::from_two_pos(bs.start, bs.current);
                if !ui.input(|i| i.modifiers.shift) {
                    self.state.selected_nodes.clear();
                }
                for node in nodes {
                    if sel_rect.intersects(node.rect) {
                        self.state.selected_nodes.insert(node.id);
                    }
                }
            }

            // Finish node drag — check for reparent
            if let Some(ref drag) = self.state.dragging {
                if let Some(pos) = pointer_pos {
                    let drag_ids = drag.node_ids.clone();
                    // Find if dropped onto a container
                    let mut target_container: Option<Uuid> = None;
                    for node in nodes.iter().rev() {
                        if node.is_container
                            && node.rect.contains(pos)
                            && !drag_ids.contains(&node.id)
                        {
                            target_container = Some(node.id);
                            break;
                        }
                    }
                    // Check if any node changed container
                    if let Some(target) = target_container {
                        for &nid in &drag_ids {
                            // Find current parent
                            let current_parent = nodes
                                .iter()
                                .find(|n| {
                                    n.is_container
                                        && n.id != nid
                                        && n.rect.contains(pos)
                                        && n.id != target
                                })
                                .map(|n| n.id);
                            let from = current_parent
                                .or(self.state.current_container)
                                .unwrap_or(target);
                            if from != target {
                                pending.nodes_to_move.push((nid, from, target));
                            }
                        }
                    }
                }
            }

            self.state.dragging = None;
            self.state.resizing = None;
        }

        // --- Drag start ---
        if canvas_response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(pos) = pointer_pos {
                // 1. Check pin hit
                let mut best_pin: Option<(f32, &PinScreen)> = None;
                for ps in pin_screens {
                    let d = pos.distance(ps.pos);
                    if d < hit_radius {
                        if best_pin.is_none() || d < best_pin.unwrap().0 {
                            best_pin = Some((d, ps));
                        }
                    }
                }

                if let Some((_, ps)) = best_pin {
                    self.state.connecting = Some(ConnectingState {
                        from_node: ps.node_id,
                        from_pin: ps.name.clone(),
                        is_output: ps.is_output,
                        mouse_pos: pos,
                    });
                } else {
                    // 2. Check resize handle (container bottom-right)
                    let mut hit_resize = false;
                    let handle_size = 8.0 * zoom;
                    for node in nodes.iter().rev() {
                        if node.is_container {
                            let handle_rect = Rect::from_min_size(
                                Pos2::new(
                                    node.rect.max.x - handle_size,
                                    node.rect.max.y - handle_size,
                                ),
                                Vec2::new(handle_size, handle_size),
                            );
                            if handle_rect.contains(pos) {
                                let current_size = self
                                    .state
                                    .container_sizes
                                    .get(&node.id)
                                    .copied()
                                    .unwrap_or(node.rect.size() / zoom);
                                self.state.resizing = Some(ResizeState {
                                    node_id: node.id,
                                    start_size: current_size,
                                    mouse_start: pos,
                                });
                                hit_resize = true;
                                break;
                            }
                        }
                    }

                    if !hit_resize {
                        // 3. Check node header for dragging
                        let header_h = self.theme.header_height * zoom;
                        let mut hit_header = false;
                        for node in nodes.iter().rev() {
                            let header_rect = Rect::from_min_size(
                                node.rect.min,
                                Vec2::new(node.rect.width(), header_h),
                            );
                            if header_rect.contains(pos) {
                                if !ui.input(|i| i.modifiers.shift) {
                                    self.state.selected_nodes.clear();
                                }
                                self.state.selected_nodes.insert(node.id);
                                pending.selected_node = Some(node.id);

                                let drag_ids: Vec<Uuid> =
                                    self.state.selected_nodes.iter().copied().collect();
                                let start_positions: Vec<Pos2> = drag_ids
                                    .iter()
                                    .map(|id| {
                                        self.state
                                            .node_positions
                                            .get(id)
                                            .copied()
                                            .unwrap_or(Pos2::ZERO)
                                    })
                                    .collect();
                                self.state.dragging = Some(DragState {
                                    node_ids: drag_ids,
                                    start_positions,
                                    mouse_start: pos,
                                });
                                hit_header = true;
                                break;
                            }
                        }

                        // 4. Empty space → box selection
                        if !hit_header {
                            self.state.box_selecting = Some(BoxSelectState {
                                start: pos,
                                current: pos,
                            });
                        }
                    }
                }
            }
        }

        // --- Update connecting line ---
        if let Some(ref mut connecting) = self.state.connecting {
            if let Some(pos) = pointer_pos {
                connecting.mouse_pos = pos;
            }
        }

        // --- Double-click: toggle container expansion ---
        // Find the smallest (most specific) container containing the click position
        if canvas_response.double_clicked() {
            if let Some(pos) = pointer_pos {
                let mut best: Option<(Uuid, f32)> = None;
                for node in nodes {
                    if node.is_container && node.rect.contains(pos) {
                        let area = node.rect.area();
                        if best.is_none() || area < best.unwrap().1 {
                            best = Some((node.id, area));
                        }
                    }
                }
                if let Some((id, _)) = best {
                    if !self.state.expanded_containers.remove(&id) {
                        self.state.expanded_containers.insert(id);
                    }
                }
            }
        }

        // --- Single click: select node or deselect ---
        if canvas_response.clicked() {
            if let Some(pos) = pointer_pos {
                let mut hit = false;
                for node in nodes.iter().rev() {
                    if node.rect.contains(pos) {
                        if !ui.input(|i| i.modifiers.shift) {
                            self.state.selected_nodes.clear();
                        }
                        self.state.selected_nodes.insert(node.id);
                        pending.selected_node = Some(node.id);
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    self.state.selected_nodes.clear();
                    self.state.selected_connections.clear();
                }
                self.state.context_menu = None;
                self.state.node_context_menu = None;
            }
        }

        // --- Right-click ---
        if canvas_response.secondary_clicked() {
            if let Some(pos) = pointer_pos {
                let mut hit_node = false;
                for node in nodes.iter().rev() {
                    if node.rect.contains(pos) {
                        self.state.node_context_menu = Some(NodeContextMenuState {
                            screen_pos: pos,
                            node_id: node.id,
                        });
                        self.state.context_menu = None;
                        hit_node = true;
                        break;
                    }
                }
                if !hit_node {
                    if let Some(cid) = self.state.current_container {
                        self.state.context_menu = Some(ContextMenuState {
                            screen_pos: pos,
                            container_id: cid,
                        });
                        self.state.context_search.clear();
                    }
                    self.state.node_context_menu = None;
                }
            }
        }

        // --- Render "Add Node" context menu ---
        if let Some(ref menu) = self.state.context_menu.clone() {
            let mut close = false;
            let popup_id = ui.make_persistent_id("node_editor_context_menu");
            egui::Area::new(popup_id)
                .order(egui::Order::Foreground)
                .fixed_pos(menu.screen_pos)
                .show(ui.ctx(), |ui| {
                    egui::Frame::menu(ui.style()).show(ui, |ui| {
                        ui.set_max_width(250.0);
                        ui.label("Add Node");
                        ui.separator();
                        let response = ui.text_edit_singleline(&mut self.state.context_search);
                        if !response.has_focus() {
                            response.request_focus();
                        }
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            close = true;
                        }
                        ui.separator();

                        let node_types = mutator.get_available_node_types();
                        let search_lower = self.state.context_search.to_lowercase();
                        let has_search = !search_lower.is_empty();

                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .show(ui, |ui| {
                                if has_search {
                                    for nt in &node_types {
                                        if nt.display_name.to_lowercase().contains(&search_lower)
                                            || nt.type_id.to_lowercase().contains(&search_lower)
                                        {
                                            let label =
                                                format!("{} ({})", nt.display_name, nt.category);
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
                                                        pending.nodes_to_add.push((
                                                            menu.container_id,
                                                            nt.type_id.clone(),
                                                        ));
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
            if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.state.context_menu = None;
            }
        }

        // --- Render node context menu (right-click on node) ---
        if let Some(ref menu) = self.state.node_context_menu.clone() {
            let mut close = false;
            let sel_count = self.state.selected_nodes.len();
            let popup_id = ui.make_persistent_id("node_context_menu");
            egui::Area::new(popup_id)
                .order(egui::Order::Foreground)
                .fixed_pos(menu.screen_pos)
                .show(ui.ctx(), |ui| {
                    egui::Frame::menu(ui.style()).show(ui, |ui| {
                        ui.set_max_width(180.0);
                        if sel_count > 1 {
                            let label = format!("Delete Selected ({})", sel_count);
                            if ui.button(&label).clicked() {
                                let to_remove: Vec<Uuid> =
                                    self.state.selected_nodes.iter().copied().collect();
                                pending.nodes_to_remove.extend(to_remove);
                                self.state.selected_nodes.clear();
                                close = true;
                            }
                        } else {
                            if ui.button("Delete Node").clicked() {
                                pending.nodes_to_remove.push(menu.node_id);
                                self.state.selected_nodes.remove(&menu.node_id);
                                close = true;
                            }
                        }
                    });
                });
            if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.state.node_context_menu = None;
            }
        }

        // --- Delete key ---
        if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            let to_remove: Vec<Uuid> = self.state.selected_nodes.iter().copied().collect();
            pending.nodes_to_remove.extend(to_remove);
            self.state.selected_nodes.clear();
            let conns: Vec<Uuid> = self.state.selected_connections.iter().copied().collect();
            pending.connections_to_remove.extend(conns);
            self.state.selected_connections.clear();
        }

        pending
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct NodeInteraction {
    id: Uuid,
    rect: Rect,
    is_container: bool,
}

fn dim_color(color: Color32, factor: f32) -> Color32 {
    if factor >= 1.0 {
        return color;
    }
    Color32::from_rgba_unmultiplied(
        (color.r() as f32 * factor) as u8,
        (color.g() as f32 * factor) as u8,
        (color.b() as f32 * factor) as u8,
        color.a(),
    )
}
