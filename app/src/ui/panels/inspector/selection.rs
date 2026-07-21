use std::collections::HashSet;

use library::model::project::{ContainerGraphSemantics, PortOwner, Project, ProjectConnection};
use library::model::{Clip, Composition, Node, Track};
use uuid::Uuid;

use crate::state::context_types::SelectionTarget;

use super::presentation::{resolve_node_time_source, NodeTimeSource};

#[derive(Clone, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "the Inspector takes one short-lived authoritative selection snapshot per frame"
)]
pub(super) enum InspectorSelection {
    Composition {
        composition: Composition,
        nodes: Vec<Node>,
        connections: Vec<ProjectConnection>,
        semantics: ContainerGraphSemantics,
    },
    Track {
        track: Track,
        nodes: Vec<Node>,
        connections: Vec<ProjectConnection>,
        semantics: ContainerGraphSemantics,
    },
    Clip {
        clip: Clip,
        track_id: Option<Uuid>,
    },
    Node {
        node: Node,
        track_id: Option<Uuid>,
        containing_clip: Option<Clip>,
        time_source: Option<NodeTimeSource>,
    },
}

pub(super) fn resolve_selection(
    project: &Project,
    selected_target: Option<SelectionTarget>,
    composition_id: Uuid,
) -> Option<InspectorSelection> {
    match selected_target {
        Some(SelectionTarget::Clip(clip_id))
            if project
                .find_track_for_clip(clip_id)
                .and_then(|track_id| project.find_composition_for_track(track_id))
                == Some(composition_id) =>
        {
            if let Some(clip) = project.get_clip(clip_id) {
                return Some(InspectorSelection::Clip {
                    clip: clip.clone(),
                    track_id: project.find_track_for_clip(clip_id),
                });
            }
        }
        Some(SelectionTarget::Node(node_id))
            if node_containing_composition(project, node_id) == Some(composition_id) =>
        {
            if let Some(node) = project.get_node(node_id) {
                let containing_clip = project
                    .find_parent_clip(node_id)
                    .and_then(|clip_id| project.get_clip(clip_id))
                    .cloned();
                return Some(InspectorSelection::Node {
                    node: node.clone(),
                    track_id: node_containing_track(project, node_id),
                    containing_clip,
                    time_source: resolve_node_time_source(project, node_id),
                });
            }
        }
        Some(SelectionTarget::Track(track_id))
            if project.find_composition_for_track(track_id) == Some(composition_id) =>
        {
            if let Some(track) = project.get_track(track_id) {
                return Some(InspectorSelection::Track {
                    track: track.clone(),
                    nodes: nodes_for_ids(project, &track.node_ids),
                    connections: connections_for_nodes(project, &track.node_ids),
                    semantics: project.container_graph_semantics(PortOwner::Track(track.id)),
                });
            }
        }
        Some(SelectionTarget::Composition(selected_composition_id))
            if selected_composition_id == composition_id => {}
        Some(
            SelectionTarget::Node(_)
            | SelectionTarget::Clip(_)
            | SelectionTarget::Track(_)
            | SelectionTarget::Composition(_),
        ) => return None,
        None => {}
    }

    let composition = project.get_composition(composition_id)?;
    Some(InspectorSelection::Composition {
        composition: composition.clone(),
        nodes: nodes_for_ids(project, &composition.node_ids),
        connections: connections_for_nodes(project, &composition.node_ids),
        semantics: project.container_graph_semantics(PortOwner::Composition(composition.id)),
    })
}

fn node_containing_composition(project: &Project, node_id: Uuid) -> Option<Uuid> {
    match project.find_node_container(node_id)? {
        library::model::NodeContainer::Composition(id) => Some(id),
        library::model::NodeContainer::Track(id) => project.find_composition_for_track(id),
        library::model::NodeContainer::Clip(id) => project
            .find_track_for_clip(id)
            .and_then(|track_id| project.find_composition_for_track(track_id)),
    }
}

fn node_containing_track(project: &Project, node_id: Uuid) -> Option<Uuid> {
    match project.find_node_container(node_id)? {
        library::model::NodeContainer::Composition(_) => None,
        library::model::NodeContainer::Track(track_id) => Some(track_id),
        library::model::NodeContainer::Clip(clip_id) => project.find_track_for_clip(clip_id),
    }
}

fn nodes_for_ids(project: &Project, node_ids: &[Uuid]) -> Vec<Node> {
    node_ids
        .iter()
        .filter_map(|node_id| project.get_node(*node_id).cloned())
        .collect()
}

pub(super) fn connections_for_nodes(
    project: &Project,
    node_ids: &[Uuid],
) -> Vec<ProjectConnection> {
    let node_ids = node_ids.iter().copied().collect::<HashSet<_>>();
    project
        .connections
        .iter()
        .filter(|connection| {
            matches!(connection.from.owner, PortOwner::Node(id) if node_ids.contains(&id))
                || matches!(connection.to.owner, PortOwner::Node(id) if node_ids.contains(&id))
        })
        .cloned()
        .collect()
}
