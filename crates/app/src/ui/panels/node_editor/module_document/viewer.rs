//! Module-specific data access for the production `egui-snarl` surface.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use egui_phosphor::regular as icons;
use egui_snarl::ui::{
    BackgroundPattern, NodeLayout, PinInfo, PinWireInfo, SnarlPin, SnarlStyle, SnarlViewer,
};
use egui_snarl::{InPin, OutPin, Snarl};
use library::model::project::{PortDefinition, PortDirection};
use node_editor_ui::{Editor, HeaderGlyph, NodeBodyResponse, NodeHeader, PortLabel};

use super::*;
use crate::ui::panels::node_editor::{
    measured_label_width, node_editor_details_visible, node_editor_port_interactions_enabled,
    node_icon_for_node, node_palette_for_node, paint_node_editor_canvas_grid, pin_info,
    NODE_HEADER_WIDTH, PORT_LABEL_WIDTH, PORT_ROW_HEIGHT,
};

#[derive(Default)]
pub(super) struct ModuleSurfaceCapture {
    pub(super) node_rects: HashMap<Uuid, egui::Rect>,
    pub(super) header_rects: HashMap<Uuid, egui::Rect>,
    pub(super) ports: HashMap<ModuleEditorPortId, PortVisual>,
    pub(super) selection_order: Vec<Uuid>,
    pub(super) body_pointer_owned: bool,
}

impl ModuleSurfaceCapture {
    fn record_node(&mut self, node_id: Uuid, rect: egui::Rect) {
        self.node_rects.insert(node_id, rect);
        self.selection_order.retain(|existing| *existing != node_id);
        self.selection_order.push(node_id);
    }

    fn record_response(&mut self, response: &egui::Response) {
        self.body_pointer_owned |= NodeBodyResponse::from_response(response).owns_pointer();
    }

    pub(super) const fn owns_pointer(&self, _pointer: Option<egui::Pos2>) -> bool {
        self.body_pointer_owned
    }
}

pub(super) struct ModuleNodeViewer<'a> {
    pub(super) definition: &'a ModuleDefinition,
    pub(super) palette: &'a library::model::authoring::ProjectPalette,
    pub(super) plugins: &'a PluginManager,
    pub(super) property_context: ModulePropertyContext,
    pub(super) selected_nodes: &'a HashSet<Uuid>,
    pub(super) actions: &'a mut Vec<ModuleEditorAction>,
    pub(super) canvas_transform: egui::emath::TSTransform,
    pub(super) to_global: &'a mut egui::emath::TSTransform,
    pub(super) canvas_clip: &'a mut egui::Rect,
    pub(super) capture: Arc<Mutex<ModuleSurfaceCapture>>,
}

impl ModuleNodeViewer<'_> {
    fn node(&self, snarl: &Snarl<Uuid>, node_id: egui_snarl::NodeId) -> Option<&Node> {
        snarl
            .get_node(node_id)
            .and_then(|id| self.definition.graph.nodes.get(id))
    }

    fn port(
        &self,
        snarl: &Snarl<Uuid>,
        node_id: egui_snarl::NodeId,
        direction: PortDirection,
        index: usize,
    ) -> Option<PortDefinition> {
        let node = self.node(snarl, node_id)?;
        document_port_contract(self.definition, node)
            .ok()?
            .ports
            .into_iter()
            .filter(|port| port.direction == direction)
            .nth(index)
    }

    fn capture_response(&self, response: &egui::Response) {
        if let Ok(mut capture) = self.capture.lock() {
            capture.record_response(response);
        }
    }
}

impl SnarlViewer<Uuid> for ModuleNodeViewer<'_> {
    fn title(&mut self, node_id: &Uuid) -> String {
        self.definition
            .graph
            .nodes
            .get(node_id)
            .map_or_else(|| "Missing Node".to_string(), |node| node.name.clone())
    }

    fn node_layout(
        &mut self,
        _default: NodeLayout,
        _node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        _snarl: &Snarl<Uuid>,
    ) -> NodeLayout {
        NodeLayout::coil()
            .with_min_pin_row_height(PORT_ROW_HEIGHT)
            .with_equal_pin_rows()
    }

    fn node_frame(
        &mut self,
        default: egui::Frame,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<Uuid>,
    ) -> egui::Frame {
        let Some(node) = self.node(snarl, node_id) else {
            return default;
        };
        let visual = Editor::node_visual_style(
            node_palette_for_node(Some(node)),
            !node.enabled,
            self.selected_nodes.contains(&node.id),
            self.to_global.scaling,
        );
        Editor::node_frame(visual)
    }

    fn header_frame(
        &mut self,
        default: egui::Frame,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<Uuid>,
    ) -> egui::Frame {
        let Some(node) = self.node(snarl, node_id) else {
            return default;
        };
        let visual = Editor::node_visual_style(
            node_palette_for_node(Some(node)),
            !node.enabled,
            self.selected_nodes.contains(&node.id),
            self.to_global.scaling,
        );
        Editor::node_header_frame(visual)
    }

    fn show_header(
        &mut self,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<Uuid>,
    ) {
        let Some(node) = self.node(snarl, node_id).cloned() else {
            return;
        };
        let node_id = node.id;
        let is_output = matches!(node.content(), NodeContent::ModuleOutput(_));
        let is_protected = self.definition.is_protected_host_boundary_node(node_id);
        let icon = node_icon_for_node(Some(&node), |_| None);
        let status = if is_protected {
            (icons::LOCK, "Protected host boundary")
        } else if node.bypassed {
            (icons::PAUSE, "Bypassed")
        } else if !node.enabled {
            (icons::EYE_SLASH, "Disabled")
        } else {
            (icons::CHECK, "Enabled")
        };
        let header_width = NODE_HEADER_WIDTH
            .max(node.ui_size[0])
            .max(measured_label_width(ui, &node.name, 0.0) + 48.0);
        let response = Editor::show_node_header(
            ui,
            NodeHeader {
                title: &node.name,
                title_color: None,
                leading: Some(HeaderGlyph {
                    glyph: icon.glyph,
                    tooltip: icon.label,
                }),
                trailing: Some(HeaderGlyph {
                    glyph: status.0,
                    tooltip: status.1,
                }),
                accent: node_palette_for_node(Some(&node)).accent,
                min_width: header_width,
                title_width: header_width - 48.0,
                row_height: PORT_ROW_HEIGHT,
                details_visible: node_editor_details_visible(self.to_global.scaling),
            },
        );
        let graph_rect = response.rect;
        if let Ok(mut capture) = self.capture.lock() {
            capture.header_rects.insert(node_id, graph_rect);
        }
        let screen_rect = (*self.to_global * graph_rect).intersect(*self.canvas_clip);
        crate::qa::register_component_with_metadata(
            format!("node_editor.node_header:{node_id}"),
            "node_header",
            screen_rect,
            true,
            Some(serde_json::json!({
                "document_kind": "module_definition",
                "node_id": node_id,
                "selected": self.selected_nodes.contains(&node_id),
                "module_output": is_output,
                "host_boundary": is_protected,
                "production_surface": "egui_snarl",
            })),
        );
        response.context_menu(|ui| {
            const OUTPUT_STATE_REASON: &str =
                "Module Output is a required render terminal and cannot be disabled or bypassed.";
            const OUTPUT_DELETE_REASON: &str =
                "Module Output is a required render terminal and cannot be deleted.";
            const HOST_BOUNDARY_STATE_REASON: &str =
                "Transition A/B/Progress boundaries are supplied by the Timeline and cannot be disabled or bypassed.";
            const HOST_BOUNDARY_DELETE_REASON: &str =
                "Transition A/B/Progress boundaries are required by the host contract and cannot be deleted.";
            let mut name = node.name.clone();
            if ui.text_edit_singleline(&mut name).changed() {
                self.actions.push(ModuleEditorAction::SetNodeState {
                    node_id,
                    name,
                    enabled: node.enabled,
                    bypassed: node.bypassed,
                });
            }
            let mut enabled = node.enabled;
            let enabled_response = ui.add_enabled(
                !is_output && !is_protected,
                egui::Checkbox::new(&mut enabled, "Enabled"),
            );
            if enabled_response.changed() {
                self.actions.push(ModuleEditorAction::SetNodeState {
                    node_id,
                    name: node.name.clone(),
                    enabled,
                    bypassed: node.bypassed,
                });
            }
            if is_output {
                register_output_control(node_id, "enabled", &enabled_response, OUTPUT_STATE_REASON);
                enabled_response.on_hover_text(OUTPUT_STATE_REASON);
            } else if is_protected {
                register_host_boundary_control(
                    node_id,
                    "enabled",
                    &enabled_response,
                    HOST_BOUNDARY_STATE_REASON,
                );
                enabled_response.on_hover_text(HOST_BOUNDARY_STATE_REASON);
            }
            let mut bypassed = node.bypassed;
            let bypass_response = ui.add_enabled(
                !is_output && !is_protected && node.supports_bypass(),
                egui::Checkbox::new(&mut bypassed, "Bypass"),
            );
            if bypass_response.changed() {
                self.actions.push(ModuleEditorAction::SetNodeState {
                    node_id,
                    name: node.name.clone(),
                    enabled: node.enabled,
                    bypassed,
                });
            }
            if is_output {
                register_output_control(node_id, "bypass", &bypass_response, OUTPUT_STATE_REASON);
                bypass_response.on_hover_text(OUTPUT_STATE_REASON);
            } else if is_protected {
                register_host_boundary_control(
                    node_id,
                    "bypass",
                    &bypass_response,
                    HOST_BOUNDARY_STATE_REASON,
                );
                bypass_response.on_hover_text(HOST_BOUNDARY_STATE_REASON);
            }
            ui.separator();
            let delete_response = ui.add_enabled(
                !is_output && !is_protected,
                egui::Button::new(format!("{} Delete Node", icons::TRASH)),
            );
            if delete_response.clicked() {
                self.actions
                    .push(ModuleEditorAction::DeleteNodes(vec![node_id]));
                ui.close();
            }
            if is_output {
                register_output_control(node_id, "delete", &delete_response, OUTPUT_DELETE_REASON);
                delete_response.on_hover_text(OUTPUT_DELETE_REASON);
            } else if is_protected {
                register_host_boundary_control(
                    node_id,
                    "delete",
                    &delete_response,
                    HOST_BOUNDARY_DELETE_REASON,
                );
                delete_response.on_hover_text(HOST_BOUNDARY_DELETE_REASON);
            }
        });
    }

    fn inputs(&mut self, node_id: &Uuid) -> usize {
        self.definition
            .graph
            .nodes
            .get(node_id)
            .and_then(|node| document_port_contract(self.definition, node).ok())
            .map_or(0, |contract| {
                contract
                    .ports
                    .iter()
                    .filter(|port| port.direction == PortDirection::Input)
                    .count()
            })
    }

    fn outputs(&mut self, node_id: &Uuid) -> usize {
        self.definition
            .graph
            .nodes
            .get(node_id)
            .and_then(|node| document_port_contract(self.definition, node).ok())
            .map_or(0, |contract| {
                contract
                    .ports
                    .iter()
                    .filter(|port| port.direction == PortDirection::Output)
                    .count()
            })
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<Uuid>,
    ) -> impl SnarlPin + 'static {
        // Edit intents are recorded while drawing. Keep a Node snapshot so
        // pushing an intent never overlaps a borrow of the document through
        // `self`; the authoritative definition is mutated after this frame.
        let node = self.node(snarl, pin.id.node).cloned();
        let port = self.port(snarl, pin.id.node, PortDirection::Input, pin.id.input);
        let ownership = node.as_ref().zip(port.as_ref()).map_or(
            ModuleInputPortOwnership::Internal,
            |(node, port)| {
                self.definition.input_port_ownership(&ModulePortAddress {
                    node_id: node.id,
                    port: port.key.clone(),
                })
            },
        );
        let graph_connected = !pin.remotes.is_empty();
        if node_editor_details_visible(self.to_global.scaling) {
            if let (Some(node), Some(port)) = (node.as_ref(), port.as_ref()) {
                let property = input_allows_inline_authoring(ownership)
                    .then(|| authored_property_key_for_port(node, &port.key))
                    .flatten()
                    .and_then(|key| node.properties().get(key).map(|value| (key, value)));
                let interface_response = if ownership.is_externally_driven() {
                    let response = show_externally_driven_input(ui, port, ownership);
                    self.capture_response(&response);
                    response
                } else if let Some((key, property)) = property {
                    let definition = property::node_property_definition(self.plugins, node, key);
                    let (response, action) = property::show_property_input(
                        ui,
                        self.plugins,
                        node,
                        key,
                        property,
                        definition.as_ref(),
                        graph_connected,
                        self.property_context,
                        self.canvas_transform,
                        self.palette,
                    );
                    self.capture_response(&response);
                    if let Some(action) = action {
                        self.actions.push(action);
                    }
                    response
                } else {
                    let label_width = measured_label_width(ui, &port.label, PORT_LABEL_WIDTH);
                    let response = Editor::show_port_label(
                        ui,
                        PortLabel {
                            text: &port.label,
                            width: label_width,
                            row_height: PORT_ROW_HEIGHT,
                            align: egui::Align::LEFT,
                            details_visible: true,
                        },
                    );
                    self.capture_response(&response);
                    response
                };
                self.actions
                    .extend(super::interface::input_port_interface_actions(
                        &interface_response,
                        self.canvas_transform * interface_response.rect,
                        self.definition,
                        node.id,
                        port,
                    ));
            }
        } else {
            ui.allocate_space(egui::vec2(PORT_LABEL_WIDTH + 80.0, PORT_ROW_HEIGHT));
        }
        ModulePin::new(ModulePinRequest {
            node_id: node.as_ref().map(|node| node.id),
            port,
            direction: PortDirection::Input,
            connected: graph_connected || ownership.is_externally_driven(),
            ownership,
            to_global: *self.to_global,
            canvas_clip: *self.canvas_clip,
            capture: Arc::clone(&self.capture),
        })
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<Uuid>,
    ) -> impl SnarlPin + 'static {
        let node = self.node(snarl, pin.id.node);
        let port = self.port(snarl, pin.id.node, PortDirection::Output, pin.id.output);
        if let Some(port) = port.as_ref() {
            let label_width = measured_label_width(ui, &port.label, PORT_LABEL_WIDTH);
            Editor::show_port_label(
                ui,
                PortLabel {
                    text: &port.label,
                    width: label_width,
                    row_height: PORT_ROW_HEIGHT,
                    align: egui::Align::RIGHT,
                    details_visible: node_editor_details_visible(self.to_global.scaling),
                },
            );
        }
        ModulePin::new(ModulePinRequest {
            node_id: node.map(|node| node.id),
            port,
            direction: PortDirection::Output,
            connected: !pin.remotes.is_empty(),
            ownership: ModuleInputPortOwnership::Internal,
            to_global: *self.to_global,
            canvas_clip: *self.canvas_clip,
            capture: Arc::clone(&self.capture),
        })
    }

    fn final_node_rect(
        &mut self,
        node_id: egui_snarl::NodeId,
        rect: egui::Rect,
        _ui: &mut egui::Ui,
        snarl: &mut Snarl<Uuid>,
    ) {
        let Some(node_id) = snarl.get_node(node_id).copied() else {
            return;
        };
        if let Ok(mut capture) = self.capture.lock() {
            capture.record_node(node_id, rect);
        }
        crate::qa::register_component_with_metadata(
            format!("node_editor.node:{node_id}"),
            "node_editor_node",
            (*self.to_global * rect).intersect(*self.canvas_clip),
            true,
            Some(serde_json::json!({
                "document_kind": "module_definition",
                "node_id": node_id,
                "production_surface": "egui_snarl",
            })),
        );
    }

    // Snarl remains the mature production layout/paint implementation. The
    // shared editor owns the complete physical connect lifecycle so an edge
    // reconnect cannot race a second shadow-graph mutation path.
    fn connect(&mut self, _from: &OutPin, _to: &InPin, _snarl: &mut Snarl<Uuid>) {}

    fn disconnect(&mut self, _from: &OutPin, _to: &InPin, _snarl: &mut Snarl<Uuid>) {}

    fn drop_outputs(&mut self, _pin: &OutPin, _snarl: &mut Snarl<Uuid>) {}

    fn drop_inputs(&mut self, _pin: &InPin, _snarl: &mut Snarl<Uuid>) {}

    fn draw_background(
        &mut self,
        _background: Option<&BackgroundPattern>,
        viewport: &egui::Rect,
        _snarl_style: &SnarlStyle,
        _style: &egui::Style,
        painter: &egui::Painter,
        _snarl: &Snarl<Uuid>,
    ) {
        *self.canvas_clip = *self.to_global * painter.clip_rect();
        paint_node_editor_canvas_grid(painter, *viewport, *self.canvas_clip, *self.to_global);
    }

    fn current_transform(
        &mut self,
        to_global: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<Uuid>,
    ) {
        // egui-snarl samples its built-in Scene navigation before this hook.
        // Deliberately discard that proposal: the shared ViewportController
        // has already updated the one authoritative application camera.
        *to_global = self.canvas_transform;
        *self.to_global = self.canvas_transform;
    }
}

pub(super) const fn input_allows_inline_authoring(ownership: ModuleInputPortOwnership) -> bool {
    !ownership.is_externally_driven()
}

fn show_externally_driven_input(
    ui: &mut egui::Ui,
    port: &PortDefinition,
    ownership: ModuleInputPortOwnership,
) -> egui::Response {
    let reason = if ownership.is_host_protected() {
        "This input is supplied by the Timeline host and cannot be wired or authored inside the Module"
    } else {
        "This Published Interface input is supplied by the Module host; unpublish it before wiring or authoring it"
    };
    ui.horizontal(|ui| {
        let label_width = measured_label_width(ui, &port.label, PORT_LABEL_WIDTH);
        Editor::show_port_label(
            ui,
            PortLabel {
                text: &port.label,
                width: label_width,
                row_height: PORT_ROW_HEIGHT,
                align: egui::Align::LEFT,
                details_visible: true,
            },
        );
        ui.weak(icons::LOCK).on_hover_text(reason);
    })
    .response
    .on_hover_text(reason)
}

fn register_output_control(
    node_id: Uuid,
    action: &str,
    response: &egui::Response,
    disabled_reason: &str,
) {
    crate::qa::register_component_with_metadata(
        format!("node_editor.output_control:{node_id}:{action}"),
        "node_editor_output_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "node_id": node_id,
            "action": action,
            "module_output": true,
            "disabled_reason": disabled_reason,
        })),
    );
}

fn register_host_boundary_control(
    node_id: Uuid,
    action: &str,
    response: &egui::Response,
    disabled_reason: &str,
) {
    crate::qa::register_component_with_metadata(
        format!("node_editor.host_boundary_control:{node_id}:{action}"),
        "node_editor_host_boundary_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "node_id": node_id,
            "action": action,
            "host_boundary": true,
            "disabled_reason": disabled_reason,
        })),
    );
}

struct ModulePin {
    info: PinInfo,
    node_id: Option<Uuid>,
    port: Option<PortDefinition>,
    direction: PortDirection,
    connected: bool,
    ownership: ModuleInputPortOwnership,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    capture: Arc<Mutex<ModuleSurfaceCapture>>,
}

struct ModulePinRequest {
    node_id: Option<Uuid>,
    port: Option<PortDefinition>,
    direction: PortDirection,
    connected: bool,
    ownership: ModuleInputPortOwnership,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    capture: Arc<Mutex<ModuleSurfaceCapture>>,
}

impl ModulePin {
    fn new(request: ModulePinRequest) -> Self {
        let ModulePinRequest {
            node_id,
            port,
            direction,
            connected,
            ownership,
            to_global,
            canvas_clip,
            capture,
        } = request;
        let data_type = port
            .as_ref()
            .map_or(PortDataType::Any, |port| port.data_type);
        Self {
            info: pin_info(data_type, connected),
            node_id,
            port,
            direction,
            connected,
            ownership,
            to_global,
            canvas_clip,
            capture,
        }
    }
}

impl SnarlPin for ModulePin {
    fn pin_rect(&self, x: f32, y0: f32, y1: f32, _size: f32) -> egui::Rect {
        let rect = egui::Rect::from_center_size(egui::pos2(x, (y0 + y1) * 0.5), egui::Vec2::ZERO);
        if let (Some(node_id), Some(port)) = (self.node_id, self.port.as_ref()) {
            let id = ModuleEditorPortId {
                address: ModulePortAddress {
                    node_id,
                    port: port.key.clone(),
                },
                direction: self.direction,
            };
            if let Ok(mut capture) = self.capture.lock() {
                capture.ports.insert(
                    id.clone(),
                    PortVisual {
                        id,
                        label: port.label.clone(),
                        center: rect.center(),
                        data_type: port.data_type,
                    },
                );
            }
            let qa_rect = (self.to_global * rect)
                .expand(5.0)
                .intersect(self.canvas_clip);
            let direction = match self.direction {
                PortDirection::Input => "input",
                PortDirection::Output => "output",
            };
            crate::qa::register_component_with_metadata(
                format!("node_editor.port.node:{node_id}.{direction}:{}", port.key),
                "node_editor_port",
                qa_rect,
                !self.ownership.is_externally_driven(),
                Some(serde_json::json!({
                    "document_kind": "module_definition",
                    "node_id": node_id,
                    "port": port.key,
                    "label": port.label,
                    "direction": direction,
                    "data_type": port.data_type,
                    "connected": self.connected,
                    "input_ownership": match self.ownership {
                        ModuleInputPortOwnership::Internal => "internal",
                        ModuleInputPortOwnership::Published => "published",
                        ModuleInputPortOwnership::HostProtected => "host_protected",
                    },
                    "production_surface": "egui_snarl",
                })),
            );
        }
        rect
    }

    fn draw(
        self,
        snarl_style: &SnarlStyle,
        style: &egui::Style,
        rect: egui::Rect,
        painter: &egui::Painter,
    ) -> PinWireInfo {
        let visual_rect = egui::Rect::from_center_size(
            rect.center(),
            egui::Vec2::splat(
                if node_editor_port_interactions_enabled(self.to_global.scaling) {
                    crate::ui::panels::node_editor::PORT_SOCKET_SIZE
                } else {
                    0.0
                },
            ),
        );
        self.info.draw(snarl_style, style, visual_rect, painter)
    }
}
