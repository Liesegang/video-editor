//! A Module-definition document hosted by the production Node Editor.
//!
//! This is a real `egui-snarl` document, just like the pre-v1 Project graph.
//! The payload contains stable Module Node IDs only; graph data stays in the
//! authoritative `ModuleDefinition` and edits go through
//! `TimelineEditorService`. Timeline items are never projected into it.

use eframe::egui;
use library::editor::{ModuleInterfaceCommand, TimelineEditorService};
use library::model::asset::Asset;
use library::model::authoring::{
    AuthoringProject, ModuleConnectionId, ModuleDefinition, ModuleInputPortOwnership,
    ModuleInstanceId, ModuleNodePortContract, ModulePortAddress,
};
use library::model::project::{PortDataType, PortDirection};
use library::model::property::{Property, PropertyDefinition, PropertyValue};
use library::model::{Node, NodeContent};
use library::plugin::PluginManager;
use node_editor_ui::{EditorOutput, ItemId, MoveEndOutcome};
use uuid::Uuid;

use crate::state::authoring::AuthoringUiState;
use crate::state::node_editor::{
    ModuleEditorHost, ModuleEditorPortId, NodeEditorDocument, NodeEditorState,
};
mod clock;
mod context_menu;
mod host;
mod interface;
mod layout;
mod menu;
mod property;
mod surface;
mod viewer;

#[cfg(test)]
mod tests;

pub use host::node_editor_panel;
use menu::ModuleNodeCreateRequest;
use surface::show_module_document;

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
    Reconnect {
        connection_id: ModuleConnectionId,
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
    CreateAssetNode {
        asset_id: Uuid,
        graph_position: egui::Pos2,
    },
    EditInterface(ModuleInterfaceCommand),
}

#[derive(Clone, Debug)]
struct PortVisual {
    id: ModuleEditorPortId,
    label: String,
    center: egui::Pos2,
    data_type: PortDataType,
}

#[derive(Clone, Copy, Debug)]
struct ModulePropertyContext {
    time: f64,
    fps: f64,
    resolution: (u64, u64),
}

fn authored_property_key_for_port<'a>(node: &'a Node, port_key: &str) -> Option<&'a str> {
    let property_key = library::plugin::property_name_from_port(port_key).unwrap_or(port_key);
    node.properties()
        .iter()
        .find_map(|(key, _)| (key == property_key).then_some(key.as_str()))
}

/// Port contract presented by this Module document.
///
/// Generic Modules may expose both Image and Audio on one Output terminal.
/// A Transition host has one explicit media contract, so its protected Output
/// presents only that type instead of suggesting a second, unusable output.
fn document_port_contract(
    definition: &ModuleDefinition,
    node: &Node,
) -> Result<ModuleNodePortContract, String> {
    let mut contract = ModuleNodePortContract::resolve(node)?;
    if matches!(node.content(), NodeContent::ModuleOutput(_)) {
        if let Some(transition) = definition.host_contract.transition() {
            let media_type = transition.media_type.port_data_type();
            contract.ports.retain(|port| port.data_type == media_type);
        }
    }
    Ok(contract)
}

fn translate_surface_outputs(
    definition: &ModuleDefinition,
    outputs: Vec<EditorOutput<Uuid, ModuleEditorPortId, ModuleConnectionId, Uuid>>,
    state: &mut NodeEditorState,
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
            EditorOutput::Delete { items } => {
                let mut nodes = Vec::new();
                let mut connections = Vec::new();
                for item in items {
                    match item {
                        ItemId::Node(node_id)
                            if !is_module_output_node(definition, node_id)
                                && !definition.is_protected_host_boundary_node(node_id) =>
                        {
                            nodes.push(node_id);
                        }
                        ItemId::Node(_) => {}
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
            EditorOutput::DeselectWire { wire } => {
                if state.selected_connection == Some(wire) {
                    state.selected_connection = None;
                }
            }
            EditorOutput::Disconnect { wire } => {
                actions.push(ModuleEditorAction::Disconnect(wire));
            }
            EditorOutput::WireContextMenu { .. } => {}
            EditorOutput::Connect { from, to }
                if from.direction == PortDirection::Output
                    && to.direction == PortDirection::Input =>
            {
                actions.push(ModuleEditorAction::Connect {
                    from: from.address,
                    to: to.address,
                });
            }
            EditorOutput::Reconnect { wire, from, to }
                if from.direction == PortDirection::Output
                    && to.direction == PortDirection::Input =>
            {
                actions.push(ModuleEditorAction::Reconnect {
                    connection_id: wire,
                    from: from.address,
                    to: to.address,
                });
            }
            EditorOutput::Connect { .. }
            | EditorOutput::Reconnect { .. }
            | EditorOutput::LayoutSwipe(_)
            | EditorOutput::Reparent { .. }
            | EditorOutput::ResizeGroup { .. } => {}
        }
    }
    actions
}

fn is_module_output_node(definition: &ModuleDefinition, node_id: Uuid) -> bool {
    definition
        .graph
        .nodes
        .get(&node_id)
        .is_some_and(|node| matches!(node.content(), NodeContent::ModuleOutput(_)))
}
