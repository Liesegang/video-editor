use std::collections::HashSet;

use uuid::Uuid;

use super::super::{
    AUDIO_OUTPUT_PORT, DEFAULT_GRAPH_CONTENT_INSET, DEFAULT_GRAPH_ITEM_GAP, IMAGE_OUTPUT_PORT,
    MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT, NodeContainer, PortAddress, PortDataType, PortDirection,
    PortOwner, Project, ProjectConnection, ProjectGraphError,
};
use crate::model::{Node, NodeContent};

#[derive(Clone, Copy)]
pub(super) enum StructuralMergeKind {
    Image,
    Sound,
}

impl StructuralMergeKind {
    pub(super) const ALL: [Self; 2] = [Self::Image, Self::Sound];

    pub(super) fn source_port(self) -> &'static str {
        match self {
            Self::Image => IMAGE_OUTPUT_PORT,
            Self::Sound => AUDIO_OUTPUT_PORT,
        }
    }

    pub(super) fn input_port(self) -> &'static str {
        match self {
            Self::Image => MERGE_IMAGES_PORT,
            Self::Sound => MERGE_SOUNDS_PORT,
        }
    }

    pub(super) fn data_type(self) -> PortDataType {
        match self {
            Self::Image => PortDataType::Image,
            Self::Sound => PortDataType::Audio,
        }
    }

    pub(super) fn node_matches(self, node: &Node) -> bool {
        matches!(
            (self, node.content()),
            (Self::Image, NodeContent::Merge) | (Self::Sound, NodeContent::SoundMerge)
        )
    }
}

impl Project {
    pub(super) fn structural_merge_id_for(
        &self,
        container: NodeContainer,
        kind: StructuralMergeKind,
    ) -> Option<Uuid> {
        match container {
            NodeContainer::Composition(id) => {
                self.get_composition(id).map(|composition| match kind {
                    StructuralMergeKind::Image => composition.structural_merge_node_id,
                    StructuralMergeKind::Sound => composition.structural_sound_merge_node_id,
                })
            }
            NodeContainer::Track(id) => self.get_track(id).map(|track| match kind {
                StructuralMergeKind::Image => track.structural_merge_node_id,
                StructuralMergeKind::Sound => track.structural_sound_merge_node_id,
            }),
            NodeContainer::Clip(_) => None,
        }
    }

    pub(in crate::model::project) fn structural_merge_owner(
        &self,
        node_id: Uuid,
    ) -> Option<NodeContainer> {
        self.compositions
            .iter()
            .find(|composition| {
                composition.structural_merge_node_id == node_id
                    || composition.structural_sound_merge_node_id == node_id
            })
            .map(|composition| NodeContainer::Composition(composition.id))
            .or_else(|| {
                self.tracks
                    .values()
                    .find(|track| {
                        track.structural_merge_node_id == node_id
                            || track.structural_sound_merge_node_id == node_id
                    })
                    .map(|track| NodeContainer::Track(track.id))
            })
    }

    pub(crate) fn structural_sound_merge_owner(&self, node_id: Uuid) -> Option<NodeContainer> {
        self.compositions
            .iter()
            .find(|composition| composition.structural_sound_merge_node_id == node_id)
            .map(|composition| NodeContainer::Composition(composition.id))
            .or_else(|| {
                self.tracks
                    .values()
                    .find(|track| track.structural_sound_merge_node_id == node_id)
                    .map(|track| NodeContainer::Track(track.id))
            })
    }

    pub(in crate::model::project) fn structural_merge_is_well_formed(
        &self,
        container: NodeContainer,
    ) -> bool {
        self.structural_merge_is_well_formed_for(container, StructuralMergeKind::Image)
    }

    pub(crate) fn structural_sound_merge_is_well_formed(&self, container: NodeContainer) -> bool {
        self.structural_merge_is_well_formed_for(container, StructuralMergeKind::Sound)
    }

    fn structural_merge_is_well_formed_for(
        &self,
        container: NodeContainer,
        kind: StructuralMergeKind,
    ) -> bool {
        let Some(node_id) = self.structural_merge_id_for(container, kind) else {
            return false;
        };
        let owner = container_owner(container);
        if !self.container_directly_contains_node(owner, node_id)
            || !self
                .get_node(node_id)
                .is_some_and(|node| kind.node_matches(node))
        {
            return false;
        }
        let Some(target) = self.structural_merge_target_for(container, kind) else {
            return false;
        };
        if !self
            .port_definition(&target, PortDirection::Input)
            .is_some_and(|definition| {
                definition.data_type == kind.data_type()
                    && definition.multiplicity == super::super::PortMultiplicity::Variadic
            })
        {
            return false;
        }
        let children = self.structural_child_owners(container);
        let mut inputs = self
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .collect::<Vec<_>>();
        inputs.sort_by_key(|connection| (connection.order, connection.id));
        inputs.len() >= children.len()
            && children.into_iter().zip(inputs).enumerate().all(
                |(expected_order, (child, connection))| {
                    connection.from == PortAddress::new(child, kind.source_port())
                        && connection.order == expected_order as i64
                        && self
                            .port_definition(&connection.from, PortDirection::Output)
                            .is_some_and(|definition| definition.data_type == kind.data_type())
                },
            )
    }

    pub(in crate::model::project) fn structural_merge_reaches_output(
        &self,
        container: NodeContainer,
        output_node_id: Uuid,
        connections: &[ProjectConnection],
    ) -> bool {
        self.structural_merge_reaches_output_for(
            container,
            output_node_id,
            connections,
            StructuralMergeKind::Image,
        )
    }

    pub(in crate::model::project) fn structural_sound_merge_reaches_output(
        &self,
        container: NodeContainer,
        output_node_id: Uuid,
        connections: &[ProjectConnection],
    ) -> bool {
        self.structural_merge_reaches_output_for(
            container,
            output_node_id,
            connections,
            StructuralMergeKind::Sound,
        )
    }

    pub(super) fn structural_merge_reaches_output_for(
        &self,
        container: NodeContainer,
        output_node_id: Uuid,
        connections: &[ProjectConnection],
        kind: StructuralMergeKind,
    ) -> bool {
        let Some(structural_merge_id) = self.structural_merge_id_for(container, kind) else {
            return false;
        };
        if structural_merge_id == output_node_id {
            return true;
        }
        let mut pending = vec![output_node_id];
        let mut visited = HashSet::new();
        while let Some(target_node_id) = pending.pop() {
            if !visited.insert(target_node_id) {
                continue;
            }
            for connection in connections
                .iter()
                .filter(|connection| connection.to.owner == PortOwner::Node(target_node_id))
            {
                let source_matches = self
                    .port_definition(&connection.from, PortDirection::Output)
                    .is_some_and(|port| port.data_type == kind.data_type());
                let target_matches = self
                    .port_definition(&connection.to, PortDirection::Input)
                    .is_some_and(|port| port.data_type == kind.data_type());
                if !source_matches || !target_matches {
                    continue;
                }
                let PortOwner::Node(source_node_id) = connection.from.owner else {
                    continue;
                };
                if source_node_id == structural_merge_id {
                    return true;
                }
                pending.push(source_node_id);
            }
        }
        false
    }

    pub(super) fn structural_merge_target_for(
        &self,
        container: NodeContainer,
        kind: StructuralMergeKind,
    ) -> Option<PortAddress> {
        self.structural_merge_id_for(container, kind)
            .map(|node_id| PortAddress::new(PortOwner::Node(node_id), kind.input_port()))
    }

    pub(super) fn structural_child_owners(&self, container: NodeContainer) -> Vec<PortOwner> {
        match container {
            NodeContainer::Composition(id) => self
                .get_composition(id)
                .map(|composition| {
                    composition
                        .track_ids
                        .iter()
                        .copied()
                        .map(PortOwner::Track)
                        .collect()
                })
                .unwrap_or_default(),
            NodeContainer::Track(id) => self
                .get_track(id)
                .map(|track| {
                    track
                        .clip_ids
                        .iter()
                        .copied()
                        .map(PortOwner::Clip)
                        .collect()
                })
                .unwrap_or_default(),
            NodeContainer::Clip(_) => Vec::new(),
        }
    }

    pub(super) fn structural_connection_id(
        &self,
        target: &PortAddress,
        child: PortOwner,
    ) -> Option<Uuid> {
        let source_port = match target.port.as_str() {
            MERGE_IMAGES_PORT => IMAGE_OUTPUT_PORT,
            MERGE_SOUNDS_PORT => AUDIO_OUTPUT_PORT,
            _ => return None,
        };
        let source = PortAddress::new(child, source_port);
        self.connections
            .iter()
            .find(|connection| connection.from == source && connection.to == *target)
            .map(|connection| connection.id)
    }

    pub(in crate::model::project) fn container_for_structural_target(
        &self,
        target: &PortAddress,
    ) -> Option<NodeContainer> {
        if !matches!(target.port.as_str(), MERGE_IMAGES_PORT | MERGE_SOUNDS_PORT) {
            return None;
        }
        let PortOwner::Node(node_id) = target.owner else {
            return None;
        };
        self.compositions
            .iter()
            .find(|composition| {
                (target.port == MERGE_IMAGES_PORT
                    && composition.structural_merge_node_id == node_id)
                    || (target.port == MERGE_SOUNDS_PORT
                        && composition.structural_sound_merge_node_id == node_id)
            })
            .map(|composition| NodeContainer::Composition(composition.id))
            .or_else(|| {
                self.tracks
                    .values()
                    .find(|track| {
                        (target.port == MERGE_IMAGES_PORT
                            && track.structural_merge_node_id == node_id)
                            || (target.port == MERGE_SOUNDS_PORT
                                && track.structural_sound_merge_node_id == node_id)
                    })
                    .map(|track| NodeContainer::Track(track.id))
            })
    }

    pub(in crate::model::project) fn structural_custom_insertion_error(
        &self,
        target: &PortAddress,
        source: &PortAddress,
        insertion_order: usize,
    ) -> Option<ProjectGraphError> {
        let container = self.container_for_structural_target(target)?;
        let source_port = match target.port.as_str() {
            MERGE_IMAGES_PORT => IMAGE_OUTPUT_PORT,
            MERGE_SOUNDS_PORT => AUDIO_OUTPUT_PORT,
            _ => return None,
        };
        let children = self.structural_child_owners(container);
        if children
            .iter()
            .any(|child| *source == PortAddress::new(*child, source_port))
            || insertion_order >= children.len()
        {
            return None;
        }
        let PortOwner::Node(node_id) = target.owner else {
            return None;
        };
        Some(ProjectGraphError::StructuralOrderMismatch {
            container,
            node_id,
            child: children[insertion_order],
            expected_order: insertion_order as i64,
            actual_order: insertion_order as i64 + 1,
        })
    }
}

pub(super) struct StructuralMergePairSpec<'a> {
    pub(super) image_id: Uuid,
    pub(super) image_name: &'a str,
    pub(super) sound_id: Uuid,
    pub(super) sound_name: &'a str,
    pub(super) container_position: [f32; 2],
    pub(super) container_size: [f32; 2],
    pub(super) after_child_right: Option<f32>,
}

pub(super) fn structural_merge_pair(spec: StructuralMergePairSpec<'_>) -> (Node, Node) {
    let mut image = Node::new_merge(spec.image_name);
    image.id = spec.image_id;
    let container_left = spec.container_position[0];
    let minimum_left = container_left + DEFAULT_GRAPH_CONTENT_INSET;
    let maximum_left =
        (container_left + spec.container_size[0] - DEFAULT_GRAPH_CONTENT_INSET - image.ui_size[0])
            .max(minimum_left);
    let left = spec
        .after_child_right
        .map_or(maximum_left, |right| right + DEFAULT_GRAPH_ITEM_GAP)
        .clamp(minimum_left, maximum_left);
    let top = spec.container_position[1] + DEFAULT_GRAPH_CONTENT_INSET;
    image.ui_position = [left, top];

    let mut sound = Node::new_sound_merge(spec.sound_name);
    sound.id = spec.sound_id;
    sound.ui_position = [left, top + image.ui_size[1] + DEFAULT_GRAPH_ITEM_GAP];
    (image, sound)
}

pub(super) fn container_owner(container: NodeContainer) -> PortOwner {
    match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}
