//! Timeline-first Module document adapter for the reusable Node Editor.
//!
//! This file is intentionally a projection-and-intents boundary. It borrows a
//! `ModuleDefinition`, renders it, and returns edits for the authoritative
//! authoring service to apply. It never inserts Module Nodes into the legacy
//! Project registry.

use std::collections::{HashMap, HashSet};

use eframe::egui;
use library::editor::{ModuleNodeRequest, TimelineEditorService};
use library::model::authoring::{
    AuthoringProject, ModuleConnectionId, ModuleDefinition, ModuleInstanceId,
    ModuleNodePortContract, ModulePortAddress,
};
use library::model::frame::color::Color;
use library::model::project::{PortDataType, PortDirection};
use library::model::property::{Property, PropertyDefinition, PropertyUiType, PropertyValue};
use library::model::{GeneratorContent, NativeNodeFactory, Node, native_node_descriptor};
use library::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY,
    EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, IMAGE_TRANSFORM_COMPONENT_ID,
    PATH_EFFECT_APPLY_OPERATION, PATH_EFFECT_CATEGORY, PluginManager, SHAPE_TRANSFORM_COMPONENT_ID,
    STYLE_APPLY_OPERATION, STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
};
use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, Editor, EditorConfig, EditorOutput, GraphFrame, ItemId,
    MoveEndOutcome, NodeBodyRenderer, NodeBodyResponse, NodeDescriptor, PortDescriptor,
    PortDirection as SurfacePortDirection, PortOwner, TypeKey, WireDescriptor,
};
use pan_zoom_ui::{CanvasState, NavigationConfig, ZoomPolicy};
use uuid::Uuid;

use crate::state::authoring::AuthoringUiState;
use crate::state::context_types::{
    ModuleEditorHost, ModuleEditorPortId, NodeEditorDocument, NodeEditorState,
};
use crate::ui::widgets::color_value_picker::color_value_picker;
use crate::ui::widgets::property_drag_value::{FloatDragValueConfig, IntegerDragValueConfig};
use crate::ui::widgets::searchable_context_menu::{
    register_searchable_popup_qa, searchable_menu_click_is_outside, searchable_popup_placement,
    show_searchable_items_with_qa, show_searchable_popup_frame,
};

use super::commands::{NodeCreateRequest, node_create_menu_items};
use crate::ui::viewport::{ViewportController, ViewportState};

const HEADER_HEIGHT: f32 = 30.0;
const MIN_NODE_WIDTH: f32 = 180.0;
const MIN_NODE_HEIGHT: f32 = 92.0;
const PORT_ROW_HEIGHT: f32 = 24.0;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ModuleEditorAction {
    MoveNodes {
        node_ids: Vec<Uuid>,
        delta: egui::Vec2,
    },
    FinishMove {
        outcome: MoveEndOutcome,
    },
    Connect {
        from: ModulePortAddress,
        to: ModulePortAddress,
    },
    Disconnect(ModuleConnectionId),
    DeleteNodes(Vec<Uuid>),
    DeleteConnections(Vec<ModuleConnectionId>),
    SetNodeState {
        node_id: Uuid,
        name: String,
        enabled: bool,
        bypassed: bool,
    },
    SetNodeProperty {
        node_id: Uuid,
        key: String,
        property: Property,
    },
    CreateNode {
        request: NodeCreateRequest,
        graph_position: egui::Pos2,
    },
}

struct ModuleViewportState<'a> {
    pan: &'a mut egui::Vec2,
    zoom: &'a mut f32,
}

impl ViewportState for ModuleViewportState<'_> {
    fn canvas_state(&self) -> CanvasState {
        CanvasState::uniform(*self.pan, normalized_zoom(*self.zoom))
    }

    fn set_canvas_state(&mut self, state: CanvasState) {
        *self.pan = state.pan;
        *self.zoom = state.zoom.x;
    }
}

fn normalized_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
    }
}

fn navigation_config() -> NavigationConfig {
    NavigationConfig {
        zoom_policy: ZoomPolicy::Uniform,
        min_zoom: egui::Vec2::splat(0.02),
        max_zoom: egui::Vec2::splat(1.25),
        max_pan: egui::Vec2::splat(10_000_000.0),
        ..NavigationConfig::default()
    }
}

#[derive(Clone)]
struct PortVisual {
    id: ModuleEditorPortId,
    label: String,
    center: egui::Pos2,
    data_type: PortDataType,
}

struct ModuleBodyRenderer<'a> {
    nodes: &'a HashMap<Uuid, Node>,
    connected_inputs: &'a HashSet<ModulePortAddress>,
    plugin_manager: &'a PluginManager,
    property_time: f64,
    actions: &'a mut Vec<ModuleEditorAction>,
}

impl NodeBodyRenderer<Uuid> for ModuleBodyRenderer<'_> {
    fn show(&mut self, node_id: &Uuid, ui: &mut egui::Ui) -> NodeBodyResponse {
        let Some(node) = self.nodes.get(node_id) else {
            return NodeBodyResponse::NONE;
        };
        let mut ownership = NodeBodyResponse::NONE;
        ui.horizontal(|ui| {
            let mut enabled = node.enabled;
            let enabled_response = ui.checkbox(&mut enabled, "Enabled");
            ownership = ownership.union(NodeBodyResponse::from_response(&enabled_response));
            let mut bypassed = node.bypassed;
            let bypass_response = ui.add_enabled(
                node.supports_bypass(),
                egui::Checkbox::new(&mut bypassed, "Bypass"),
            );
            ownership = ownership.union(NodeBodyResponse::from_response(&bypass_response));
            if enabled_response.changed() || bypass_response.changed() {
                self.actions.push(ModuleEditorAction::SetNodeState {
                    node_id: *node_id,
                    name: node.name.clone(),
                    enabled,
                    bypassed,
                });
            }
        });

        let mut properties = node.properties().iter().collect::<Vec<_>>();
        properties.sort_by(|left, right| left.0.cmp(right.0));
        for (key, property) in properties {
            let definition =
                super::queries::node_property_definition(Some(self.plugin_manager), node, key);
            let connected = self.connected_inputs.contains(&ModulePortAddress {
                node_id: *node_id,
                port: key.clone(),
            });
            let (response, edited_value) =
                show_property_control(ui, *node_id, key, property, definition.as_ref(), connected);
            ownership = ownership.union(NodeBodyResponse::from_response(&response));
            if let Some(value) = edited_value {
                self.actions.push(ModuleEditorAction::SetNodeProperty {
                    node_id: *node_id,
                    key: key.clone(),
                    property: property_with_edited_value(property, value, self.property_time),
                });
            }
        }
        ownership
    }
}

fn property_with_edited_value(
    property: &Property,
    value: PropertyValue,
    property_time: f64,
) -> Property {
    let mut replacement = property.clone();
    match replacement.evaluator.as_str() {
        "constant" => Property::constant(value),
        "keyframe" => {
            let _ = replacement.upsert_keyframe(property_time, value, None);
            replacement
        }
        _ => {
            replacement.properties.insert("value".to_string(), value);
            replacement
        }
    }
}

fn show_property_control(
    ui: &mut egui::Ui,
    node_id: Uuid,
    key: &str,
    property: &Property,
    definition: Option<&PropertyDefinition>,
    connected: bool,
) -> (egui::Response, Option<PropertyValue>) {
    let label = definition.map_or(key, PropertyDefinition::label);
    let mut edited = property.value().cloned();
    let row = ui.horizontal(|ui| {
        ui.add_sized(
            [70.0, PORT_ROW_HEIGHT],
            egui::Label::new(label).truncate().selectable(false),
        );
        if connected {
            return (ui.weak("Connected"), None);
        }
        let Some(value) = edited.as_mut() else {
            return (ui.weak("No value"), None);
        };
        let response = match value {
            PropertyValue::Number(number) => {
                if let Some(config) = definition.and_then(FloatDragValueConfig::from_definition) {
                    ui.add_sized([96.0, PORT_ROW_HEIGHT], config.widget(&mut number.0))
                } else {
                    ui.add_sized(
                        [96.0, PORT_ROW_HEIGHT],
                        egui::DragValue::new(&mut number.0).speed(0.05),
                    )
                }
            }
            PropertyValue::Integer(integer) => {
                if let Some(config) = definition.and_then(|definition| {
                    IntegerDragValueConfig::from_ui_type(definition.ui_type())
                }) {
                    ui.add_sized([96.0, PORT_ROW_HEIGHT], config.widget(integer))
                } else {
                    ui.add_sized([96.0, PORT_ROW_HEIGHT], egui::DragValue::new(integer))
                }
            }
            PropertyValue::String(text) => {
                if let Some(PropertyUiType::Dropdown { options }) =
                    definition.map(PropertyDefinition::ui_type)
                {
                    egui::ComboBox::from_id_salt(("module_property", node_id, key))
                        .selected_text(text.as_str())
                        .width(104.0)
                        .show_ui(ui, |ui| {
                            for option in options {
                                ui.selectable_value(text, option.clone(), option);
                            }
                        })
                        .response
                } else {
                    ui.add_sized(
                        [116.0, PORT_ROW_HEIGHT],
                        egui::TextEdit::singleline(text).clip_text(true),
                    )
                }
            }
            PropertyValue::Boolean(boolean) => ui.checkbox(boolean, ""),
            PropertyValue::Color(color) => {
                let mut display =
                    egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
                let response = ui.color_edit_button_srgba(&mut display);
                if response.changed() {
                    color.r = display.r();
                    color.g = display.g();
                    color.b = display.b();
                    color.a = display.a();
                }
                response
            }
            PropertyValue::ColorValue(color) => {
                let picker =
                    color_value_picker(ui, egui::Id::new(("module_color", node_id, key)), color);
                if let Some(value) = picker.value {
                    *color = value;
                }
                picker.response
            }
            PropertyValue::Vec2(value) => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut value.x.0)
                            .prefix("x ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.y.0)
                            .prefix("y ")
                            .speed(0.05),
                    )
                })
                .response
            }
            PropertyValue::Vec3(value) => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut value.x.0)
                            .prefix("x ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.y.0)
                            .prefix("y ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.z.0)
                            .prefix("z ")
                            .speed(0.05),
                    )
                })
                .response
            }
            PropertyValue::Vec4(value) => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut value.x.0)
                            .prefix("x ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.y.0)
                            .prefix("y ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.z.0)
                            .prefix("z ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut value.w.0)
                            .prefix("w ")
                            .speed(0.05),
                    )
                })
                .response
            }
            PropertyValue::Path(_) => ui.weak("Path"),
            PropertyValue::Array(values) => ui.weak(format!("{} items", values.len())),
            PropertyValue::Map(values) => ui.weak(format!("{} fields", values.len())),
            PropertyValue::OpaqueJson(_) => ui.weak("Unsupported value"),
        };
        let value = response.changed().then(|| value.clone());
        (response, value)
    });
    let (response, changed) = row.inner;
    crate::qa::register_component_with_metadata(
        format!("node_editor.property.node:{node_id}:{key}"),
        "node_property_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "document_kind": "module_definition",
            "node_id": node_id,
            "property": key,
            "connected": connected,
            "evaluator": property.evaluator,
            "descriptor_available": definition.is_some(),
        })),
    );
    (response, changed)
}

/// Render one bounded Module graph and return only model mutation intents.
pub(super) fn show_module_document(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    state: &mut NodeEditorState,
    plugin_manager: &PluginManager,
    property_time: f64,
) -> Vec<ModuleEditorAction> {
    let viewport = ui.available_rect_before_wrap();
    if !viewport.is_positive() {
        return Vec::new();
    }

    let locked_transform = state.surface_interaction.locked_transform();
    if locked_transform.is_none() {
        let mut handled_pan = false;
        let mut viewport_state = ModuleViewportState {
            pan: &mut state.module_canvas_pan,
            zoom: &mut state.module_canvas_zoom,
        };
        let mut controller =
            ViewportController::new(ui, ui.id().with("module_graph_viewport"), None)
                .with_config(navigation_config())
                .with_screen_origin(viewport.min);
        let _ =
            controller.interact_with_rect(viewport, &mut viewport_state, &mut handled_pan);
    }

    let transform = locked_transform.unwrap_or_else(|| {
        egui::emath::TSTransform::new(
            viewport.min.to_vec2() + state.module_canvas_pan,
            normalized_zoom(state.module_canvas_zoom),
        )
    });

    let mut nodes = definition.graph.nodes.values().collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.id);
    let mut node_rects = HashMap::with_capacity(nodes.len());
    for node in &nodes {
        let contract = ModuleNodePortContract::resolve(node).ok();
        let input_count = contract.as_ref().map_or(0, |contract| {
            contract
                .ports
                .iter()
                .filter(|port| port.direction == PortDirection::Input)
                .count()
        });
        let output_count = contract.as_ref().map_or(0, |contract| {
            contract
                .ports
                .iter()
                .filter(|port| port.direction == PortDirection::Output)
                .count()
        });
        let row_count = input_count.max(output_count).max(1) as f32;
        let size = egui::vec2(
            node.ui_size[0].max(MIN_NODE_WIDTH),
            node.ui_size[1]
                .max(MIN_NODE_HEIGHT)
                .max(HEADER_HEIGHT + 12.0 + row_count * PORT_ROW_HEIGHT),
        );
        node_rects.insert(
            node.id,
            egui::Rect::from_min_size(
                egui::pos2(node.ui_position[0], node.ui_position[1])
                    + state
                        .module_node_drag_offsets
                        .get(&node.id)
                        .copied()
                        .unwrap_or(egui::Vec2::ZERO),
                egui::vec2(
                    size.x.max(260.0),
                    size.y.max(
                        HEADER_HEIGHT
                            + 42.0
                            + node.properties().iter().count() as f32 * PORT_ROW_HEIGHT,
                    ),
                ),
            ),
        );
    }

    let node_descriptors = nodes
        .iter()
        .map(|node| {
            let rect = node_rects[&node.id];
            NodeDescriptor {
                id: node.id,
                title: node.name.as_str(),
                rect,
                header_rect: egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width(), HEADER_HEIGHT),
                ),
                parent: None,
                enabled: node.enabled,
            }
        })
        .collect::<Vec<_>>();

    let mut diagnostics = Vec::new();
    let mut port_visuals = Vec::new();
    for node in &nodes {
        let contract = match ModuleNodePortContract::resolve(node) {
            Ok(contract) => contract,
            Err(error) => {
                diagnostics.push(error);
                continue;
            }
        };
        let rect = node_rects[&node.id];
        let mut input_index = 0;
        let mut output_index = 0;
        for port in contract.ports {
            let (x, row) = match port.direction {
                PortDirection::Input => {
                    let row = input_index;
                    input_index += 1;
                    (rect.left(), row)
                }
                PortDirection::Output => {
                    let row = output_index;
                    output_index += 1;
                    (rect.right(), row)
                }
            };
            let center = egui::pos2(
                x,
                rect.top() + HEADER_HEIGHT + 12.0 + row as f32 * PORT_ROW_HEIGHT,
            );
            port_visuals.push(PortVisual {
                id: ModuleEditorPortId {
                    address: ModulePortAddress {
                        node_id: node.id,
                        port: port.key,
                    },
                    direction: port.direction,
                },
                label: port.label,
                center,
                data_type: port.data_type,
            });
        }
    }

    let port_descriptors = port_visuals
        .iter()
        .map(|port| PortDescriptor {
            id: port.id.clone(),
            owner: PortOwner::Node(port.id.address.node_id),
            label: port.label.as_str(),
            center: port.center,
            direction: match port.id.direction {
                PortDirection::Input => SurfacePortDirection::Input,
                PortDirection::Output => SurfacePortDirection::Output,
            },
            type_key: TypeKey::new(port.data_type),
            // The model owns compatibility validation. `Any` is a legitimate
            // polymorphic port, not a disabled socket.
            connectable: true,
        })
        .collect::<Vec<_>>();
    let port_centers = port_visuals
        .iter()
        .map(|port| (port.id.clone(), port.center))
        .collect::<HashMap<_, _>>();

    let wires = definition
        .graph
        .connections
        .iter()
        .filter_map(|connection| {
            let from_id = ModuleEditorPortId {
                address: connection.from.clone(),
                direction: PortDirection::Output,
            };
            let to_id = ModuleEditorPortId {
                address: connection.to.clone(),
                direction: PortDirection::Input,
            };
            let from = *port_centers.get(&from_id)?;
            let to = *port_centers.get(&to_id)?;
            let handle = ((to.x - from.x).abs() * 0.5).max(48.0);
            Some(WireDescriptor {
                id: connection.id,
                from: from_id,
                to: to_id,
                curve: CubicBezier::new(
                    from,
                    from + egui::vec2(handle, 0.0),
                    to - egui::vec2(handle, 0.0),
                    to,
                ),
                editable: true,
            })
        })
        .collect::<Vec<_>>();

    let mut selected = state
        .module_selected_nodes
        .iter()
        .copied()
        .map(ItemId::Node)
        .collect::<Vec<_>>();
    if let Some(connection) = state.module_selected_connection {
        selected.push(ItemId::Wire(connection));
    }
    let primary = state
        .module_primary_node
        .map(ItemId::Node)
        .or_else(|| state.module_selected_connection.map(ItemId::Wire));
    let selection_order = node_descriptors
        .iter()
        .map(|node| ItemId::Node(node.id))
        .collect::<Vec<_>>();
    let frame = GraphFrame {
        viewport,
        transform,
        nodes: &node_descriptors,
        ports: &port_descriptors,
        wires: &wires,
        groups: &[],
        selection_order: &selection_order,
        selection: AuthoritativeSelection {
            items: &selected,
            primary,
        },
    };

    crate::qa::register_component_with_metadata(
        "node_editor.canvas",
        "node_editor_canvas",
        viewport,
        true,
        Some(serde_json::json!({
            "document_kind": "module_definition",
            "module_definition_id": definition.id,
            "module_node_count": definition.graph.nodes.len(),
            "module_connection_count": definition.graph.connections.len(),
            "scale": transform.scaling,
            "translation": {
                "x": state.module_canvas_pan.x,
                "y": state.module_canvas_pan.y,
            },
            "diagnostics": diagnostics,
        })),
    );
    for node in &node_descriptors {
        crate::qa::register_component_with_metadata(
            format!("node_editor.node:{}", node.id),
            "node_editor_node",
            frame.screen_rect(node.rect).intersect(viewport),
            true,
            Some(serde_json::json!({
                "document_kind": "module_definition",
                "module_definition_id": definition.id,
                "node_id": node.id,
            })),
        );
        crate::qa::register_component_with_metadata(
            format!("node_editor.node_header:{}", node.id),
            "node_editor_node_header",
            frame.screen_rect(node.header_rect).intersect(viewport),
            true,
            Some(serde_json::json!({
                "document_kind": "module_definition",
                "module_definition_id": definition.id,
                "node_id": node.id,
            })),
        );
    }
    for port in &port_visuals {
        let center = frame.screen_position(port.center);
        let direction = match port.id.direction {
            PortDirection::Input => "input",
            PortDirection::Output => "output",
        };
        crate::qa::register_component_with_metadata(
            format!(
                "node_editor.port.node:{}.{direction}:{}",
                port.id.address.node_id, port.id.address.port
            ),
            "node_editor_port",
            egui::Rect::from_center_size(center, egui::Vec2::splat(14.0)).intersect(viewport),
            true,
            Some(serde_json::json!({
                "document_kind": "module_definition",
                "module_definition_id": definition.id,
                "node_id": port.id.address.node_id,
                "port": port.id.address.port,
                "direction": direction,
                "data_type": port.data_type,
            })),
        );
    }

    let connected_inputs = definition
        .graph
        .connections
        .iter()
        .map(|connection| connection.to.clone())
        .collect::<HashSet<_>>();
    let mut actions = Vec::new();
    let mut body = ModuleBodyRenderer {
        nodes: &definition.graph.nodes,
        connected_inputs: &connected_inputs,
        plugin_manager,
        property_time,
        actions: &mut actions,
    };
    let outputs = Editor::show(
        ui,
        &frame,
        &mut state.module_surface_interaction,
        &mut body,
        EditorConfig::default(),
    );
    actions.extend(translate_outputs(outputs, state));
    if let Some((request, graph_position)) = show_module_create_menu(
        ui,
        state,
        plugin_manager,
        viewport,
        transform,
        node_rects.values().copied().collect(),
    ) {
        actions.push(ModuleEditorAction::CreateNode {
            request,
            graph_position,
        });
    }
    actions
}

fn show_module_create_menu(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    plugin_manager: &PluginManager,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
    node_rects: Vec<egui::Rect>,
) -> Option<(NodeCreateRequest, egui::Pos2)> {
    let (secondary_clicked, pointer_position, open_time) = ui.input(|input| {
        (
            input.pointer.secondary_clicked(),
            input.pointer.interact_pos(),
            input.time,
        )
    });
    super::authoring::update_global_context_menu_for_secondary_click(
        &mut state.module_create_menu,
        secondary_clicked,
        pointer_position,
        viewport,
        &node_rects,
        transform,
        open_time,
    );

    let mut selected = None;
    let mut should_close = false;
    if let Some(context) = state.module_create_menu.as_ref() {
        let position = context.position;
        let graph_position = transform.inverse() * position;
        let popup =
            searchable_popup_placement(position, egui::vec2(320.0, 348.0), ui.ctx().content_rect());
        let menu_id = format!(
            "module_node_editor_add_menu:{}",
            context.open_time.to_bits()
        );
        let response = egui::Area::new(egui::Id::new("module_node_editor_context_menu"))
            .order(egui::Order::Foreground)
            .pivot(popup.pivot)
            .fixed_pos(popup.area_anchor)
            .constrain(false)
            .show(ui.ctx(), |ui| {
                show_searchable_popup_frame(ui, popup, |ui| {
                    let items = node_create_menu_items(plugin_manager)
                        .into_iter()
                        .filter(|item| {
                            !matches!(
                                item.value,
                                NodeCreateRequest::Clip
                                    | NodeCreateRequest::Track
                                    | NodeCreateRequest::Composition
                            )
                        })
                        .collect::<Vec<_>>();
                    if let Some(request) = show_searchable_items_with_qa(
                        ui,
                        &menu_id,
                        Some("node_editor.menu.search"),
                        &items,
                    ) {
                        selected = Some((request, graph_position));
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
    if should_close {
        state.module_create_menu = None;
    }
    selected
}

mod host;
pub use host::module_node_editor_panel;
fn translate_outputs(
    outputs: Vec<EditorOutput<Uuid, ModuleEditorPortId, ModuleConnectionId, Uuid>>,
    state: &mut NodeEditorState,
) -> Vec<ModuleEditorAction> {
    let mut actions = Vec::new();
    for output in outputs {
        match output {
            EditorOutput::Select { items, primary } => {
                state.module_selected_nodes = items
                    .iter()
                    .filter_map(|item| match item {
                        ItemId::Node(node_id) => Some(*node_id),
                        ItemId::Group(_) | ItemId::Wire(_) => None,
                    })
                    .collect();
                state.module_selected_connection = items.iter().find_map(|item| match item {
                    ItemId::Wire(connection_id) => Some(*connection_id),
                    ItemId::Node(_) | ItemId::Group(_) => None,
                });
                state.module_primary_node = match primary {
                    Some(ItemId::Node(node_id)) => Some(node_id),
                    Some(ItemId::Wire(connection_id)) => {
                        state.module_selected_connection = Some(connection_id);
                        None
                    }
                    Some(ItemId::Group(_)) | None => None,
                };
            }
            EditorOutput::Move { items, delta, .. } => {
                let mut node_ids = items
                    .into_iter()
                    .filter_map(|item| match item {
                        ItemId::Node(node_id) => Some(node_id),
                        ItemId::Group(_) | ItemId::Wire(_) => None,
                    })
                    .collect::<Vec<_>>();
                node_ids.sort_unstable();
                node_ids.dedup();
                if !node_ids.is_empty() && delta != egui::Vec2::ZERO {
                    actions.push(ModuleEditorAction::MoveNodes { node_ids, delta });
                }
            }
            EditorOutput::MoveEnd { outcome } => {
                actions.push(ModuleEditorAction::FinishMove { outcome });
            }
            EditorOutput::Connect { from, to }
                if from.direction == PortDirection::Output
                    && to.direction == PortDirection::Input =>
            {
                actions.push(ModuleEditorAction::Connect {
                    from: from.address,
                    to: to.address,
                });
            }
            EditorOutput::Connect { .. } => {}
            EditorOutput::Disconnect { wire } => {
                actions.push(ModuleEditorAction::Disconnect(wire));
            }
            EditorOutput::DeselectWire { wire } => {
                if state.module_selected_connection == Some(wire) {
                    state.module_selected_connection = None;
                }
            }
            EditorOutput::Delete { items } => {
                let mut nodes = Vec::new();
                let mut connections = Vec::new();
                for item in items {
                    match item {
                        ItemId::Node(node_id) => nodes.push(node_id),
                        ItemId::Wire(connection_id) => connections.push(connection_id),
                        ItemId::Group(_) => {}
                    }
                }
                if !connections.is_empty() {
                    actions.push(ModuleEditorAction::DeleteConnections(connections));
                }
                if !nodes.is_empty() {
                    actions.push(ModuleEditorAction::DeleteNodes(nodes));
                }
            }
            EditorOutput::LayoutSwipe(_)
            | EditorOutput::Reparent { .. }
            | EditorOutput::ResizeGroup { .. } => {}
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::animation::EasingFunction;
    use library::model::Node;
    use library::model::authoring::{
        ModuleDefinitionId, ModuleDefinitionSharing, ModuleGraph, ModuleInterface,
        ModuleTemplateOrigin,
    };

    #[test]
    fn document_projection_never_adds_timeline_container_groups() {
        let node = Node::new_merge("Module Merge");
        let definition = ModuleDefinition {
            id: ModuleDefinitionId::new(),
            name: "Module".to_string(),
            sharing: ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
            graph: ModuleGraph {
                nodes: HashMap::from([(node.id, node)]),
                connections: Vec::new(),
            },
            interface: ModuleInterface::default(),
            topology_revision: 1,
            interface_version: 1,
        };
        let context = egui::Context::default();
        let mut state = NodeEditorState::default();
        state.module_canvas_zoom = 1.0;
        let plugins = PluginManager::default();
        let actions = std::cell::RefCell::new(Vec::new());
        drop(context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                *actions.borrow_mut() =
                    show_module_document(ui, &definition, &mut state, &plugins, 0.0);
            });
        }));
        assert!(actions.into_inner().is_empty());
        assert!(state.module_selected_nodes.is_empty());
    }

    #[test]
    fn property_edits_preserve_the_authored_evaluator_mode() {
        let constant = Property::constant(PropertyValue::Integer(1));
        let edited = property_with_edited_value(&constant, PropertyValue::Integer(2), 3.0);
        assert_eq!(edited.evaluator, "constant");
        assert_eq!(edited.value(), Some(&PropertyValue::Integer(2)));

        let keyframed = Property::keyframe(vec![library::model::property::Keyframe::new(
            0.0,
            PropertyValue::Integer(1),
            EasingFunction::Linear,
        )]);
        let edited = property_with_edited_value(&keyframed, PropertyValue::Integer(7), 3.0);
        assert_eq!(edited.evaluator, "keyframe");
        assert_eq!(edited.keyframes().len(), 2);
        assert!(edited.keyframes().iter().any(|keyframe| {
            keyframe.time.into_inner() == 3.0 && keyframe.value == PropertyValue::Integer(7)
        }));

        let expression = Property::expression("x * 2".to_string(), PropertyValue::Integer(1));
        let edited = property_with_edited_value(&expression, PropertyValue::Integer(9), 3.0);
        assert_eq!(edited.evaluator, "expression");
        assert_eq!(edited.expression_text(), Some("x * 2"));
        assert_eq!(edited.value(), Some(&PropertyValue::Integer(9)));
    }

    #[test]
    fn delete_disconnects_explicit_wires_before_removing_nodes() {
        let node_id = Uuid::new_v4();
        let connection_id = ModuleConnectionId::new();
        let mut state = NodeEditorState::default();
        let actions = translate_outputs(
            vec![EditorOutput::Delete {
                items: vec![ItemId::Node(node_id), ItemId::Wire(connection_id)],
            }],
            &mut state,
        );
        assert!(matches!(
            actions.as_slice(),
            [
                ModuleEditorAction::DeleteConnections(connections),
                ModuleEditorAction::DeleteNodes(nodes)
            ] if connections == &[connection_id] && nodes == &[node_id]
        ));
    }
}
