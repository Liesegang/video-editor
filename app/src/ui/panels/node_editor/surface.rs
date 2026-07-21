//! Thin projection from the authoritative video Project to node-editor-ui.
//!
//! All vectors in this module live for one UI frame. They are neither a graph
//! model nor a cache: mutations are returned as generic intents and resolved
//! by the panel against `Project`, `SelectionState`, and history.

use std::collections::HashMap;

use eframe::egui;
use library::model::project::{
    PortAddress, PortDataType, PortDirection as ProjectPortDirection, PortOwner as ProjectPortOwner,
};
use library::model::Project;
use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, EditorOutput, GraphFrame, GroupDescriptor, ItemId,
    NodeDescriptor, PortDescriptor, PortDirection, PortInstanceId, PortOwner, TypeKey,
    WireDescriptor,
};
use uuid::Uuid;

use crate::state::context_types::{NodeEditorEditableWire, SelectionTarget};

use super::{
    container_output_binding_port, container_output_binding_type, container_output_port,
    parent_container_owner, port_owner_for_node_container, ContainerVisual, RenderedEdge,
    RenderedEdgeKind, RenderedPortKey, CONTAINER_HEADER_HEIGHT,
};

pub(super) type SurfacePortId = PortInstanceId<PortAddress, NodeEditorEditableWire>;
pub(super) type SurfaceOutput =
    EditorOutput<Uuid, SurfacePortId, NodeEditorEditableWire, ProjectPortOwner>;

pub(super) struct SurfaceProjection<'a> {
    nodes: Vec<NodeDescriptor<'a, Uuid, ProjectPortOwner>>,
    ports: Vec<PortDescriptor<'static, Uuid, SurfacePortId, ProjectPortOwner, PortDataType>>,
    wires: Vec<WireDescriptor<SurfacePortId, NodeEditorEditableWire>>,
    groups: Vec<GroupDescriptor<'a, ProjectPortOwner>>,
    selection: Vec<ItemId<Uuid, ProjectPortOwner, NodeEditorEditableWire>>,
    primary: Option<ItemId<Uuid, ProjectPortOwner, NodeEditorEditableWire>>,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
}

impl<'a> SurfaceProjection<'a> {
    #[allow(
        clippy::too_many_arguments,
        reason = "all arguments are borrowed parts of one production render frame"
    )]
    pub(super) fn from_project(
        project: &'a Project,
        containers: &[ContainerVisual],
        rendered_node_rects: &HashMap<Uuid, egui::Rect>,
        rendered_ports: &HashMap<RenderedPortKey, egui::Rect>,
        rendered_edges: &[RenderedEdge],
        selection: &[SelectionTarget],
        primary: Option<SelectionTarget>,
        selected_wire: Option<Uuid>,
        viewport: egui::Rect,
        transform: egui::emath::TSTransform,
    ) -> Self {
        let nodes = rendered_node_rects
            .iter()
            .filter_map(|(node_id, rect)| {
                let node = project.get_node(*node_id)?;
                Some(NodeDescriptor {
                    id: *node_id,
                    title: node.name.as_str(),
                    rect: *rect,
                    parent: project
                        .find_node_container(*node_id)
                        .map(port_owner_for_node_container),
                    enabled: node.enabled,
                })
            })
            .collect();
        let groups = containers
            .iter()
            .filter_map(|container| {
                let title = container_name(project, container.owner)?;
                let rect = container.rect();
                let header_rect = egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width(), CONTAINER_HEADER_HEIGHT.min(rect.height())),
                );
                Some(GroupDescriptor {
                    id: container.owner,
                    title,
                    rect,
                    header_rect,
                    parent: parent_container_owner(project, container.owner),
                    resizable: !container.collapsed,
                })
            })
            .collect();
        let ports = rendered_ports
            .keys()
            .map(|key| {
                let wire = key.connection_id.map(|connection_id| {
                    NodeEditorEditableWire::ProjectConnection { connection_id }
                });
                PortDescriptor {
                    id: PortInstanceId::new(key.address.clone(), wire),
                    owner: surface_port_owner(key.address.owner),
                    label: "",
                    center: transform.inverse() * rendered_ports[key].center(),
                    direction: surface_direction(key.direction),
                    type_key: TypeKey::new(port_type(project, key)),
                    connectable: true,
                }
            })
            .collect::<Vec<_>>();
        let wires = rendered_edges
            .iter()
            .filter_map(|edge| surface_wire(project, edge, &ports, transform))
            .collect();
        let mut selected_items = selection
            .iter()
            .copied()
            .map(surface_selection_item)
            .collect::<Vec<_>>();
        let selected_wire = selected_wire
            .map(|connection_id| NodeEditorEditableWire::ProjectConnection { connection_id });
        if let Some(wire) = selected_wire {
            selected_items.push(ItemId::Wire(wire));
        }
        let primary = selected_wire
            .map(ItemId::Wire)
            .or_else(|| primary.map(surface_selection_item));

        Self {
            nodes,
            ports,
            wires,
            groups,
            selection: selected_items,
            primary,
            viewport,
            transform,
        }
    }

    pub(super) fn frame(
        &self,
    ) -> GraphFrame<'_, Uuid, SurfacePortId, NodeEditorEditableWire, ProjectPortOwner, PortDataType>
    {
        GraphFrame {
            viewport: self.viewport,
            transform: self.transform,
            nodes: &self.nodes,
            ports: &self.ports,
            wires: &self.wires,
            groups: &self.groups,
            selection: AuthoritativeSelection {
                items: &self.selection,
                primary: self.primary,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SurfaceSelectionChange {
    pub(super) targets: Vec<SelectionTarget>,
    pub(super) primary: Option<SelectionTarget>,
}

pub(super) fn selection_change(outputs: &[SurfaceOutput]) -> Option<SurfaceSelectionChange> {
    outputs.iter().rev().find_map(|output| {
        let SurfaceOutput::Select { items, primary } = output else {
            return None;
        };
        let targets = items
            .iter()
            .filter_map(selection_target)
            .collect::<Vec<_>>();
        if targets.is_empty() && items.iter().any(|item| matches!(item, ItemId::Wire(_))) {
            return None;
        }
        Some(SurfaceSelectionChange {
            targets,
            primary: primary.as_ref().and_then(selection_target),
        })
    })
}

pub(super) fn deselects_wire(outputs: &[SurfaceOutput]) -> bool {
    outputs
        .iter()
        .any(|output| matches!(output, SurfaceOutput::DeselectWire { .. }))
}

fn selection_target(
    item: &ItemId<Uuid, ProjectPortOwner, NodeEditorEditableWire>,
) -> Option<SelectionTarget> {
    match item {
        ItemId::Node(id) => Some(SelectionTarget::Node(*id)),
        ItemId::Group(owner) => Some(match owner {
            ProjectPortOwner::Composition(id) => SelectionTarget::Composition(*id),
            ProjectPortOwner::Track(id) => SelectionTarget::Track(*id),
            ProjectPortOwner::Clip(id) => SelectionTarget::Clip(*id),
            ProjectPortOwner::Node(id) => SelectionTarget::Node(*id),
        }),
        ItemId::Wire(_) => None,
    }
}

const fn surface_selection_item(
    target: SelectionTarget,
) -> ItemId<Uuid, ProjectPortOwner, NodeEditorEditableWire> {
    match target {
        SelectionTarget::Node(id) => ItemId::Node(id),
        SelectionTarget::Clip(id) => ItemId::Group(ProjectPortOwner::Clip(id)),
        SelectionTarget::Track(id) => ItemId::Group(ProjectPortOwner::Track(id)),
        SelectionTarget::Composition(id) => ItemId::Group(ProjectPortOwner::Composition(id)),
    }
}

fn surface_wire(
    project: &Project,
    edge: &RenderedEdge,
    ports: &[PortDescriptor<'_, Uuid, SurfacePortId, ProjectPortOwner, PortDataType>],
    transform: egui::emath::TSTransform,
) -> Option<WireDescriptor<SurfacePortId, NodeEditorEditableWire>> {
    let wire = edge.kind.editable_wire()?;
    let (from_address, to_address) = match edge.kind {
        RenderedEdgeKind::ProjectConnection { connection_id } => {
            let connection = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)?;
            (connection.from.clone(), connection.to.clone())
        }
        RenderedEdgeKind::OutputBinding {
            owner,
            node_id,
            data_type,
        } => (
            PortAddress::new(
                ProjectPortOwner::Node(node_id),
                container_output_port(data_type)?,
            ),
            PortAddress::new(owner, container_output_binding_port(data_type)?),
        ),
        RenderedEdgeKind::DerivedOutput { .. } => return None,
    };
    let from = find_port(ports, &from_address, PortDirection::Output, None)?
        .id
        .clone();
    let to = find_port(ports, &to_address, PortDirection::Input, Some(wire))
        .or_else(|| find_port(ports, &to_address, PortDirection::Input, None))?
        .id
        .clone();
    let inverse = transform.inverse();
    Some(WireDescriptor {
        id: wire,
        from,
        to,
        curve: CubicBezier::new(
            inverse * edge.start,
            inverse * edge.control_a,
            inverse * edge.control_b,
            inverse * edge.end,
        ),
        editable: true,
    })
}

fn find_port<'a>(
    ports: &'a [PortDescriptor<'_, Uuid, SurfacePortId, ProjectPortOwner, PortDataType>],
    address: &PortAddress,
    direction: PortDirection,
    wire: Option<NodeEditorEditableWire>,
) -> Option<&'a PortDescriptor<'a, Uuid, SurfacePortId, ProjectPortOwner, PortDataType>> {
    ports.iter().find(|port| {
        port.id.port == *address && port.id.wire == wire && port.direction == direction
    })
}

fn port_type(project: &Project, key: &RenderedPortKey) -> PortDataType {
    project
        .port_definition(&key.address, key.direction)
        .map(|definition| definition.data_type)
        .or_else(|| container_output_binding_type(&key.address.port))
        .unwrap_or(PortDataType::Any)
}

const fn surface_direction(direction: ProjectPortDirection) -> PortDirection {
    match direction {
        ProjectPortDirection::Input => PortDirection::Input,
        ProjectPortDirection::Output => PortDirection::Output,
    }
}

const fn surface_port_owner(owner: ProjectPortOwner) -> PortOwner<Uuid, ProjectPortOwner> {
    match owner {
        ProjectPortOwner::Node(id) => PortOwner::Node(id),
        ProjectPortOwner::Composition(_)
        | ProjectPortOwner::Track(_)
        | ProjectPortOwner::Clip(_) => PortOwner::Group(owner),
    }
}

fn container_name(project: &Project, owner: ProjectPortOwner) -> Option<&str> {
    match owner {
        ProjectPortOwner::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.name.as_str()),
        ProjectPortOwner::Track(id) => project.get_track(id).map(|track| track.name.as_str()),
        ProjectPortOwner::Clip(id) => project.get_clip(id).map(|clip| clip.name.as_str()),
        ProjectPortOwner::Node(id) => project.get_node(id).map(|node| node.name.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use library::model::Composition;

    #[test]
    fn selection_intent_maps_opaque_groups_without_registry_probe_order() {
        let node = Uuid::from_u128(1);
        let clip = Uuid::from_u128(2);
        let outputs = [SurfaceOutput::Select {
            items: vec![
                ItemId::Node(node),
                ItemId::Group(ProjectPortOwner::Clip(clip)),
            ],
            primary: Some(ItemId::Group(ProjectPortOwner::Clip(clip))),
        }];

        assert_eq!(
            selection_change(&outputs),
            Some(SurfaceSelectionChange {
                targets: vec![SelectionTarget::Node(node), SelectionTarget::Clip(clip)],
                primary: Some(SelectionTarget::Clip(clip)),
            })
        );
    }

    #[test]
    fn wire_only_selection_does_not_clear_project_item_selection() {
        let wire = NodeEditorEditableWire::ProjectConnection {
            connection_id: Uuid::from_u128(3),
        };
        let outputs = [SurfaceOutput::Select {
            items: vec![ItemId::Wire(wire)],
            primary: Some(ItemId::Wire(wire)),
        }];

        assert_eq!(selection_change(&outputs), None);
    }

    #[test]
    fn production_projection_drives_core_selection_with_real_pointer_input() {
        let mut project = Project::new("surface adapter");
        let (composition, track) = Composition::new("Main", 320, 180, 24.0, 2.0);
        let composition_id = composition.id;
        let track_id = track.id;
        assert!(project.add_track(track).is_ok());
        assert!(project.add_composition(composition).is_ok());
        let composition = project.get_composition(composition_id);
        let track = project.get_track(track_id);
        assert!(composition.is_some());
        assert!(track.is_some());
        let containers = [
            ContainerVisual {
                owner: ProjectPortOwner::Composition(composition_id),
                kind: super::super::ContainerKind::Composition,
                position: composition.map_or([0.0, 0.0], |item| item.ui_position),
                size: composition.map_or([640.0, 420.0], |item| item.ui_size),
                collapsed: composition.is_some_and(|item| item.ui_collapsed),
            },
            ContainerVisual {
                owner: ProjectPortOwner::Track(track_id),
                kind: super::super::ContainerKind::Track,
                position: track.map_or([100.0, 100.0], |item| item.ui_position),
                size: track.map_or([480.0, 300.0], |item| item.ui_size),
                collapsed: track.is_some_and(|item| item.ui_collapsed),
            },
        ];
        let selected = [SelectionTarget::Composition(composition_id)];
        let node_rects = HashMap::new();
        let port_rects = HashMap::new();
        let edges = Vec::new();
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_000.0, 700.0));
        let projection = SurfaceProjection::from_project(
            &project,
            &containers,
            &node_rects,
            &port_rects,
            &edges,
            &selected,
            Some(selected[0]),
            None,
            viewport,
            egui::emath::TSTransform::IDENTITY,
        );
        let click = containers[1].rect().min + egui::vec2(160.0, 20.0);
        let context = egui::Context::default();
        let outputs = RefCell::new(Vec::new());
        let mut state = node_editor_ui::InteractionState::default();
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(viewport),
                events: vec![
                    egui::Event::PointerMoved(click),
                    egui::Event::PointerButton {
                        pos: click,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(context, |ui| {
                        outputs
                            .borrow_mut()
                            .extend(node_editor_ui::Editor::interact(
                                ui,
                                &projection.frame(),
                                &mut state,
                                node_editor_ui::InteractionOptions::SELECTION,
                                false,
                            ));
                    });
            },
        ));

        assert_eq!(
            selection_change(&outputs.into_inner()),
            Some(SurfaceSelectionChange {
                targets: vec![SelectionTarget::Track(track_id)],
                primary: Some(SelectionTarget::Track(track_id)),
            })
        );
    }
}
