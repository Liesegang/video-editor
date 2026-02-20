//! Main node editor widget.

use egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, Vec2};
use uuid::Uuid;

use crate::drawing::{draw_bezier_connection, draw_grid};
use crate::state::{ConnectingState, ContextMenuState, DragState, NodeEditorState};
use crate::theme::NodeEditorTheme;
use crate::traits::{NodeEditorDataSource, NodeEditorMutator};
use crate::types::{NodeDisplay, PinInfo};

/// Pending mutations collected during the render phase, applied after.
#[derive(Default)]
pub struct PendingActions {
    pub nodes_to_remove: Vec<Uuid>,
    pub connections_to_remove: Vec<Uuid>,
    pub connections_to_add: Vec<(Uuid, String, Uuid, String)>,
    pub nodes_to_add: Vec<(Uuid, String)>, // (container_id, type_id)
}

impl PendingActions {
    /// Apply all pending mutations.
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
    }

    pub fn is_empty(&self) -> bool {
        self.nodes_to_remove.is_empty()
            && self.connections_to_remove.is_empty()
            && self.connections_to_add.is_empty()
            && self.nodes_to_add.is_empty()
    }
}

/// The main node editor widget.
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
        let container_id = match self.state.current_container {
            Some(id) => id,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("No container selected");
                });
                return PendingActions::default();
            }
        };

        // Collect children
        let child_ids = source.get_container_children(container_id);
        if child_ids.is_empty() {
            if source.get_container_name(container_id).is_none() {
                ui.label("Container not found");
                return PendingActions::default();
            }
        }

        // Ensure all nodes have positions
        let mut offset_y = 0.0_f32;
        for &child_id in &child_ids {
            self.state
                .node_positions
                .entry(child_id)
                .or_insert_with(|| {
                    let pos = Pos2::new(50.0, 50.0 + offset_y);
                    offset_y += 150.0;
                    pos
                });
            offset_y += 150.0;
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

        // Handle panning
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
            self.theme.grid_spacing,
        );

        // Draw connections
        let connections = source.get_connections();
        for conn in &connections {
            let from_in = child_ids.contains(&conn.from_node);
            let to_in = child_ids.contains(&conn.to_node);
            if !from_in && !to_in {
                continue;
            }

            let from_pos = self.get_pin_screen_pos(
                source,
                conn.from_node,
                &conn.from_pin,
                true,
                canvas_rect.min,
            );
            let to_pos =
                self.get_pin_screen_pos(source, conn.to_node, &conn.to_pin, false, canvas_rect.min);

            if let (Some(from_p), Some(to_p)) = (from_pos, to_pos) {
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

        // Draw connection being created
        if let Some(ref connecting) = self.state.connecting {
            let start = self.get_pin_screen_pos(
                source,
                connecting.from_node,
                &connecting.from_pin,
                connecting.is_output,
                canvas_rect.min,
            );
            if let Some(start_pos) = start {
                draw_bezier_connection(
                    &painter,
                    start_pos,
                    connecting.mouse_pos,
                    Color32::from_rgb(200, 200, 200),
                );
            }
        }

        // Draw nodes and collect interaction data
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
                child_id,
                &display,
                is_active,
                is_expanded,
                &mut node_interactions,
            );
        }

        // Handle interactions
        self.handle_interactions(ui, &canvas_response, &node_interactions, mutator)
    }

    /// Draw a single node (handles both normal and expanded containers).
    #[allow(clippy::too_many_arguments)]
    fn draw_node(
        &mut self,
        source: &dyn NodeEditorDataSource,
        painter: &egui::Painter,
        canvas_min: Pos2,
        node_id: Uuid,
        display: &NodeDisplay,
        is_active: bool,
        is_expanded: bool,
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
        let screen_pos = pos + self.state.pan + canvas_min.to_vec2();

        let input_pins: Vec<&PinInfo> = pins.iter().filter(|p| !p.is_output).collect();
        let output_pins: Vec<&PinInfo> = pins.iter().filter(|p| p.is_output).collect();
        let pin_count = input_pins.len().max(output_pins.len());

        // Calculate expanded children height
        let expanded_children_height = if is_expanded {
            if let NodeDisplay::Container { child_ids, .. } = display {
                self.calculate_expanded_height(source, child_ids)
            } else {
                0.0
            }
        } else {
            0.0
        };

        let node_height = self.theme.header_height
            + pin_count as f32 * self.theme.pin_row_height
            + 8.0
            + expanded_children_height;
        let node_height = node_height.max(self.theme.header_height + 8.0);

        // Expanded containers are wider
        let node_width = if is_expanded {
            self.theme.node_width * 2.0
        } else {
            self.theme.node_width
        };

        let node_rect = Rect::from_min_size(screen_pos, Vec2::new(node_width, node_height));
        let is_selected = self.state.selected_nodes.contains(&node_id);

        // Dim factor for inactive nodes
        let dim = if is_active { 1.0 } else { 0.4 };

        // Node body
        let body_color = if is_selected {
            dim_color(self.theme.node_body_selected_color, dim)
        } else {
            dim_color(self.theme.node_body_color, dim)
        };
        painter.rect_filled(node_rect, self.theme.node_rounding, body_color);

        // Selection outline
        if is_selected {
            painter.rect_stroke(
                node_rect,
                self.theme.node_rounding,
                Stroke::new(2.0, self.theme.selection_color),
                StrokeKind::Outside,
            );
        }

        // Header
        let header_rect =
            Rect::from_min_size(screen_pos, Vec2::new(node_width, self.theme.header_height));
        let header_color = dim_color((self.theme.header_color)(&node_type_id), dim);
        painter.rect_filled(
            header_rect,
            egui::CornerRadius {
                nw: self.theme.node_rounding as u8,
                ne: self.theme.node_rounding as u8,
                sw: 0,
                se: 0,
            },
            header_color,
        );

        // Expansion indicator for containers
        let header_text = if matches!(display, NodeDisplay::Container { .. }) {
            let arrow = if is_expanded { "\u{25BC}" } else { "\u{25B6}" };
            format!("{} {}", arrow, display_name)
        } else {
            display_name.clone()
        };

        painter.text(
            header_rect.center(),
            egui::Align2::CENTER_CENTER,
            &header_text,
            egui::FontId::proportional(12.0),
            dim_color(Color32::WHITE, dim),
        );

        // Draw pins
        let pin_start_y = screen_pos.y + self.theme.header_height + 4.0;
        let pin_color = dim_color((self.theme.pin_color)(&node_type_id), dim);
        let label_color = dim_color(self.theme.pin_label_color, dim);

        for (i, pin) in input_pins.iter().enumerate() {
            let pin_center = Pos2::new(
                screen_pos.x + self.theme.pin_margin,
                pin_start_y
                    + i as f32 * self.theme.pin_row_height
                    + self.theme.pin_row_height / 2.0,
            );
            painter.circle_filled(pin_center, self.theme.pin_radius, pin_color);
            painter.text(
                pin_center + Vec2::new(self.theme.pin_radius + 4.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &pin.display_name,
                egui::FontId::proportional(10.0),
                label_color,
            );
        }

        for (i, pin) in output_pins.iter().enumerate() {
            let pin_center = Pos2::new(
                screen_pos.x + node_width - self.theme.pin_margin,
                pin_start_y
                    + i as f32 * self.theme.pin_row_height
                    + self.theme.pin_row_height / 2.0,
            );
            painter.circle_filled(pin_center, self.theme.pin_radius, pin_color);
            painter.text(
                pin_center + Vec2::new(-self.theme.pin_radius - 4.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                &pin.display_name,
                egui::FontId::proportional(10.0),
                label_color,
            );
        }

        let own_pins_height = pin_count as f32 * self.theme.pin_row_height;

        // Draw expanded children inside the container
        if is_expanded {
            if let NodeDisplay::Container { child_ids, .. } = display {
                let children_y = pin_start_y + own_pins_height + 8.0;
                let inner_padding = 8.0;

                // Separator line between pins and children
                painter.line_segment(
                    [
                        Pos2::new(screen_pos.x + 4.0, children_y - 4.0),
                        Pos2::new(screen_pos.x + node_width - 4.0, children_y - 4.0),
                    ],
                    Stroke::new(1.0, Color32::from_rgb(60, 60, 60)),
                );

                // Draw each child as a mini-node inside
                let mut child_y = children_y;
                for &child_id in child_ids {
                    let Some(child_display) = source.get_node_display(child_id) else {
                        continue;
                    };

                    let child_active = source.is_node_active(child_id);
                    let child_dim = if child_active { 1.0 } else { 0.4 };

                    let (child_type_id, child_name, child_pins) = match &child_display {
                        NodeDisplay::Graph {
                            type_id,
                            display_name,
                            pins,
                        } => (type_id.clone(), display_name.clone(), pins.clone()),
                        NodeDisplay::Container { name, pins, .. } => {
                            ("track".to_string(), name.clone(), pins.clone())
                        }
                        NodeDisplay::Leaf { kind_label, pins } => (
                            format!("clip.{}", kind_label),
                            kind_label.clone(),
                            pins.clone(),
                        ),
                    };

                    let child_input_pins: Vec<&PinInfo> =
                        child_pins.iter().filter(|p| !p.is_output).collect();
                    let child_output_pins: Vec<&PinInfo> =
                        child_pins.iter().filter(|p| p.is_output).collect();
                    let child_pin_count = child_input_pins.len().max(child_output_pins.len());

                    let child_inner_width = node_width - inner_padding * 2.0;
                    let child_header_h = 20.0;
                    let child_pin_row_h = 16.0;
                    let child_height =
                        child_header_h + child_pin_count as f32 * child_pin_row_h + 6.0;
                    let child_height = child_height.max(child_header_h + 6.0);

                    let child_rect = Rect::from_min_size(
                        Pos2::new(screen_pos.x + inner_padding, child_y),
                        Vec2::new(child_inner_width, child_height),
                    );

                    // Child body
                    let child_body = dim_color(Color32::from_rgb(35, 35, 40), child_dim);
                    painter.rect_filled(child_rect, 3.0, child_body);

                    // Child header
                    let child_header_rect = Rect::from_min_size(
                        child_rect.min,
                        Vec2::new(child_inner_width, child_header_h),
                    );
                    let child_header_color =
                        dim_color((self.theme.header_color)(&child_type_id), child_dim);
                    painter.rect_filled(
                        child_header_rect,
                        egui::CornerRadius {
                            nw: 3,
                            ne: 3,
                            sw: 0,
                            se: 0,
                        },
                        child_header_color,
                    );
                    painter.text(
                        child_header_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &child_name,
                        egui::FontId::proportional(10.0),
                        dim_color(Color32::WHITE, child_dim),
                    );

                    // Child pins
                    let child_pin_start = child_rect.min.y + child_header_h + 3.0;
                    let child_pin_color =
                        dim_color((self.theme.pin_color)(&child_type_id), child_dim);
                    let child_label_color = dim_color(self.theme.pin_label_color, child_dim);

                    for (i, pin) in child_input_pins.iter().enumerate() {
                        let cy =
                            child_pin_start + i as f32 * child_pin_row_h + child_pin_row_h / 2.0;
                        let cx = child_rect.min.x + 8.0;
                        painter.circle_filled(Pos2::new(cx, cy), 3.5, child_pin_color);
                        painter.text(
                            Pos2::new(cx + 6.0, cy),
                            egui::Align2::LEFT_CENTER,
                            &pin.display_name,
                            egui::FontId::proportional(9.0),
                            child_label_color,
                        );
                    }

                    for (i, pin) in child_output_pins.iter().enumerate() {
                        let cy =
                            child_pin_start + i as f32 * child_pin_row_h + child_pin_row_h / 2.0;
                        let cx = child_rect.max.x - 8.0;
                        painter.circle_filled(Pos2::new(cx, cy), 3.5, child_pin_color);
                        painter.text(
                            Pos2::new(cx - 6.0, cy),
                            egui::Align2::RIGHT_CENTER,
                            &pin.display_name,
                            egui::FontId::proportional(9.0),
                            child_label_color,
                        );
                    }

                    // Store interaction for child node (allow pin connections)
                    interactions.push(NodeInteraction {
                        id: child_id,
                        rect: child_rect,
                        input_pins: child_input_pins.iter().map(|p| p.name.clone()).collect(),
                        output_pins: child_output_pins.iter().map(|p| p.name.clone()).collect(),
                        pin_start_y: child_pin_start,
                        pin_row_height: child_pin_row_h,
                        pin_margin: 8.0,
                        node_width: child_inner_width,
                        is_container: matches!(child_display, NodeDisplay::Container { .. }),
                    });

                    child_y += child_height + 4.0;
                }
            }
        }

        interactions.push(NodeInteraction {
            id: node_id,
            rect: node_rect,
            input_pins: input_pins.iter().map(|p| p.name.clone()).collect(),
            output_pins: output_pins.iter().map(|p| p.name.clone()).collect(),
            pin_start_y,
            pin_row_height: self.theme.pin_row_height,
            pin_margin: self.theme.pin_margin,
            node_width,
            is_container: matches!(display, NodeDisplay::Container { .. }),
        });
    }

    /// Calculate the total height needed for expanded container children.
    fn calculate_expanded_height(
        &self,
        source: &dyn NodeEditorDataSource,
        child_ids: &[Uuid],
    ) -> f32 {
        let child_header_h = 20.0;
        let child_pin_row_h = 16.0;
        let mut total = 8.0; // top separator padding

        for &child_id in child_ids {
            let Some(display) = source.get_node_display(child_id) else {
                continue;
            };
            let pins = match &display {
                NodeDisplay::Graph { pins, .. } => pins,
                NodeDisplay::Container { pins, .. } => pins,
                NodeDisplay::Leaf { pins, .. } => pins,
            };
            let input_count = pins.iter().filter(|p| !p.is_output).count();
            let output_count = pins.iter().filter(|p| p.is_output).count();
            let pin_count = input_count.max(output_count);
            let child_height = child_header_h + pin_count as f32 * child_pin_row_h + 6.0;
            let child_height = child_height.max(child_header_h + 6.0);
            total += child_height + 4.0;
        }

        total
    }

    fn handle_interactions(
        &mut self,
        ui: &egui::Ui,
        canvas_response: &egui::Response,
        nodes: &[NodeInteraction],
        mutator: &dyn NodeEditorMutator,
    ) -> PendingActions {
        let mut pending = PendingActions::default();
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());

        // Handle node dragging
        if canvas_response.dragged_by(egui::PointerButton::Primary) {
            if self.state.dragging.is_some() {
                let delta = canvas_response.drag_delta();
                let drag_ids: Vec<Uuid> = self
                    .state
                    .dragging
                    .as_ref()
                    .map(|d| d.node_ids.clone())
                    .unwrap_or_default();
                for node_id in &drag_ids {
                    if let Some(pos) = self.state.node_positions.get_mut(node_id) {
                        *pos += delta;
                    }
                }
            }
        }

        if canvas_response.drag_stopped_by(egui::PointerButton::Primary) {
            // Finish connection creation
            if let Some(connecting) = self.state.connecting.take() {
                if let Some(pos) = pointer_pos {
                    for node in nodes {
                        if !node.rect.contains(pos) {
                            continue;
                        }
                        if connecting.is_output {
                            for (i, pin_name) in node.input_pins.iter().enumerate() {
                                let pin_y = node.pin_start_y
                                    + i as f32 * node.pin_row_height
                                    + node.pin_row_height / 2.0;
                                let pin_pos = Pos2::new(node.rect.min.x + node.pin_margin, pin_y);
                                if pos.distance(pin_pos) < self.theme.pin_radius * 3.0 {
                                    pending.connections_to_add.push((
                                        connecting.from_node,
                                        connecting.from_pin.clone(),
                                        node.id,
                                        pin_name.clone(),
                                    ));
                                    break;
                                }
                            }
                        } else {
                            for (i, pin_name) in node.output_pins.iter().enumerate() {
                                let pin_y = node.pin_start_y
                                    + i as f32 * node.pin_row_height
                                    + node.pin_row_height / 2.0;
                                let pin_pos = Pos2::new(
                                    node.rect.min.x + node.node_width - node.pin_margin,
                                    pin_y,
                                );
                                if pos.distance(pin_pos) < self.theme.pin_radius * 3.0 {
                                    pending.connections_to_add.push((
                                        node.id,
                                        pin_name.clone(),
                                        connecting.from_node,
                                        connecting.from_pin.clone(),
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            self.state.dragging = None;
        }

        // Handle click / drag start
        if canvas_response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(pos) = pointer_pos {
                let mut hit_pin = false;

                for node in nodes {
                    if !node.rect.contains(pos) {
                        continue;
                    }

                    // Check output pins
                    for (i, pin_name) in node.output_pins.iter().enumerate() {
                        let pin_y = node.pin_start_y
                            + i as f32 * node.pin_row_height
                            + node.pin_row_height / 2.0;
                        let pin_pos =
                            Pos2::new(node.rect.min.x + node.node_width - node.pin_margin, pin_y);
                        if pos.distance(pin_pos) < self.theme.pin_radius * 3.0 {
                            self.state.connecting = Some(ConnectingState {
                                from_node: node.id,
                                from_pin: pin_name.clone(),
                                is_output: true,
                                mouse_pos: pos,
                            });
                            hit_pin = true;
                            break;
                        }
                    }
                    if hit_pin {
                        break;
                    }

                    // Check input pins
                    for (i, pin_name) in node.input_pins.iter().enumerate() {
                        let pin_y = node.pin_start_y
                            + i as f32 * node.pin_row_height
                            + node.pin_row_height / 2.0;
                        let pin_pos = Pos2::new(node.rect.min.x + node.pin_margin, pin_y);
                        if pos.distance(pin_pos) < self.theme.pin_radius * 3.0 {
                            self.state.connecting = Some(ConnectingState {
                                from_node: node.id,
                                from_pin: pin_name.clone(),
                                is_output: false,
                                mouse_pos: pos,
                            });
                            hit_pin = true;
                            break;
                        }
                    }
                    if hit_pin {
                        break;
                    }
                }

                if !hit_pin {
                    // Check if clicking on a node header (for dragging)
                    for node in nodes {
                        let header_rect = Rect::from_min_size(
                            node.rect.min,
                            Vec2::new(node.node_width, self.theme.header_height),
                        );
                        if header_rect.contains(pos) {
                            if !ui.input(|i| i.modifiers.shift) {
                                self.state.selected_nodes.clear();
                            }
                            self.state.selected_nodes.insert(node.id);

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
                            break;
                        }
                    }
                }
            }
        }

        // Update connecting line endpoint
        if let Some(ref mut connecting) = self.state.connecting {
            if let Some(pos) = pointer_pos {
                connecting.mouse_pos = pos;
            }
        }

        // Handle double-click on containers → toggle inline expansion (no shift needed)
        if canvas_response.double_clicked() {
            if let Some(pos) = pointer_pos {
                for node in nodes {
                    if node.is_container && node.rect.contains(pos) {
                        if !self.state.expanded_containers.remove(&node.id) {
                            self.state.expanded_containers.insert(node.id);
                        }
                        break;
                    }
                }
            }
        }

        // Handle click on empty space (deselect)
        if canvas_response.clicked() {
            if let Some(pos) = pointer_pos {
                let hit = nodes.iter().any(|n| n.rect.contains(pos));
                if !hit {
                    self.state.selected_nodes.clear();
                    self.state.selected_connections.clear();
                }
            }
        }

        // Right-click context menu
        if canvas_response.secondary_clicked() {
            if let Some(pos) = pointer_pos {
                if let Some(cid) = self.state.current_container {
                    self.state.context_menu = Some(ContextMenuState {
                        screen_pos: pos,
                        container_id: cid,
                    });
                    self.state.context_search.clear();
                }
            }
        }

        // Render context menu
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

                        // Search field
                        let search_id = ui.make_persistent_id("node_search_field");
                        let response = ui.text_edit_singleline(&mut self.state.context_search);
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            close = true;
                        }
                        if response.gained_focus() || self.state.context_search.is_empty() {
                            response.request_focus();
                            let _ = search_id;
                        }

                        ui.separator();

                        let node_types = mutator.get_available_node_types();
                        let search_lower = self.state.context_search.to_lowercase();

                        let mut categories: Vec<String> = node_types
                            .iter()
                            .map(|nt| nt.category.clone())
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect();
                        categories.sort();

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

                        ui.separator();
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                });

            if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.state.context_menu = None;
            }
        }

        // Delete key
        if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            let to_remove: Vec<Uuid> = self.state.selected_nodes.iter().copied().collect();
            pending.nodes_to_remove.extend(to_remove);
            self.state.selected_nodes.clear();

            let conns_to_remove: Vec<Uuid> =
                self.state.selected_connections.iter().copied().collect();
            pending.connections_to_remove.extend(conns_to_remove);
            self.state.selected_connections.clear();
        }

        pending
    }

    /// Get the screen position of a pin on a node.
    fn get_pin_screen_pos(
        &self,
        source: &dyn NodeEditorDataSource,
        node_id: Uuid,
        pin_name: &str,
        is_output: bool,
        canvas_min: Pos2,
    ) -> Option<Pos2> {
        let node_pos = self.state.node_positions.get(&node_id)?.to_vec2();
        let screen_offset = self.state.pan + canvas_min.to_vec2() + node_pos;
        let pin_start_y = self.theme.header_height + 4.0;

        let display = source.get_node_display(node_id)?;
        let pins = match &display {
            NodeDisplay::Graph { pins, .. } => pins.clone(),
            NodeDisplay::Leaf { pins, .. } => pins.clone(),
            NodeDisplay::Container { pins, .. } => pins.clone(),
        };

        let is_expanded = self.state.expanded_containers.contains(&node_id);
        let node_width = if is_expanded {
            self.theme.node_width * 2.0
        } else {
            self.theme.node_width
        };

        let filtered: Vec<&PinInfo> = pins.iter().filter(|p| p.is_output == is_output).collect();
        let idx = filtered.iter().position(|p| p.name == pin_name)?;

        let x = if is_output {
            screen_offset.x + node_width - self.theme.pin_margin
        } else {
            screen_offset.x + self.theme.pin_margin
        };
        let y = screen_offset.y
            + pin_start_y
            + idx as f32 * self.theme.pin_row_height
            + self.theme.pin_row_height / 2.0;

        Some(Pos2::new(x, y))
    }
}

struct NodeInteraction {
    id: Uuid,
    rect: Rect,
    input_pins: Vec<String>,
    output_pins: Vec<String>,
    pin_start_y: f32,
    pin_row_height: f32,
    pin_margin: f32,
    node_width: f32,
    is_container: bool,
}

/// Dim a color by a factor (0.0 = fully dimmed, 1.0 = original).
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
