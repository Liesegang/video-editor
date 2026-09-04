//! Thin projection from the authoritative video Project to node-editor-ui.
//!
//! All vectors in this module live for one UI frame. They are neither a graph
//! model nor a cache: mutations are returned as generic intents and resolved
//! by the panel against `Project`, `SelectionState`, and history.

use std::collections::HashMap;

use eframe::egui;
use library::model::Project;
use library::model::project::{
    PortAddress, PortDataType, PortDirection as ProjectPortDirection, PortOwner as ProjectPortOwner,
};
use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, EditorOutput, GraphFrame, GroupDescriptor, ItemId,
    MoveEndOutcome, NodeBodyResponse, NodeDescriptor, PortDescriptor, PortDirection,
    PortInstanceId, PortOwner, TypeKey, WireDescriptor,
};
use uuid::Uuid;

use crate::state::context_types::{NodeEditorEditableWire, SelectionTarget};

use super::{
    CONTAINER_HEADER_HEIGHT, ContainerVisual, LayoutEdit, PORT_ROW_HEIGHT, RenderedEdge,
    RenderedEdgeKind, RenderedPortKey, container_output_binding_port,
    container_output_binding_type, container_output_port, parent_container_owner,
    port_owner_for_node_container,
};

pub(super) type SurfacePortId = PortInstanceId<PortAddress, NodeEditorEditableWire>;
pub(super) type SurfaceOutput =
    EditorOutput<Uuid, SurfacePortId, NodeEditorEditableWire, ProjectPortOwner>;

/// Frame-local geometry/order observed from Snarl's real draw callbacks.
///
/// Hash maps remain lookup accelerators only. Selection and port hit priority
/// come from these back-to-front vectors, so randomized map iteration can
/// never change which overlapping production surface wins.
#[derive(Default)]
pub(super) struct SurfaceCapture {
    node_headers: HashMap<Uuid, egui::Rect>,
    selectable_order: Vec<SelectionTarget>,
    port_order: Vec<RenderedPortKey>,
    body_pointer_owned: bool,
}

impl SurfaceCapture {
    pub(super) fn record_node_header(&mut self, node_id: Uuid, rect: egui::Rect) {
        self.node_headers.insert(node_id, rect);
    }

    pub(super) fn record_selectable(&mut self, target: SelectionTarget) {
        self.selectable_order.retain(|existing| *existing != target);
        self.selectable_order.push(target);
    }

    pub(super) fn record_port(&mut self, key: RenderedPortKey) {
        self.port_order.retain(|existing| *existing != key);
        self.port_order.push(key);
    }

    pub(super) fn record_body_response(&mut self, response: &egui::Response) {
        self.body_pointer_owned |= NodeBodyResponse::from_response(response).owns_pointer();
    }

    pub(super) const fn body_pointer_owned(&self) -> bool {
        self.body_pointer_owned
    }
}

pub(super) struct SurfaceProjection<'a> {
    nodes: Vec<NodeDescriptor<'a, Uuid, ProjectPortOwner>>,
    ports: Vec<PortDescriptor<'static, Uuid, SurfacePortId, ProjectPortOwner, PortDataType>>,
    wires: Vec<WireDescriptor<SurfacePortId, NodeEditorEditableWire>>,
    groups: Vec<GroupDescriptor<'a, ProjectPortOwner>>,
    selection_order: Vec<ItemId<Uuid, ProjectPortOwner, NodeEditorEditableWire>>,
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
        capture: &SurfaceCapture,
        rendered_edges: &[RenderedEdge],
        selection: &[SelectionTarget],
        primary: Option<SelectionTarget>,
        selected_wire: Option<Uuid>,
        viewport: egui::Rect,
        transform: egui::emath::TSTransform,
    ) -> Self {
        let selection_order = ordered_selection_items(
            project,
            containers,
            rendered_node_rects,
            &capture.selectable_order,
        );
        let nodes = selection_order
            .iter()
            .filter_map(|item| {
                let ItemId::Node(node_id) = item else {
                    return None;
                };
                let rect = *rendered_node_rects.get(node_id)?;
                let node = project.get_node(*node_id)?;
                let header_rect = capture
                    .node_headers
                    .get(node_id)
                    .copied()
                    .unwrap_or_else(|| {
                        egui::Rect::from_min_size(
                            rect.min,
                            egui::vec2(rect.width(), PORT_ROW_HEIGHT.min(rect.height())),
                        )
                    });
                Some(NodeDescriptor {
                    id: *node_id,
                    title: node.name.as_str(),
                    rect,
                    header_rect,
                    parent: project
                        .find_node_container(*node_id)
                        .map(port_owner_for_node_container),
                    enabled: node.enabled,
                })
            })
            .collect();
        let groups = selection_order
            .iter()
            .filter_map(|item| {
                let ItemId::Group(owner) = item else {
                    return None;
                };
                let container = containers
                    .iter()
                    .find(|container| container.owner == *owner)?;
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
        let ports = ordered_port_keys(rendered_ports, &capture.port_order)
            .into_iter()
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
            .filter_map(surface_selection_item)
            .collect::<Vec<_>>();
        let selected_wire = selected_wire
            .map(|connection_id| NodeEditorEditableWire::ProjectConnection { connection_id });
        if let Some(wire) = selected_wire {
            selected_items.push(ItemId::Wire(wire));
        }
        let primary = selected_wire
            .map(ItemId::Wire)
            .or_else(|| primary.and_then(surface_selection_item));

        Self {
            nodes,
            ports,
            wires,
            groups,
            selection_order,
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
            selection_order: &self.selection_order,
            selection: AuthoritativeSelection {
                items: &self.selection,
                primary: self.primary,
            },
        }
    }
}

fn ordered_selection_items(
    project: &Project,
    containers: &[ContainerVisual],
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    captured: &[SelectionTarget],
) -> Vec<ItemId<Uuid, ProjectPortOwner, NodeEditorEditableWire>> {
    let mut ordered = captured
        .iter()
        .copied()
        .filter(|target| match target {
            SelectionTarget::Node(id) => {
                rendered_node_rects.contains_key(id) && project.get_node(*id).is_some()
            }
            SelectionTarget::Composition(id) => containers
                .iter()
                .any(|container| container.owner == ProjectPortOwner::Composition(*id)),
            SelectionTarget::Track(id) => containers
                .iter()
                .any(|container| container.owner == ProjectPortOwner::Track(*id)),
            SelectionTarget::Clip(id) => containers
                .iter()
                .any(|container| container.owner == ProjectPortOwner::Clip(*id)),
            SelectionTarget::TimelineItem(_) => false,
        })
        .filter_map(surface_selection_item)
        .collect::<Vec<_>>();

    // A missing callback should degrade deterministically for tests/partial
    // frames, never by HashMap iteration. Production normally adds nothing in
    // these fallback loops because every painted selectable was captured.
    for container in containers {
        let item = ItemId::Group(container.owner);
        if !ordered.contains(&item) {
            ordered.push(item);
        }
    }
    let mut remaining_nodes = rendered_node_rects.keys().copied().collect::<Vec<_>>();
    remaining_nodes.sort_unstable();
    for node_id in remaining_nodes {
        let item = ItemId::Node(node_id);
        if project.get_node(node_id).is_some() && !ordered.contains(&item) {
            ordered.push(item);
        }
    }
    ordered
}

fn ordered_port_keys<'a>(
    rendered_ports: &'a HashMap<RenderedPortKey, egui::Rect>,
    captured: &'a [RenderedPortKey],
) -> Vec<&'a RenderedPortKey> {
    let mut ordered = captured
        .iter()
        .filter(|key| rendered_ports.contains_key(*key))
        .collect::<Vec<_>>();
    let mut remaining = rendered_ports
        .keys()
        .filter(|key| !ordered.contains(key))
        .collect::<Vec<_>>();
    remaining
        .sort_by(|left, right| rendered_port_sort_key(left).cmp(&rendered_port_sort_key(right)));
    ordered.extend(remaining);
    ordered
}

fn rendered_port_sort_key(key: &RenderedPortKey) -> (u8, Uuid, &str, u8, Option<Uuid>) {
    let owner_rank = match key.address.owner {
        ProjectPortOwner::Composition(_) => 0,
        ProjectPortOwner::Track(_) => 1,
        ProjectPortOwner::Clip(_) => 2,
        ProjectPortOwner::Node(_) => 3,
    };
    let direction_rank = match key.direction {
        ProjectPortDirection::Input => 0,
        ProjectPortDirection::Output => 1,
    };
    (
        owner_rank,
        key.address.owner.id(),
        key.address.port.as_str(),
        direction_rank,
        key.connection_id,
    )
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

/// Return the typed terminal outcome for a position-changing Move. The core
/// emits exactly one, and production uses it instead of inferring completion
/// from unrelated raw pointer releases.
pub(super) fn move_end(outputs: &[SurfaceOutput]) -> Option<MoveEndOutcome> {
    outputs.iter().find_map(|output| match output {
        SurfaceOutput::MoveEnd { outcome } => Some(*outcome),
        _ => None,
    })
}

#[derive(Debug)]
pub(super) struct SurfaceMoveChange {
    pub(super) edits: Vec<LayoutEdit>,
    pub(super) grabbed_node: Option<Uuid>,
}

/// Resolve a domain-neutral incremental Move against the authoritative
/// Project. Hierarchy filtering belongs here: moving a selected Composition,
/// Track, or Clip already translates its descendants, so selected descendants
/// must not receive the same delta twice.
pub(super) fn move_change(
    project: &Project,
    outputs: &[SurfaceOutput],
) -> Option<SurfaceMoveChange> {
    outputs.iter().find_map(|output| {
        let SurfaceOutput::Move {
            items,
            grabbed,
            delta,
        } = output
        else {
            return None;
        };
        let candidates = items
            .iter()
            .filter_map(selection_target)
            .collect::<Vec<_>>();
        let edits = selection_move_roots(project, &candidates)
            .into_iter()
            .filter_map(|target| layout_edit_for_delta(project, target, *delta))
            .collect();
        Some(SurfaceMoveChange {
            edits,
            grabbed_node: match grabbed {
                ItemId::Node(node_id) => Some(*node_id),
                ItemId::Group(_) | ItemId::Wire(_) => None,
            },
        })
    })
}

fn selection_move_roots(project: &Project, selected: &[SelectionTarget]) -> Vec<SelectionTarget> {
    let mut roots = Vec::new();
    for target in selected.iter().copied() {
        if roots.contains(&target)
            || selected.iter().copied().any(|candidate| {
                candidate != target && target_is_ancestor(project, candidate, target)
            })
        {
            continue;
        }
        roots.push(target);
    }
    roots
}

fn target_is_ancestor(
    project: &Project,
    ancestor: SelectionTarget,
    descendant: SelectionTarget,
) -> bool {
    let Some(ancestor) = owner_for_selection_target(ancestor) else {
        return false;
    };
    let Some(descendant) = owner_for_selection_target(descendant) else {
        return false;
    };
    let mut current = parent_owner(project, descendant);
    while let Some(owner) = current {
        if owner == ancestor {
            return true;
        }
        current = parent_owner(project, owner);
    }
    false
}

fn parent_owner(project: &Project, owner: ProjectPortOwner) -> Option<ProjectPortOwner> {
    match owner {
        ProjectPortOwner::Node(id) => project
            .find_node_container(id)
            .map(port_owner_for_node_container),
        ProjectPortOwner::Composition(_)
        | ProjectPortOwner::Track(_)
        | ProjectPortOwner::Clip(_) => parent_container_owner(project, owner),
    }
}

const fn owner_for_selection_target(target: SelectionTarget) -> Option<ProjectPortOwner> {
    match target {
        SelectionTarget::Node(id) => Some(ProjectPortOwner::Node(id)),
        SelectionTarget::Clip(id) => Some(ProjectPortOwner::Clip(id)),
        SelectionTarget::Track(id) => Some(ProjectPortOwner::Track(id)),
        SelectionTarget::Composition(id) => Some(ProjectPortOwner::Composition(id)),
        SelectionTarget::TimelineItem(_) => None,
    }
}

fn layout_edit_for_delta(
    project: &Project,
    target: SelectionTarget,
    delta: egui::Vec2,
) -> Option<LayoutEdit> {
    match target {
        SelectionTarget::Node(node_id) => {
            let node = project.get_node(node_id)?;
            Some(LayoutEdit::MoveNode {
                node_id,
                position: [node.ui_position[0] + delta.x, node.ui_position[1] + delta.y],
            })
        }
        SelectionTarget::Clip(id) => Some(LayoutEdit::MoveContainer {
            owner: ProjectPortOwner::Clip(id),
            delta: [delta.x, delta.y],
        }),
        SelectionTarget::Track(id) => Some(LayoutEdit::MoveContainer {
            owner: ProjectPortOwner::Track(id),
            delta: [delta.x, delta.y],
        }),
        SelectionTarget::Composition(id) => Some(LayoutEdit::MoveContainer {
            owner: ProjectPortOwner::Composition(id),
            delta: [delta.x, delta.y],
        }),
        SelectionTarget::TimelineItem(_) => None,
    }
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
) -> Option<ItemId<Uuid, ProjectPortOwner, NodeEditorEditableWire>> {
    match target {
        SelectionTarget::Node(id) => Some(ItemId::Node(id)),
        SelectionTarget::Clip(id) => Some(ItemId::Group(ProjectPortOwner::Clip(id))),
        SelectionTarget::Track(id) => Some(ItemId::Group(ProjectPortOwner::Track(id))),
        SelectionTarget::Composition(id) => Some(ItemId::Group(ProjectPortOwner::Composition(id))),
        SelectionTarget::TimelineItem(_) => None,
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
mod tests;
