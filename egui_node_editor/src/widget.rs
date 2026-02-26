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
use crate::types::{ConnectionView, ContainerKind, NodeDisplay, PinDataType, PinInfo};

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
    /// (node_id, pin_name, new_value_string)
    pub pin_value_changes: Vec<(Uuid, String, String)>,
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
        for (node_id, pin_name, value_str) in self.pin_value_changes {
            let _ = mutator.set_pin_value(node_id, &pin_name, &value_str);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes_to_remove.is_empty()
            && self.connections_to_remove.is_empty()
            && self.connections_to_add.is_empty()
            && self.nodes_to_add.is_empty()
            && self.nodes_to_move.is_empty()
            && self.pin_value_changes.is_empty()
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
    pub data_type: PinDataType,
    /// Which container this pin is "inside". Pins at the same nesting level share the same value.
    /// Port pins (internal container connection points) share container_id with the container's children.
    pub container_id: Option<Uuid>,
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

        // Ensure all top-level children have positions.
        // Place new nodes below existing ones to avoid overlap.
        let mut max_y = 50.0_f32;
        for &child_id in &child_ids {
            if let Some(pos) = self.state.node_positions.get(&child_id) {
                max_y = max_y.max(pos.y + 200.0);
            }
        }
        for &child_id in &child_ids {
            self.state
                .node_positions
                .entry(child_id)
                .or_insert_with(|| {
                    let pos = Pos2::new(50.0, max_y);
                    max_y += 200.0;
                    pos
                });
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

        // Zoom via scroll wheel (skip when context menus are open to prevent scroll leak)
        let any_menu_open = self.state.context_menu.is_some()
            || self.state.node_context_menu.is_some()
            || self.state.edge_context_menu.is_some();
        if !any_menu_open {
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
                Some(container_id), // Top-level nodes are inside the current container
                canvas_rect,        // Top-level nodes are clipped to canvas
            );
        }

        // Resize cursor for container edges
        let edge_width = 6.0 * zoom;
        let resize_handle_size = 16.0 * zoom;
        let header_h = self.theme.header_height * zoom;
        if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
            let is_resizing = self.state.resizing.is_some();
            let mut on_resize_area = is_resizing;

            if !is_resizing {
                for node in node_interactions.iter().rev() {
                    if node.is_container {
                        let handle_rect = Rect::from_min_size(
                            Pos2::new(
                                node.rect.max.x - resize_handle_size,
                                node.rect.max.y - resize_handle_size,
                            ),
                            Vec2::new(resize_handle_size, resize_handle_size),
                        );
                        let right_edge = Rect::from_min_max(
                            Pos2::new(node.rect.max.x - edge_width, node.rect.min.y + header_h),
                            node.rect.max,
                        );
                        let bottom_edge = Rect::from_min_max(
                            Pos2::new(node.rect.min.x, node.rect.max.y - edge_width),
                            node.rect.max,
                        );
                        if handle_rect.contains(hover_pos)
                            || right_edge.contains(hover_pos)
                            || bottom_edge.contains(hover_pos)
                        {
                            on_resize_area = true;
                            break;
                        }
                    }
                }
            }

            if on_resize_area {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
        }

        // Build lookup: (node_id, pin_name, is_output) -> screen pos
        let pin_pos_map: HashMap<(Uuid, &str, bool), Pos2> = pin_screens
            .iter()
            .map(|p| ((p.node_id, p.name.as_str(), p.is_output), p.pos))
            .collect();

        // ---- Phase 2: Draw connections ON TOP of nodes ----
        let connections = source.get_connections();
        self.draw_connections(&painter, source, &connections, &pin_pos_map, &pin_screens);
        self.draw_connecting_line(&painter, &pin_pos_map);
        self.draw_box_selection(&painter);

        // ---- Phase 3: Handle interactions ----
        let hit_radius = self.theme.pin_radius * zoom * 4.0;
        let ctx = InteractionContext {
            ui,
            canvas_response: &canvas_response,
            nodes: &node_interactions,
            pin_screens: &pin_screens,
            connections: &connections,
            pin_pos_map: &pin_pos_map,
            mutator,
            theme: self.theme,
            zoom,
            hit_radius,
        };
        let mut pending = interactions::handle_interactions(self.state, &ctx);

        // ---- Phase 3.5: Inline editors for unconnected input pins ----
        self.draw_inline_editors(ui, source, &pin_screens, &connections, zoom, &mut pending);

        pending
    }

    // -----------------------------------------------------------------------
    // Drawing helpers (extracted from show)
    // -----------------------------------------------------------------------

    fn draw_connections(
        &self,
        painter: &egui::Painter,
        _source: &dyn NodeEditorDataSource,
        connections: &[ConnectionView],
        pin_pos_map: &HashMap<(Uuid, &str, bool), Pos2>,
        pin_screens: &[PinScreen],
    ) {
        for conn in connections {
            let from_pos = pin_pos_map
                .get(&(conn.from_node, conn.from_pin.as_str(), true))
                .or_else(|| pin_pos_map.get(&(conn.from_node, conn.from_pin.as_str(), false)));
            let to_pos = pin_pos_map
                .get(&(conn.to_node, conn.to_pin.as_str(), false))
                .or_else(|| pin_pos_map.get(&(conn.to_node, conn.to_pin.as_str(), true)));

            if let (Some(&from_p), Some(&to_p)) = (from_pos, to_pos) {
                let color = if self.state.selected_connections.contains(&conn.id) {
                    self.theme.connection_selected_color
                } else {
                    // Use the output pin's data type color for the connection
                    let from_type = pin_screens
                        .iter()
                        .find(|ps| {
                            ps.node_id == conn.from_node && ps.name == conn.from_pin && ps.is_output
                        })
                        .map(|ps| &ps.data_type);
                    if let Some(dt) = from_type {
                        (self.theme.pin_type_color)(dt)
                    } else {
                        self.theme.connection_color
                    }
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
    // Inline pin value display and editing
    // -----------------------------------------------------------------------

    /// Draw inline editors for unconnected input pins.
    fn draw_inline_editors(
        &self,
        ui: &mut egui::Ui,
        source: &dyn NodeEditorDataSource,
        pin_screens: &[PinScreen],
        connections: &[ConnectionView],
        zoom: f32,
        pending: &mut PendingActions,
    ) {
        use crate::traits::PinEditValue;

        let disabled_color = Color32::from_rgb(100, 100, 110);
        let font = egui::FontId::proportional(9.0 * zoom);
        let editor_offset_x = self.theme.pin_radius * zoom + 4.0 * zoom + 50.0 * zoom;

        // Scale widget fonts and spacing with zoom so DragValue/ComboBox/TextEdit match node text
        ui.style_mut().override_font_id = Some(egui::FontId::proportional(10.0 * zoom));
        ui.spacing_mut().interact_size = Vec2::new(40.0 * zoom, 18.0 * zoom);
        ui.spacing_mut().button_padding = Vec2::new(4.0 * zoom, 1.0 * zoom);

        for ps in pin_screens {
            if ps.is_output {
                continue;
            }

            let is_connected = connections
                .iter()
                .any(|c| c.to_node == ps.node_id && c.to_pin == ps.name);

            let prop_info = source.get_pin_property(ps.node_id, &ps.name);

            if is_connected {
                // Show read-only display for connected pins
                if let Some(val_str) = source.get_pin_value_display(ps.node_id, &ps.name) {
                    let display = if val_str.len() > 12 {
                        format!("{}...", &val_str[..10])
                    } else {
                        val_str
                    };
                    let text_pos = ps.pos + Vec2::new(editor_offset_x, 0.0);
                    ui.painter().text(
                        text_pos,
                        egui::Align2::LEFT_CENTER,
                        &display,
                        font.clone(),
                        disabled_color,
                    );
                }
                continue;
            }

            let Some(info) = prop_info else {
                // Fallback to text display if no property info
                if let Some(val_str) = source.get_pin_value_display(ps.node_id, &ps.name) {
                    let display = if val_str.len() > 12 {
                        format!("{}...", &val_str[..10])
                    } else {
                        val_str
                    };
                    let text_pos = ps.pos + Vec2::new(editor_offset_x, 0.0);
                    ui.painter().text(
                        text_pos,
                        egui::Align2::LEFT_CENTER,
                        &display,
                        font.clone(),
                        disabled_color,
                    );
                }
                continue;
            };

            // Render an inline editor based on the value type
            let widget_pos = ps.pos + Vec2::new(editor_offset_x, -7.0 * zoom);
            let node_id = ps.node_id;
            let pin_name = ps.name.clone();

            match info.value {
                PinEditValue::Scalar(mut val) => {
                    let w = 60.0 * zoom;
                    let rect = Rect::from_min_size(widget_pos, Vec2::new(w, 14.0 * zoom));
                    let resp = ui.put(rect, egui::DragValue::new(&mut val).speed(0.1));
                    if resp.changed() {
                        pending
                            .pin_value_changes
                            .push((node_id, pin_name, val.to_string()));
                    }
                }
                PinEditValue::Integer(mut val) => {
                    let w = 60.0 * zoom;
                    let rect = Rect::from_min_size(widget_pos, Vec2::new(w, 14.0 * zoom));
                    let resp = ui.put(rect, egui::DragValue::new(&mut val).speed(1.0));
                    if resp.changed() {
                        pending
                            .pin_value_changes
                            .push((node_id, pin_name, val.to_string()));
                    }
                }
                PinEditValue::Boolean(mut val) => {
                    let rect = Rect::from_min_size(widget_pos, Vec2::new(14.0 * zoom, 14.0 * zoom));
                    let resp = ui.put(rect, egui::Checkbox::without_text(&mut val));
                    if resp.changed() {
                        pending
                            .pin_value_changes
                            .push((node_id, pin_name, val.to_string()));
                    }
                }
                PinEditValue::Color(rgba) => {
                    let mut color32 = Color32::from_rgba_unmultiplied(
                        (rgba[0] * 255.0) as u8,
                        (rgba[1] * 255.0) as u8,
                        (rgba[2] * 255.0) as u8,
                        (rgba[3] * 255.0) as u8,
                    );
                    let rect = Rect::from_min_size(widget_pos, Vec2::new(20.0 * zoom, 14.0 * zoom));
                    let resp = ui.put(rect, |ui: &mut egui::Ui| {
                        ui.color_edit_button_srgba(&mut color32)
                    });
                    if resp.changed() {
                        let [r, g, b, a] = color32.to_array();
                        let color_str = format!("{},{},{},{}", r, g, b, a);
                        pending
                            .pin_value_changes
                            .push((node_id, pin_name, color_str));
                    }
                }
                PinEditValue::Vec2(mut x, mut y) => {
                    let w = 40.0 * zoom;
                    let h = 14.0 * zoom;
                    let rect_x = Rect::from_min_size(widget_pos, Vec2::new(w, h));
                    let rect_y = Rect::from_min_size(
                        widget_pos + Vec2::new(w + 2.0 * zoom, 0.0),
                        Vec2::new(w, h),
                    );
                    let rx = ui.put(rect_x, egui::DragValue::new(&mut x).speed(0.1));
                    let ry = ui.put(rect_y, egui::DragValue::new(&mut y).speed(0.1));
                    if rx.changed() || ry.changed() {
                        pending
                            .pin_value_changes
                            .push((node_id, pin_name, format!("{},{}", x, y)));
                    }
                }
                PinEditValue::Vec3(mut x, mut y, mut z) => {
                    let w = 36.0 * zoom;
                    let h = 14.0 * zoom;
                    let gap = 2.0 * zoom;
                    let rect_x = Rect::from_min_size(widget_pos, Vec2::new(w, h));
                    let rect_y =
                        Rect::from_min_size(widget_pos + Vec2::new(w + gap, 0.0), Vec2::new(w, h));
                    let rect_z = Rect::from_min_size(
                        widget_pos + Vec2::new((w + gap) * 2.0, 0.0),
                        Vec2::new(w, h),
                    );
                    let rx = ui.put(rect_x, egui::DragValue::new(&mut x).speed(0.1));
                    let ry = ui.put(rect_y, egui::DragValue::new(&mut y).speed(0.1));
                    let rz = ui.put(rect_z, egui::DragValue::new(&mut z).speed(0.1));
                    if rx.changed() || ry.changed() || rz.changed() {
                        pending.pin_value_changes.push((
                            node_id,
                            pin_name,
                            format!("{},{},{}", x, y, z),
                        ));
                    }
                }
                PinEditValue::String(mut val) => {
                    let w = 80.0 * zoom;
                    let rect = Rect::from_min_size(widget_pos, Vec2::new(w, 14.0 * zoom));
                    let resp = ui.put(
                        rect,
                        egui::TextEdit::singleline(&mut val)
                            .font(font.clone())
                            .desired_width(w),
                    );
                    if resp.changed() {
                        pending.pin_value_changes.push((node_id, pin_name, val));
                    }
                }
                PinEditValue::Enum {
                    mut selected,
                    ref options,
                } => {
                    let w = 80.0 * zoom;
                    let rect = Rect::from_min_size(widget_pos, Vec2::new(w, 14.0 * zoom));
                    let current_label = options.get(selected).cloned().unwrap_or_default();
                    let resp = ui.put(rect, |ui: &mut egui::Ui| {
                        let mut resp = egui::ComboBox::from_id_salt(format!(
                            "pin_enum_{}_{}",
                            node_id, pin_name
                        ))
                        .selected_text(&current_label)
                        .width(w - 16.0 * zoom)
                        .show_ui(ui, |ui| {
                            for (i, opt) in options.iter().enumerate() {
                                ui.selectable_value(&mut selected, i, opt);
                            }
                        });
                        if resp.inner.is_some() {
                            resp.response.mark_changed();
                        }
                        resp.response
                    });
                    if resp.changed() {
                        pending
                            .pin_value_changes
                            .push((node_id, pin_name, selected.to_string()));
                    }
                }
                PinEditValue::None => {
                    // Non-editable type (Image, Shape, Style) — show type label
                    let label = match info.data_type {
                        PinDataType::Image => "Image",
                        PinDataType::Shape => "Shape",
                        PinDataType::Style => "Style",
                        _ => "",
                    };
                    if !label.is_empty() {
                        let text_pos = ps.pos + Vec2::new(editor_offset_x, 0.0);
                        ui.painter().text(
                            text_pos,
                            egui::Align2::LEFT_CENTER,
                            label,
                            font.clone(),
                            disabled_color,
                        );
                    }
                }
            }
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
        parent_container_id: Option<Uuid>,
        visible_clip_rect: Rect,
    ) {
        let (node_type_id, display_name, pins) = match display {
            NodeDisplay::Graph {
                type_id,
                display_name,
                pins,
            } => (type_id.clone(), display_name.clone(), pins.clone()),
            NodeDisplay::Container {
                kind, name, pins, ..
            } => {
                let (type_id, prefix) = match kind {
                    ContainerKind::Composition => ("composition", "Composition"),
                    ContainerKind::Track => ("track", "Track"),
                    ContainerKind::Layer => ("layer", "Layer"),
                };
                (
                    type_id.to_string(),
                    format!("{}: {}", prefix, name),
                    pins.clone(),
                )
            }
            NodeDisplay::Leaf { kind_label, pins } => {
                let type_id = format!("source.{}", kind_label);
                (type_id, format!("Source ({})", kind_label), pins.clone())
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
        let is_container = matches!(display, NodeDisplay::Container { .. });
        // For containers, paired pins share rows — count only output pins.
        let pin_count = if is_container {
            output_pins.len()
        } else {
            input_pins.len().max(output_pins.len())
        };

        // Initialize child positions early (needed for expanded height calculation)
        // Place new children below existing ones to avoid overlap.
        if let NodeDisplay::Container { child_ids, .. } = display {
            let mut max_child_y = 10.0f32;
            for &child_id in child_ids {
                if let Some(pos) = self.state.node_positions.get(&child_id) {
                    max_child_y = max_child_y.max(pos.y + 100.0);
                }
            }
            for &child_id in child_ids {
                if !self.state.node_positions.contains_key(&child_id) {
                    self.state
                        .node_positions
                        .insert(child_id, Pos2::new(10.0, max_child_y));
                    max_child_y += 100.0;
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

        // Use custom size only for expanded containers; collapsed containers revert to auto size
        let (node_w, node_h) = if is_expanded {
            if let Some(custom) = self.state.container_sizes.get(&node_id) {
                (
                    (custom.x * zoom).max(auto_w.min(self.theme.node_width * zoom)),
                    (custom.y * zoom).max(header_h + 8.0 * zoom),
                )
            } else {
                (auto_w, auto_h)
            }
        } else {
            (auto_w, auto_h)
        };

        let node_rect = Rect::from_min_size(screen_pos, Vec2::new(node_w, node_h));
        let pin_start_y = screen_pos.y + header_h + 4.0 * zoom;
        let is_selected = self.state.selected_nodes.contains(&node_id);

        // Push interaction BEFORE drawing children so .rev() finds children first.
        // Clip to visible area — nodes outside the viewport or container bounds are not interactive.
        if let Some(clipped_rect) =
            interactions::clip_interaction_rect(node_rect, visible_clip_rect)
        {
            interactions.push(NodeInteraction {
                id: node_id,
                rect: clipped_rect,
                is_container,
            });
        }

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
            is_container,
            is_active,
            zoom,
            pin_screens,
            parent_container_id,
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

                // Clip children to container interior
                let clip_rect = Rect::from_min_max(
                    Pos2::new(screen_pos.x, children_y),
                    Pos2::new(screen_pos.x + node_w, screen_pos.y + node_h),
                );
                let child_painter = painter.with_clip_rect(clip_rect);

                for &child_id in child_ids {
                    let Some(child_display) = source.get_node_display(child_id) else {
                        continue;
                    };
                    let child_active = source.is_node_active(child_id);
                    let child_expanded = self.state.expanded_containers.contains(&child_id);

                    self.draw_node(
                        source,
                        &child_painter,
                        child_canvas_min,
                        zoom,
                        child_id,
                        &child_display,
                        child_active,
                        child_expanded,
                        pin_screens,
                        interactions,
                        Some(node_id), // Children are inside this container
                        clip_rect,     // Children are clipped to container interior
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
                Some(node_id), // Port pins are internal — same scope as container's children
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
                    NodeDisplay::Container { pins, .. } => {
                        // Containers: paired pins share rows
                        pins.iter().filter(|p| p.is_output).count()
                    }
                    NodeDisplay::Graph { pins, .. } | NodeDisplay::Leaf { pins, .. } => {
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
