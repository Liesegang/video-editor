//! Main node editor widget.

use egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, Vec2};
use std::collections::HashMap;
use uuid::Uuid;

use crate::drawing::{draw_bezier_connection, draw_grid};
use crate::interactions::{self, InteractionContext};
use crate::node_rendering::{self, NodeLayout};
use crate::state::NodeEditorState;
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
pub(crate) struct PinScreen {
    pub pos: Pos2,
    pub node_id: Uuid,
    pub name: String,
    pub is_output: bool,
}

// ---------------------------------------------------------------------------
// Node interaction info
// ---------------------------------------------------------------------------

pub(crate) struct NodeInteraction {
    pub id: Uuid,
    pub rect: Rect,
    pub is_container: bool,
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
                    let graph_pos = (hover - canvas_rect.min - self.state.pan) / old_zoom;
                    self.state.pan = hover - canvas_rect.min - graph_pos * new_zoom;
                    self.state.zoom = new_zoom;
                }
            }
        }
        let zoom = self.state.zoom;

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
        self.draw_connections(&painter, source, &pin_pos_map);
        self.draw_connecting_line(&painter, &pin_pos_map);
        self.draw_box_selection(&painter);

        // ---- Phase 3: Handle interactions ----
        let hit_radius = self.theme.pin_radius * zoom * 4.0;
        let ctx = InteractionContext {
            ui,
            canvas_response: &canvas_response,
            nodes: &node_interactions,
            pin_screens: &pin_screens,
            mutator,
            theme: self.theme,
            zoom,
            hit_radius,
        };
        interactions::handle_interactions(self.state, &ctx)
    }

    // -----------------------------------------------------------------------
    // Drawing helpers (extracted from show)
    // -----------------------------------------------------------------------

    fn draw_connections(
        &self,
        painter: &egui::Painter,
        source: &dyn NodeEditorDataSource,
        pin_pos_map: &HashMap<(Uuid, &str, bool), Pos2>,
    ) {
        let connections = source.get_connections();
        for conn in &connections {
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
                draw_bezier_connection(painter, from_p, to_p, color);
            }
        }
    }

    fn draw_connecting_line(
        &self,
        painter: &egui::Painter,
        pin_pos_map: &HashMap<(Uuid, &str, bool), Pos2>,
    ) {
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
                    painter,
                    start_pos,
                    connecting.mouse_pos,
                    Color32::from_rgb(200, 200, 200),
                );
            }
        }
    }

    fn draw_box_selection(&self, painter: &egui::Painter) {
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
        let pin_start_y = screen_pos.y + header_h + 4.0 * zoom;
        let is_container = matches!(display, NodeDisplay::Container { .. });
        let is_selected = self.state.selected_nodes.contains(&node_id);

        // Push interaction BEFORE drawing children so .rev() finds children first
        interactions.push(NodeInteraction {
            id: node_id,
            rect: node_rect,
            is_container,
        });

        // Build layout for rendering functions
        let layout = NodeLayout {
            screen_pos,
            node_rect,
            header_h,
            pin_row_h,
            pin_r,
            pin_margin,
            rounding,
            node_w,
            pin_start_y,
        };

        // Draw node chrome (body + header)
        node_rendering::draw_node_chrome(
            painter,
            &layout,
            self.theme,
            &node_type_id,
            &display_name,
            is_container,
            is_expanded,
            is_selected,
            is_active,
            zoom,
        );

        // Draw pins
        node_rendering::draw_pins(
            painter,
            &layout,
            self.theme,
            &node_type_id,
            node_id,
            &input_pins,
            &output_pins,
            is_active,
            zoom,
            pin_screens,
        );

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

        // Resize handle for containers
        if is_container {
            node_rendering::draw_resize_handle(painter, node_rect, zoom);
        }

        // Port pins for expanded containers
        if is_expanded && is_container {
            node_rendering::draw_port_pins(
                painter,
                &layout,
                node_id,
                &output_pins,
                zoom,
                pin_screens,
            );
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
}
