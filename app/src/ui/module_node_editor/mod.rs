//! Timeline-first Module document adapter for the reusable Node Editor.
//!
//! This file is intentionally a projection-and-intents boundary. It borrows a
//! `ModuleDefinition`, renders it, and returns edits for the authoritative
//! authoring service to apply. It never inserts Module Nodes into the legacy
//! Project registry.

use std::collections::{HashMap, HashSet};

use eframe::egui;
use egui_phosphor::regular as icons;
use library::editor::{ModuleInterfaceCommand, ModuleNodeRequest, TimelineEditorService};
use library::model::authoring::{
    AttachmentProcessor, AuthoringProject, ModuleConnectionId, ModuleDefinition, ModuleInstanceId,
    ModuleInvocation, ModuleNodePortContract, ModulePortAddress, PublishedMediaOutputId, SourceRef,
};
use library::model::frame::color::Color;
use library::model::project::{PortDataType, PortDirection};
use library::model::property::{
    Property, PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue,
};
use library::model::{
    native_node_descriptor, GeneratorContent, NativeNodeFactory, Node, NodeContent,
};
use library::plugin::PluginManager;
use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, Editor, EditorConfig, EditorOutput, GraphFrame, ItemId,
    MoveEndOutcome, NodeBodyRenderer, NodeBodyResponse, NodeDescriptor, PortDescriptor,
    PortDirection as SurfacePortDirection, PortOwner, TypeKey, WireDescriptor,
};
use pan_zoom_ui::{CanvasState, NavigationConfig, ZoomPolicy};
use uuid::Uuid;

use crate::state::authoring::AuthoringUiState;
use crate::state::module_node_editor::{
    ModuleCreateMenuState, ModuleEditorHost, ModuleEditorPortId, ModuleNodeEditorDocument,
    ModuleNodeEditorState,
};
use crate::ui::widgets::color_value_picker::color_value_picker;
use crate::ui::widgets::property_drag_value::{FloatDragValueConfig, IntegerDragValueConfig};
use crate::ui::widgets::searchable_context_menu::{
    register_searchable_popup_qa, searchable_menu_click_is_outside, searchable_popup_placement,
    show_searchable_items_with_qa, show_searchable_popup_frame,
};

use crate::ui::viewport::{ViewportController, ViewportState};

mod menu;
use menu::{module_node_menu_items, ModuleNodeCreateRequest};

const HEADER_HEIGHT: f32 = 30.0;
const MIN_NODE_WIDTH: f32 = 180.0;
const MIN_NODE_HEIGHT: f32 = 92.0;
const PORT_ROW_HEIGHT: f32 = 24.0;
const MIN_CANVAS_ZOOM: f32 = 0.02;
const MAX_CANVAS_ZOOM: f32 = 1.25;
const DETAIL_CANVAS_ZOOM: f32 = 0.18;
const BODY_INPUT_GUTTER: f32 = 60.0;

#[derive(Clone, Debug, PartialEq)]
enum ModuleEditorAction {
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
        request: ModuleNodeCreateRequest,
        graph_position: egui::Pos2,
    },
    EditInterface(ModuleInterfaceCommand),
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
        zoom.clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM)
    } else {
        1.0
    }
}

fn navigation_config() -> NavigationConfig {
    NavigationConfig {
        zoom_policy: ZoomPolicy::Uniform,
        min_zoom: egui::Vec2::splat(MIN_CANVAS_ZOOM),
        max_zoom: egui::Vec2::splat(MAX_CANVAS_ZOOM),
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

fn authored_property_key_for_port<'a>(
    properties: &PropertyMap,
    port_key: &'a str,
) -> Option<&'a str> {
    let property_key = port_key.strip_prefix("property:").unwrap_or(port_key);
    properties
        .get(property_key)
        .is_some()
        .then_some(property_key)
}

mod property;
use property::ModuleBodyRenderer;

mod layout;

#[cfg(test)]
mod interaction_tests;

/// Render one bounded Module graph and return only model mutation intents.
fn show_module_document(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    active_output_id: Option<PublishedMediaOutputId>,
    state: &mut ModuleNodeEditorState,
    plugin_manager: &PluginManager,
    property_time: f64,
) -> Vec<ModuleEditorAction> {
    let viewport = ui.available_rect_before_wrap();
    if !viewport.is_positive() {
        return Vec::new();
    }

    // Direct-manipulation gestures are expressed in the transform captured on
    // pointer press. Keep navigation frozen until release so a wheel/pinch
    // cannot move a port or node out from underneath the active gesture.
    let locked_transform = state.surface_interaction.locked_transform();
    if locked_transform.is_none() {
        let mut handled_pan = false;
        let mut viewport_state = ModuleViewportState {
            pan: &mut state.canvas_pan,
            zoom: &mut state.canvas_zoom,
        };
        let mut controller =
            ViewportController::new(ui, ui.id().with("module_graph_viewport"), None)
                .with_config(navigation_config())
                .with_screen_origin(viewport.min);
        let _ = controller.interact_with_rect(viewport, &mut viewport_state, &mut handled_pan);
    }

    let transform = locked_transform.unwrap_or_else(|| {
        egui::emath::TSTransform::new(
            viewport.min.to_vec2() + state.canvas_pan,
            normalized_zoom(state.canvas_zoom),
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
                        .node_drag_offsets
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
        .map(|port| {
            let property_is_labeled_by_body = port.id.direction == PortDirection::Input
                && definition
                    .graph
                    .nodes
                    .get(&port.id.address.node_id)
                    .is_some_and(|node| {
                        authored_property_key_for_port(node.properties(), &port.id.address.port)
                            .is_some()
                    });
            PortDescriptor {
                id: port.id.clone(),
                owner: PortOwner::Node(port.id.address.node_id),
                // Property controls already carry their human-readable label.
                // Keep generic labels for media/time sockets, but do not paint
                // the property name twice on top of its editor.
                label: if property_is_labeled_by_body {
                    ""
                } else {
                    port.label.as_str()
                },
                center: port.center,
                direction: match port.id.direction {
                    PortDirection::Input => SurfacePortDirection::Input,
                    PortDirection::Output => SurfacePortDirection::Output,
                },
                type_key: TypeKey::new(port.data_type),
                // The model owns compatibility validation. `Any` is a legitimate
                // polymorphic port, not a disabled socket.
                connectable: true,
            }
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
        .selected_nodes
        .iter()
        .copied()
        .map(ItemId::Node)
        .collect::<Vec<_>>();
    if let Some(connection) = state.selected_connection {
        selected.push(ItemId::Wire(connection));
    }
    let primary = state
        .primary_node
        .map(ItemId::Node)
        .or_else(|| state.selected_connection.map(ItemId::Wire));
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
            "min_scale": MIN_CANVAS_ZOOM,
            "max_scale": MAX_CANVAS_ZOOM,
            "detail_enabled": transform.scaling >= DETAIL_CANVAS_ZOOM,
            "translation": {
                "x": state.canvas_pan.x,
                "y": state.canvas_pan.y,
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
        &mut state.surface_interaction,
        &mut body,
        EditorConfig {
            details_min_scale: DETAIL_CANVAS_ZOOM,
            ..EditorConfig::default()
        },
    );
    actions.extend(translate_outputs(outputs, state));
    actions.extend(port_interface_actions(
        ui,
        definition,
        active_output_id,
        &port_visuals,
        transform,
        viewport,
    ));
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

fn port_interface_actions(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    active_output_id: Option<PublishedMediaOutputId>,
    ports: &[PortVisual],
    transform: egui::emath::TSTransform,
    viewport: egui::Rect,
) -> Vec<ModuleEditorAction> {
    let mut actions = Vec::new();
    for port in ports {
        let center = transform * port.center;
        let label_center = match port.id.direction {
            PortDirection::Input => center + egui::vec2(54.0, 0.0),
            PortDirection::Output => center - egui::vec2(54.0, 0.0),
        };
        let rect =
            egui::Rect::from_center_size(label_center, egui::vec2(100.0, 20.0)).intersect(viewport);
        let response = ui.interact(
            rect,
            ui.id().with((
                "module-port-interface-menu",
                port.id.address.node_id,
                port.id.direction,
                port.id.address.port.as_str(),
            )),
            egui::Sense::click(),
        );
        response.context_menu(|ui| {
            ui.strong(&port.label);
            ui.weak(format!("{:?}", port.data_type));
            ui.separator();
            let published_parameter = definition
                .interface
                .parameters
                .iter()
                .find(|entry| entry.target == port.id.address);
            let published_input = definition
                .interface
                .media_inputs
                .iter()
                .find(|entry| entry.target == port.id.address);
            let published_outputs = definition
                .interface
                .media_outputs
                .iter()
                .filter(|entry| entry.source == port.id.address)
                .collect::<Vec<_>>();

            if let Some(parameter) = published_parameter {
                ui.label(format!("Published parameter: {}", parameter.name));
                if ui.button("Unpublish parameter").clicked() {
                    actions.push(ModuleEditorAction::EditInterface(
                        ModuleInterfaceCommand::UnpublishParameter {
                            parameter_id: parameter.id,
                        },
                    ));
                    ui.close();
                }
                return;
            }
            if let Some(input) = published_input {
                let role = if input.primary {
                    "Primary input"
                } else {
                    "Published input"
                };
                ui.label(format!("{role}: {}", input.name));
                if ui.button("Unpublish media input").clicked() {
                    actions.push(ModuleEditorAction::EditInterface(
                        ModuleInterfaceCommand::UnpublishMediaInput { input_id: input.id },
                    ));
                    ui.close();
                }
                return;
            }
            for output in &published_outputs {
                ui.label(format!("Published output: {}", output.name));
            }

            match port.id.direction {
                PortDirection::Input
                    if matches!(port.data_type, PortDataType::Image | PortDataType::Audio) =>
                {
                    let connected = definition
                        .graph
                        .connections
                        .iter()
                        .any(|connection| connection.to == port.id.address);
                    let primary_action = primary_media_input_action(definition, port);
                    let primary_button = ui.add_enabled(
                        primary_action.is_ok(),
                        egui::Button::new(format!("{} Set as primary input", icons::ARROW_RIGHT)),
                    );
                    if primary_button.clicked() {
                        if let Ok(command) = &primary_action {
                            actions.push(ModuleEditorAction::EditInterface(command.clone()));
                            ui.close();
                        }
                    } else if let Err(reason) = &primary_action {
                        primary_button.on_hover_text(reason);
                    }
                    if ui
                        .add_enabled(
                            !connected,
                            egui::Button::new(format!(
                                "{} Publish as additional input",
                                icons::PLUG
                            )),
                        )
                        .clicked()
                    {
                        actions.push(ModuleEditorAction::EditInterface(
                            ModuleInterfaceCommand::PublishMediaInput {
                                name: port.label.clone(),
                                target: port.id.address.clone(),
                                required: false,
                                primary: false,
                            },
                        ));
                        ui.close();
                    }
                    if connected {
                        ui.weak("Disconnect this port before exposing it.");
                    } else if let Err(reason) = primary_action {
                        ui.weak(reason);
                    }
                }
                PortDirection::Input => {
                    let default_value = module_port_default(definition, &port.id.address);
                    if ui
                        .add_enabled(
                            default_value.is_some(),
                            egui::Button::new("Publish as parameter"),
                        )
                        .clicked()
                    {
                        if let Some(default_value) = default_value.clone() {
                            actions.push(ModuleEditorAction::EditInterface(
                                ModuleInterfaceCommand::PublishParameter {
                                    name: port.label.clone(),
                                    default_value,
                                    target: port.id.address.clone(),
                                },
                            ));
                            ui.close();
                        }
                    }
                    if default_value.is_none() {
                        ui.weak("This input has no publishable authored default.");
                    }
                }
                PortDirection::Output
                    if matches!(port.data_type, PortDataType::Image | PortDataType::Audio) =>
                {
                    let output_action = published_output_action(definition, active_output_id, port);
                    let output_button = ui.add_enabled(
                        output_action.is_ok(),
                        egui::Button::new(format!(
                            "{} Set as published output",
                            icons::ARROW_SQUARE_OUT
                        )),
                    );
                    if output_button.clicked() {
                        if let Ok(command) = &output_action {
                            actions.push(ModuleEditorAction::EditInterface(command.clone()));
                            ui.close();
                        }
                    } else if let Err(reason) = &output_action {
                        output_button.on_hover_text(reason);
                    }
                    if published_outputs.is_empty()
                        && ui
                            .button(format!("{} Publish additional output", icons::PLUG))
                            .clicked()
                    {
                        actions.push(ModuleEditorAction::EditInterface(
                            ModuleInterfaceCommand::PublishMediaOutput {
                                name: port.label.clone(),
                                source: port.id.address.clone(),
                            },
                        ));
                        ui.close();
                    }
                    if let Err(reason) = output_action {
                        ui.weak(reason);
                    }
                }
                PortDirection::Output => {
                    ui.weak("Signal publishing is not exposed in this vertical slice.");
                }
            }
        });
    }
    actions
}

fn primary_media_input_action(
    definition: &ModuleDefinition,
    port: &PortVisual,
) -> Result<ModuleInterfaceCommand, String> {
    if port.id.direction != PortDirection::Input
        || !matches!(port.data_type, PortDataType::Image | PortDataType::Audio)
    {
        return Err("Only media input ports can be the primary input.".to_string());
    }
    if definition
        .graph
        .connections
        .iter()
        .any(|connection| connection.to == port.id.address)
    {
        return Err("Disconnect this port before exposing it.".to_string());
    }
    if definition
        .interface
        .parameters
        .iter()
        .any(|entry| entry.target == port.id.address)
        || definition
            .interface
            .media_inputs
            .iter()
            .any(|entry| entry.target == port.id.address)
        || definition
            .interface
            .actions
            .iter()
            .any(|entry| entry.target == port.id.address)
    {
        return Err("This port is already part of the Published Interface.".to_string());
    }
    let Some(primary) = definition
        .interface
        .media_inputs
        .iter()
        .find(|entry| entry.primary)
    else {
        return Ok(ModuleInterfaceCommand::PublishMediaInput {
            name: port.label.clone(),
            target: port.id.address.clone(),
            required: true,
            primary: true,
        });
    };
    if primary.data_type != port.data_type {
        return Err(format!(
            "The primary input is {:?}; this port is {:?}.",
            primary.data_type, port.data_type
        ));
    }
    Ok(ModuleInterfaceCommand::RetargetPrimaryMediaInput {
        input_id: primary.id,
        target: port.id.address.clone(),
    })
}

fn published_output_action(
    definition: &ModuleDefinition,
    active_output_id: Option<PublishedMediaOutputId>,
    port: &PortVisual,
) -> Result<ModuleInterfaceCommand, String> {
    if port.id.direction != PortDirection::Output
        || !matches!(port.data_type, PortDataType::Image | PortDataType::Audio)
    {
        return Err("Only media output ports can be published.".to_string());
    }
    let output_id = active_output_id
        .ok_or_else(|| "Open this Module through one of its placements first.".to_string())?;
    let output = definition
        .interface
        .media_outputs
        .iter()
        .find(|entry| entry.id == output_id)
        .ok_or_else(|| "The placement's Published Output no longer exists.".to_string())?;
    if output.source == port.id.address {
        return Err("This is already the placement's Published Output.".to_string());
    }
    if output.data_type != port.data_type {
        return Err(format!(
            "The published output is {:?}; this port is {:?}.",
            output.data_type, port.data_type
        ));
    }
    Ok(ModuleInterfaceCommand::ReplaceMediaOutputSource {
        output_id,
        source: port.id.address.clone(),
    })
}

fn module_port_default(
    definition: &ModuleDefinition,
    address: &ModulePortAddress,
) -> Option<PropertyValue> {
    let node = definition.graph.nodes.get(&address.node_id)?;
    let key = library::plugin::property_name_from_port(&address.port).unwrap_or(&address.port);
    node.properties().get(key)?.value().cloned()
}

fn show_module_create_menu(
    ui: &mut egui::Ui,
    state: &mut ModuleNodeEditorState,
    plugin_manager: &PluginManager,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
    node_rects: Vec<egui::Rect>,
) -> Option<(ModuleNodeCreateRequest, egui::Pos2)> {
    let (secondary_clicked, pointer_position, open_time) = ui.input(|input| {
        (
            input.pointer.secondary_clicked(),
            input.pointer.interact_pos(),
            input.time,
        )
    });
    update_context_menu_for_secondary_click(
        &mut state.create_menu,
        secondary_clicked,
        pointer_position,
        viewport,
        &node_rects,
        transform,
        open_time,
    );

    let mut selected = None;
    let mut should_close = false;
    if let Some(context) = state.create_menu.as_ref() {
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
                    let items = module_node_menu_items(plugin_manager);
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
        state.create_menu = None;
    }
    selected
}

fn update_context_menu_for_secondary_click(
    state: &mut Option<ModuleCreateMenuState>,
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
        *state = None;
        return;
    }
    *state = Some(ModuleCreateMenuState::new(position, open_time));
}

mod host;
pub use host::module_node_editor_panel;
fn translate_outputs(
    outputs: Vec<EditorOutput<Uuid, ModuleEditorPortId, ModuleConnectionId, Uuid>>,
    state: &mut ModuleNodeEditorState,
) -> Vec<ModuleEditorAction> {
    let mut actions = Vec::new();
    for output in outputs {
        match output {
            EditorOutput::Select { items, primary } => {
                state.selected_nodes = items
                    .iter()
                    .filter_map(|item| match item {
                        ItemId::Node(node_id) => Some(*node_id),
                        ItemId::Group(_) | ItemId::Wire(_) => None,
                    })
                    .collect();
                state.selected_connection = items.iter().find_map(|item| match item {
                    ItemId::Wire(connection_id) => Some(*connection_id),
                    ItemId::Node(_) | ItemId::Group(_) => None,
                });
                state.primary_node = match primary {
                    Some(ItemId::Node(node_id)) => Some(node_id),
                    Some(ItemId::Wire(connection_id)) => {
                        state.selected_connection = Some(connection_id);
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
                if state.selected_connection == Some(wire) {
                    state.selected_connection = None;
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
    use library::model::authoring::{
        ModuleDefinitionId, ModuleDefinitionSharing, ModuleGraph, ModuleInterface,
        ModuleTemplateOrigin, PublishedMediaInput, PublishedMediaInputId, PublishedMediaOutput,
    };
    use library::model::project::{
        AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT,
    };
    use library::model::Node;

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
        let mut state = ModuleNodeEditorState {
            canvas_zoom: 1.0,
            ..ModuleNodeEditorState::default()
        };
        let plugins = PluginManager::default();
        let actions = std::cell::RefCell::new(Vec::new());
        drop(context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                *actions.borrow_mut() =
                    show_module_document(ui, &definition, None, &mut state, &plugins, 0.0);
            });
        }));
        assert!(actions.into_inner().is_empty());
        assert!(state.selected_nodes.is_empty());
    }

    #[test]
    fn delete_disconnects_explicit_wires_before_removing_nodes() {
        let node_id = Uuid::new_v4();
        let connection_id = ModuleConnectionId::new();
        let mut state = ModuleNodeEditorState::default();
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

    #[test]
    fn port_actions_retarget_stable_primary_input_and_active_output_ids() {
        let input = Node::new_merge("Input");
        let processing = Node::new_merge("Processing");
        let primary_input_id = PublishedMediaInputId::new();
        let output_id = PublishedMediaOutputId::new();
        let definition = ModuleDefinition {
            id: ModuleDefinitionId::new(),
            name: "Image effect".to_string(),
            sharing: ModuleDefinitionSharing::Private,
            graph: ModuleGraph {
                nodes: HashMap::from([
                    (input.id, input.clone()),
                    (processing.id, processing.clone()),
                ]),
                connections: Vec::new(),
            },
            interface: ModuleInterface {
                media_inputs: vec![PublishedMediaInput {
                    id: primary_input_id,
                    name: "Host image".to_string(),
                    data_type: PortDataType::Image,
                    target: ModulePortAddress {
                        node_id: input.id,
                        port: MERGE_IMAGES_PORT.to_string(),
                    },
                    required: true,
                    primary: true,
                }],
                media_outputs: vec![PublishedMediaOutput {
                    id: output_id,
                    name: "Image".to_string(),
                    data_type: PortDataType::Image,
                    source: ModulePortAddress {
                        node_id: input.id,
                        port: IMAGE_OUTPUT_PORT.to_string(),
                    },
                }],
                ..ModuleInterface::default()
            },
            topology_revision: 1,
            interface_version: 1,
        };
        let replacement_input = PortVisual {
            id: ModuleEditorPortId {
                address: ModulePortAddress {
                    node_id: processing.id,
                    port: MERGE_IMAGES_PORT.to_string(),
                },
                direction: PortDirection::Input,
            },
            label: "Images".to_string(),
            center: egui::Pos2::ZERO,
            data_type: PortDataType::Image,
        };
        let replacement_output = PortVisual {
            id: ModuleEditorPortId {
                address: ModulePortAddress {
                    node_id: processing.id,
                    port: IMAGE_OUTPUT_PORT.to_string(),
                },
                direction: PortDirection::Output,
            },
            label: "Image".to_string(),
            center: egui::Pos2::ZERO,
            data_type: PortDataType::Image,
        };

        assert_eq!(
            primary_media_input_action(&definition, &replacement_input),
            Ok(ModuleInterfaceCommand::RetargetPrimaryMediaInput {
                input_id: primary_input_id,
                target: replacement_input.id.address.clone(),
            })
        );
        assert_eq!(
            published_output_action(&definition, Some(output_id), &replacement_output),
            Ok(ModuleInterfaceCommand::ReplaceMediaOutputSource {
                output_id,
                source: replacement_output.id.address.clone(),
            })
        );
    }

    #[test]
    fn port_actions_reject_connected_or_type_changing_targets() {
        let image = Node::new_merge("Image");
        let connected = Node::new_merge("Connected");
        let audio = Node::new_sound_merge("Audio");
        let primary_input_id = PublishedMediaInputId::new();
        let output_id = PublishedMediaOutputId::new();
        let connected_address = ModulePortAddress {
            node_id: connected.id,
            port: MERGE_IMAGES_PORT.to_string(),
        };
        let definition = ModuleDefinition {
            id: ModuleDefinitionId::new(),
            name: "Safety".to_string(),
            sharing: ModuleDefinitionSharing::Private,
            graph: ModuleGraph {
                nodes: HashMap::from([
                    (image.id, image.clone()),
                    (connected.id, connected),
                    (audio.id, audio.clone()),
                ]),
                connections: vec![library::model::authoring::ModuleConnection {
                    id: ModuleConnectionId::new(),
                    from: ModulePortAddress {
                        node_id: image.id,
                        port: IMAGE_OUTPUT_PORT.to_string(),
                    },
                    to: connected_address.clone(),
                    order: 0,
                }],
            },
            interface: ModuleInterface {
                media_inputs: vec![PublishedMediaInput {
                    id: primary_input_id,
                    name: "Host image".to_string(),
                    data_type: PortDataType::Image,
                    target: ModulePortAddress {
                        node_id: image.id,
                        port: MERGE_IMAGES_PORT.to_string(),
                    },
                    required: true,
                    primary: true,
                }],
                media_outputs: vec![PublishedMediaOutput {
                    id: output_id,
                    name: "Image".to_string(),
                    data_type: PortDataType::Image,
                    source: ModulePortAddress {
                        node_id: image.id,
                        port: IMAGE_OUTPUT_PORT.to_string(),
                    },
                }],
                ..ModuleInterface::default()
            },
            topology_revision: 1,
            interface_version: 1,
        };
        let connected_port = PortVisual {
            id: ModuleEditorPortId {
                address: connected_address,
                direction: PortDirection::Input,
            },
            label: "Images".to_string(),
            center: egui::Pos2::ZERO,
            data_type: PortDataType::Image,
        };
        let audio_input = PortVisual {
            id: ModuleEditorPortId {
                address: ModulePortAddress {
                    node_id: audio.id,
                    port: MERGE_SOUNDS_PORT.to_string(),
                },
                direction: PortDirection::Input,
            },
            label: "Sounds".to_string(),
            center: egui::Pos2::ZERO,
            data_type: PortDataType::Audio,
        };
        let audio_output = PortVisual {
            id: ModuleEditorPortId {
                address: ModulePortAddress {
                    node_id: audio.id,
                    port: AUDIO_OUTPUT_PORT.to_string(),
                },
                direction: PortDirection::Output,
            },
            label: "Audio".to_string(),
            center: egui::Pos2::ZERO,
            data_type: PortDataType::Audio,
        };

        assert!(primary_media_input_action(&definition, &connected_port).is_err());
        assert!(primary_media_input_action(&definition, &audio_input).is_err());
        assert!(published_output_action(&definition, Some(output_id), &audio_output).is_err());
    }

    #[test]
    fn property_ports_use_the_body_label_instead_of_overpainting_it() {
        let mut properties = PropertyMap::new();
        properties.set(
            "opacity".to_string(),
            Property::constant(PropertyValue::Number(1.0.into())),
        );

        assert_eq!(
            authored_property_key_for_port(&properties, "property:opacity"),
            Some("opacity")
        );
        assert_eq!(
            authored_property_key_for_port(&properties, "opacity"),
            Some("opacity")
        );
        assert_eq!(
            authored_property_key_for_port(&properties, "image_in"),
            None
        );
    }
}
