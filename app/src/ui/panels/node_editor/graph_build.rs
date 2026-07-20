use eframe::egui;
use egui_snarl::Snarl;
use library::model::project::{PortAddress, PortDirection, PortOwner, PortSide};
use library::model::Project;
use std::collections::HashMap;
use uuid::Uuid;

use super::{
    input_definitions, output_definitions, ContainerKind, ContainerVisual, GraphItem,
    PortAnchorKind, CONTAINER_CONTROL_OFFSET, CONTAINER_PORT_Y, CONTAINER_RIGHT_PORT_Y,
    MIN_CONTAINER_SIZE,
};

pub(super) fn build_snarl(
    project: &Project,
    comp_id: Uuid,
) -> (Snarl<GraphItem>, Vec<ContainerVisual>) {
    let mut snarl = Snarl::new();
    let mut snarl_ids = HashMap::new();
    let mut containers = Vec::new();

    let Some(composition) = project.get_composition(comp_id) else {
        return (snarl, containers);
    };
    let composition_visual = ContainerVisual {
        owner: PortOwner::Composition(composition.id),
        kind: ContainerKind::Composition,
        position: composition.ui_position,
        size: composition.ui_size,
        collapsed: composition.ui_collapsed,
    };
    insert_container_items(&composition_visual, &mut snarl, &mut snarl_ids);
    containers.push(composition_visual);

    if !composition.ui_collapsed {
        insert_leaf_nodes(project, &composition.node_ids, &mut snarl, &mut snarl_ids);
        for track_id in &composition.track_ids {
            let Some(track) = project.get_track(*track_id) else {
                continue;
            };
            let track_visual = ContainerVisual {
                owner: PortOwner::Track(track.id),
                kind: ContainerKind::Track,
                position: track.ui_position,
                size: track.ui_size,
                collapsed: track.ui_collapsed,
            };
            insert_container_items(&track_visual, &mut snarl, &mut snarl_ids);
            containers.push(track_visual);

            if !track.ui_collapsed {
                insert_leaf_nodes(project, &track.node_ids, &mut snarl, &mut snarl_ids);
                for clip_id in &track.clip_ids {
                    let Some(clip) = project.get_clip(*clip_id) else {
                        continue;
                    };
                    let clip_visual = ContainerVisual {
                        owner: PortOwner::Clip(clip.id),
                        kind: ContainerKind::Clip,
                        position: clip.ui_position,
                        size: clip.ui_size,
                        collapsed: clip.ui_collapsed,
                    };
                    insert_container_items(&clip_visual, &mut snarl, &mut snarl_ids);
                    containers.push(clip_visual);
                    if !clip.ui_collapsed {
                        insert_leaf_nodes(project, &clip.node_ids, &mut snarl, &mut snarl_ids);
                    }
                }
            }
        }
    }

    for connection in &project.connections {
        let Some(source_item) = output_graph_item(project, &connection.from) else {
            continue;
        };
        let Some(target_item) = input_graph_item(project, &connection.to) else {
            continue;
        };
        let (Some(source_snarl_id), Some(target_snarl_id)) =
            (snarl_ids.get(&source_item), snarl_ids.get(&target_item))
        else {
            continue;
        };
        let Some(output_index) = output_definitions(project, source_item)
            .iter()
            .position(|output| output.key == connection.from.port)
        else {
            continue;
        };
        let Some(input_index) = input_definitions(project, target_item)
            .iter()
            .position(|input| input.key == connection.to.port)
        else {
            continue;
        };
        snarl.connect(
            egui_snarl::OutPinId {
                node: *source_snarl_id,
                output: output_index,
            },
            egui_snarl::InPinId {
                node: *target_snarl_id,
                input: input_index,
            },
        );
    }

    connect_container_output_wires(project, &containers, &snarl_ids, &mut snarl);

    (snarl, containers)
}

fn connect_container_output_wires(
    project: &Project,
    containers: &[ContainerVisual],
    snarl_ids: &HashMap<GraphItem, egui_snarl::NodeId>,
    snarl: &mut Snarl<GraphItem>,
) {
    for visual in containers {
        let sink_item = GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::ImageSink,
        };
        let Some(&sink_id) = snarl_ids.get(&sink_item) else {
            continue;
        };
        for source in project.container_image_sources(visual.owner) {
            let source_owner = source.source;
            let source_item = match source_owner {
                PortOwner::Node(id) => GraphItem::Node(id),
                owner @ (PortOwner::Composition(_) | PortOwner::Track(_) | PortOwner::Clip(_)) => {
                    GraphItem::PortAnchor {
                        owner,
                        kind: PortAnchorKind::ExternalOutputs,
                    }
                }
            };
            let Some(&source_id) = snarl_ids.get(&source_item) else {
                continue;
            };
            let Some(output_index) =
                output_definitions(project, source_item)
                    .iter()
                    .position(|definition| {
                        definition.key == library::model::project::IMAGE_OUTPUT_PORT
                    })
            else {
                continue;
            };
            snarl.connect(
                egui_snarl::OutPinId {
                    node: source_id,
                    output: output_index,
                },
                egui_snarl::InPinId {
                    node: sink_id,
                    input: 0,
                },
            );
        }
    }
}

fn insert_leaf_nodes(
    project: &Project,
    node_ids: &[Uuid],
    snarl: &mut Snarl<GraphItem>,
    snarl_ids: &mut HashMap<GraphItem, egui_snarl::NodeId>,
) {
    for node_id in node_ids {
        let Some(node) = project.get_node(*node_id) else {
            continue;
        };
        let item = GraphItem::Node(*node_id);
        if snarl_ids.contains_key(&item) {
            continue;
        }
        let position = node.ui_position;
        let snarl_id = snarl.insert_node(egui::pos2(position[0], position[1]), item);
        snarl_ids.insert(item, snarl_id);
    }
}

fn insert_container_items(
    visual: &ContainerVisual,
    snarl: &mut Snarl<GraphItem>,
    snarl_ids: &mut HashMap<GraphItem, egui_snarl::NodeId>,
) {
    let items = [
        GraphItem::Container(visual.owner),
        GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::ExternalInputs,
        },
        GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::InternalMetadata,
        },
        GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::ImageSink,
        },
        GraphItem::PortAnchor {
            owner: visual.owner,
            kind: PortAnchorKind::ExternalOutputs,
        },
    ];

    for item in items {
        let position = container_item_position(visual, item);
        let node_id = snarl.insert_node(position, item);
        snarl_ids.insert(item, node_id);
    }
}

pub(super) fn container_item_position(visual: &ContainerVisual, item: GraphItem) -> egui::Pos2 {
    let position = egui::pos2(visual.position[0], visual.position[1]);
    let size = egui::vec2(
        visual.size[0].max(MIN_CONTAINER_SIZE.x),
        visual.size[1].max(MIN_CONTAINER_SIZE.y),
    );
    let port_y = if visual.collapsed {
        12.0
    } else {
        CONTAINER_PORT_Y
    };
    match item {
        GraphItem::Container(_) => position + CONTAINER_CONTROL_OFFSET,
        GraphItem::PortAnchor {
            kind: PortAnchorKind::ExternalInputs,
            ..
        } => egui::pos2(position.x - 14.0, position.y + port_y),
        GraphItem::PortAnchor {
            kind: PortAnchorKind::InternalMetadata,
            ..
        } => egui::pos2(position.x + 2.0, position.y + port_y),
        GraphItem::PortAnchor {
            kind: PortAnchorKind::ImageSink,
            ..
        } => egui::pos2(
            position.x + size.x - 40.0,
            position.y + CONTAINER_RIGHT_PORT_Y,
        ),
        GraphItem::PortAnchor {
            kind: PortAnchorKind::ExternalOutputs,
            ..
        } => egui::pos2(
            position.x + size.x - 2.0,
            position.y + CONTAINER_RIGHT_PORT_Y,
        ),
        GraphItem::Node(_) => position,
    }
}

fn output_graph_item(project: &Project, address: &PortAddress) -> Option<GraphItem> {
    let definition = project.port_definition(address, PortDirection::Output)?;
    match address.owner {
        PortOwner::Composition(_) | PortOwner::Track(_) | PortOwner::Clip(_) => {
            match definition.side {
                PortSide::Left => Some(GraphItem::PortAnchor {
                    owner: address.owner,
                    kind: PortAnchorKind::InternalMetadata,
                }),
                PortSide::Right => Some(GraphItem::PortAnchor {
                    owner: address.owner,
                    kind: PortAnchorKind::ExternalOutputs,
                }),
            }
        }
        PortOwner::Node(node_id) if project.get_node(node_id).is_some() => {
            Some(GraphItem::Node(node_id))
        }
        PortOwner::Node(_) => None,
    }
}

fn input_graph_item(project: &Project, address: &PortAddress) -> Option<GraphItem> {
    project.port_definition(address, PortDirection::Input)?;
    match address.owner {
        PortOwner::Composition(id) if project.get_composition(id).is_some() => {
            Some(GraphItem::PortAnchor {
                owner: address.owner,
                kind: PortAnchorKind::ExternalInputs,
            })
        }
        PortOwner::Track(id) if project.get_track(id).is_some() => Some(GraphItem::PortAnchor {
            owner: address.owner,
            kind: PortAnchorKind::ExternalInputs,
        }),
        PortOwner::Clip(id) if project.get_clip(id).is_some() => Some(GraphItem::PortAnchor {
            owner: address.owner,
            kind: PortAnchorKind::ExternalInputs,
        }),
        PortOwner::Node(id) if project.get_node(id).is_some() => Some(GraphItem::Node(id)),
        _ => None,
    }
}

pub(super) fn container_visual(project: &Project, owner: PortOwner) -> Option<ContainerVisual> {
    match owner {
        PortOwner::Composition(id) => {
            project
                .get_composition(id)
                .map(|composition| ContainerVisual {
                    owner,
                    kind: ContainerKind::Composition,
                    position: composition.ui_position,
                    size: composition.ui_size,
                    collapsed: composition.ui_collapsed,
                })
        }
        PortOwner::Track(id) => project.get_track(id).map(|track| ContainerVisual {
            owner,
            kind: ContainerKind::Track,
            position: track.ui_position,
            size: track.ui_size,
            collapsed: track.ui_collapsed,
        }),
        PortOwner::Clip(id) => project.get_clip(id).map(|clip| ContainerVisual {
            owner,
            kind: ContainerKind::Clip,
            position: clip.ui_position,
            size: clip.ui_size,
            collapsed: clip.ui_collapsed,
        }),
        PortOwner::Node(_) => None,
    }
}
