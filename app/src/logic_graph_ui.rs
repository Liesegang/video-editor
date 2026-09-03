use std::collections::HashMap;

use eframe::egui;
use library::model::authoring::{
    ModuleConnectionId, ModuleDefinition, ModuleDefinitionId, ModulePortAddress,
};
use library::model::node::{native_node_descriptor_for_node, Node, NodeContent};
use library::model::project::{PortDefinition, PortDirection as ModelPortDirection};
use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, Editor, EditorConfig, EditorOutput, GraphFrame,
    InteractionState, ItemId, NodeBodyRenderer, NodeBodyResponse, NodeDescriptor, PortDescriptor,
    PortDirection, PortOwner, TypeKey, WireDescriptor,
};

type GroupId = u8;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct LogicPortId {
    node_id: uuid::Uuid,
    key: String,
    direction: ModelPortDirection,
}

#[derive(Default)]
pub struct LogicGraphState {
    interaction: InteractionState<uuid::Uuid, LogicPortId, ModuleConnectionId, GroupId>,
    selection: Vec<ItemId<uuid::Uuid, GroupId, ModuleConnectionId>>,
    primary: Option<ItemId<uuid::Uuid, GroupId, ModuleConnectionId>>,
}

#[derive(Debug)]
pub enum LogicGraphEdit {
    MoveNode {
        definition_id: ModuleDefinitionId,
        node_id: uuid::Uuid,
        position: [f32; 2],
        size: [f32; 2],
        collapsed: bool,
    },
    Connect {
        definition_id: ModuleDefinitionId,
        from: ModulePortAddress,
        to: ModulePortAddress,
    },
    Disconnect {
        definition_id: ModuleDefinitionId,
        connection_id: ModuleConnectionId,
    },
    DeleteNode {
        definition_id: ModuleDefinitionId,
        node_id: uuid::Uuid,
    },
}

pub fn show(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    state: &mut LogicGraphState,
) -> Vec<LogicGraphEdit> {
    let viewport = ui.available_rect_before_wrap();
    if viewport.width() < 32.0 || viewport.height() < 32.0 {
        return Vec::new();
    }

    let mut ordered_nodes = definition.graph.nodes.values().collect::<Vec<_>>();
    ordered_nodes.sort_by_key(|node| node.id);
    let node_rects = ordered_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let fallback = [24.0 + index as f32 * 280.0, 32.0];
            let position = if node.ui_position == [0.0, 0.0] && index > 0 {
                fallback
            } else {
                node.ui_position
            };
            let size = [node.ui_size[0].max(180.0), node.ui_size[1].max(100.0)];
            (
                node.id,
                egui::Rect::from_min_size(
                    egui::pos2(position[0], position[1]),
                    egui::vec2(size[0], size[1]),
                ),
            )
        })
        .collect::<HashMap<_, _>>();

    let nodes = ordered_nodes
        .iter()
        .map(|node| {
            let rect = node_rects[&node.id];
            NodeDescriptor {
                id: node.id,
                title: node.name.as_str(),
                rect,
                header_rect: egui::Rect::from_min_max(
                    rect.min,
                    egui::pos2(rect.max.x, rect.min.y + 28.0),
                ),
                parent: None,
                enabled: node.enabled,
            }
        })
        .collect::<Vec<_>>();

    let declared_ports = ordered_nodes
        .iter()
        .map(|node| (node.id, ports_for_node(node)))
        .collect::<HashMap<_, _>>();
    let mut ports = Vec::new();
    for node in &ordered_nodes {
        let rect = node_rects[&node.id];
        let mut input_row = 0_u32;
        let mut output_row = 0_u32;
        for port in &declared_ports[&node.id] {
            let (direction, row, x) = match port.direction {
                ModelPortDirection::Input => {
                    let row = input_row;
                    input_row += 1;
                    (PortDirection::Input, row, rect.left())
                }
                ModelPortDirection::Output => {
                    let row = output_row;
                    output_row += 1;
                    (PortDirection::Output, row, rect.right())
                }
            };
            ports.push(PortDescriptor {
                id: LogicPortId {
                    node_id: node.id,
                    key: port.key.clone(),
                    direction: port.direction,
                },
                owner: PortOwner::Node(node.id),
                label: port.label.as_str(),
                center: egui::pos2(x, rect.top() + 48.0 + row as f32 * 22.0),
                direction,
                type_key: TypeKey::new(port.data_type),
                connectable: true,
            });
        }
    }
    let centers = ports
        .iter()
        .map(|port| (port.id.clone(), port.center))
        .collect::<HashMap<_, _>>();
    let wires = definition
        .graph
        .connections
        .iter()
        .filter_map(|connection| {
            let from = LogicPortId {
                node_id: connection.from.node_id,
                key: connection.from.port.clone(),
                direction: ModelPortDirection::Output,
            };
            let to = LogicPortId {
                node_id: connection.to.node_id,
                key: connection.to.port.clone(),
                direction: ModelPortDirection::Input,
            };
            let start = *centers.get(&from)?;
            let end = *centers.get(&to)?;
            let control = ((end.x - start.x).abs() * 0.5).max(48.0);
            Some(WireDescriptor {
                id: connection.id,
                from,
                to,
                curve: CubicBezier::new(
                    start,
                    start + egui::vec2(control, 0.0),
                    end - egui::vec2(control, 0.0),
                    end,
                ),
                editable: true,
            })
        })
        .collect::<Vec<_>>();
    let selection_order = nodes
        .iter()
        .map(|node| ItemId::Node(node.id))
        .collect::<Vec<_>>();
    let transform = egui::emath::TSTransform::new(viewport.min.to_vec2(), 1.0);
    let frame = GraphFrame {
        viewport,
        transform,
        nodes: &nodes,
        ports: &ports,
        wires: &wires,
        groups: &[],
        selection_order: &selection_order,
        selection: AuthoritativeSelection {
            items: &state.selection,
            primary: state.primary,
        },
    };
    let mut renderer = ModuleNodeBody {
        nodes: &definition.graph.nodes,
    };
    let outputs = Editor::show(
        ui,
        &frame,
        &mut state.interaction,
        &mut renderer,
        EditorConfig::default(),
    );
    translate_outputs(definition, state, &node_rects, outputs)
}

fn translate_outputs(
    definition: &ModuleDefinition,
    state: &mut LogicGraphState,
    node_rects: &HashMap<uuid::Uuid, egui::Rect>,
    outputs: Vec<EditorOutput<uuid::Uuid, LogicPortId, ModuleConnectionId, GroupId>>,
) -> Vec<LogicGraphEdit> {
    let mut edits = Vec::new();
    for output in outputs {
        match output {
            EditorOutput::Select { items, primary } => {
                state.selection = items;
                state.primary = primary;
            }
            EditorOutput::Move { items, delta, .. } => {
                for item in items {
                    if let ItemId::Node(node_id) = item {
                        let node = &definition.graph.nodes[&node_id];
                        let position = node_rects[&node_id].min + delta;
                        edits.push(LogicGraphEdit::MoveNode {
                            definition_id: definition.id,
                            node_id,
                            position: [position.x, position.y],
                            size: node.ui_size,
                            collapsed: node.ui_collapsed,
                        });
                    }
                }
            }
            EditorOutput::Connect { from, to } => {
                let (from, to) = if from.direction == ModelPortDirection::Output {
                    (from, to)
                } else {
                    (to, from)
                };
                edits.push(LogicGraphEdit::Connect {
                    definition_id: definition.id,
                    from: ModulePortAddress {
                        node_id: from.node_id,
                        port: from.key,
                    },
                    to: ModulePortAddress {
                        node_id: to.node_id,
                        port: to.key,
                    },
                });
            }
            EditorOutput::Disconnect { wire } => edits.push(LogicGraphEdit::Disconnect {
                definition_id: definition.id,
                connection_id: wire,
            }),
            EditorOutput::Delete { items } => {
                for item in items {
                    match item {
                        ItemId::Node(node_id) => edits.push(LogicGraphEdit::DeleteNode {
                            definition_id: definition.id,
                            node_id,
                        }),
                        ItemId::Wire(connection_id) => {
                            edits.push(LogicGraphEdit::Disconnect {
                                definition_id: definition.id,
                                connection_id,
                            });
                        }
                        ItemId::Group(_) => {}
                    }
                }
            }
            EditorOutput::MoveEnd { .. }
            | EditorOutput::LayoutSwipe(_)
            | EditorOutput::Reparent { .. }
            | EditorOutput::ResizeGroup { .. }
            | EditorOutput::DeselectWire { .. } => {}
        }
    }
    edits
}

fn ports_for_node(node: &Node) -> Vec<PortDefinition> {
    match node.content() {
        NodeContent::PluginOperation(operation) => operation.declared_ports.clone(),
        _ => native_node_descriptor_for_node(node)
            .map(|descriptor| descriptor.ports().to_vec())
            .unwrap_or_default(),
    }
}

struct ModuleNodeBody<'a> {
    nodes: &'a HashMap<uuid::Uuid, Node>,
}

impl NodeBodyRenderer<uuid::Uuid> for ModuleNodeBody<'_> {
    fn show(&mut self, node_id: &uuid::Uuid, ui: &mut egui::Ui) -> NodeBodyResponse {
        let node = &self.nodes[node_id];
        ui.small(if node.enabled { "Enabled" } else { "Disabled" });
        ui.small(format!("{} properties", node.properties().iter().count()));
        NodeBodyResponse::NONE
    }
}
