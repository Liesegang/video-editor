use super::super::{NodeContainer, Project};
use super::{
    AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_PORT, PortAddress, PortDataType, PortDirection, PortOwner,
    ProjectConnection,
};
use uuid::Uuid;

/// Why a container exposes a particular owner as its image output.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ContainerImageSourceKind {
    /// The container's `output_node_id` selects one directly contained Node.
    OutputBinding,
}

/// One ordered dependency of a Composition, Track, or Clip image output.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ContainerImageSource {
    pub source: PortOwner,
    pub kind: ContainerImageSourceKind,
}

/// Why a container exposes a particular owner as part of its audio output.
/// This is a derived graph projection, not persisted project state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ContainerAudioSourceKind {
    /// The container's `audio_output_node_id` selects one directly contained
    /// Node; Track and Composition bindings must remain downstream of their
    /// canonical structural Sound Merge.
    OutputBinding,
}

/// One ordered dependency of a Composition, Track, or Clip audio output.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ContainerAudioSource {
    pub source: PortOwner,
    pub kind: ContainerAudioSourceKind,
}

impl Project {
    pub(super) fn container_output_node_id(&self, owner: PortOwner) -> Option<Uuid> {
        match owner {
            PortOwner::Composition(id) => self
                .get_composition(id)
                .and_then(|composition| composition.output_node_id),
            PortOwner::Track(id) => self.get_track(id).and_then(|track| track.output_node_id),
            PortOwner::Clip(id) => self.get_clip(id).and_then(|clip| clip.output_node_id),
            PortOwner::Node(_) => None,
        }
    }

    pub(in crate::model::project) fn container_directly_contains_node(
        &self,
        owner: PortOwner,
        node_id: Uuid,
    ) -> bool {
        match owner {
            PortOwner::Composition(id) => self
                .get_composition(id)
                .is_some_and(|composition| composition.node_ids.contains(&node_id)),
            PortOwner::Track(id) => self
                .get_track(id)
                .is_some_and(|track| track.node_ids.contains(&node_id)),
            PortOwner::Clip(id) => self
                .get_clip(id)
                .is_some_and(|clip| clip.node_ids.contains(&node_id)),
            PortOwner::Node(_) => false,
        }
    }

    /// Return the authoritative image dependency for a container. Timeline
    /// children enter Track and Composition output only through persisted
    /// connections to their annotated structural Merge Nodes; there is no
    /// derived child fallback.
    pub fn container_image_sources(&self, owner: PortOwner) -> Vec<ContainerImageSource> {
        self.container_image_sources_with_connections(owner, &self.connections)
    }

    /// Return the authoritative, ordered audio dependencies for a container.
    ///
    /// Track and Composition children enter the output only through their
    /// canonical structural Sound Merge. A Clip has one explicit typed output
    /// binding. Missing or malformed bindings produce NoOutput; there is no
    /// hidden child-mixing fallback.
    pub fn container_audio_sources(&self, owner: PortOwner) -> Vec<ContainerAudioSource> {
        let bound = |node_id| {
            vec![ContainerAudioSource {
                source: PortOwner::Node(node_id),
                kind: ContainerAudioSourceKind::OutputBinding,
            }]
        };

        let (container, output_node_id, requires_structural_merge) = match owner {
            PortOwner::Composition(id) => {
                let Some(composition) = self.get_composition(id) else {
                    return Vec::new();
                };
                (
                    NodeContainer::Composition(id),
                    composition.audio_output_node_id,
                    true,
                )
            }
            PortOwner::Track(id) => {
                let Some(track) = self.get_track(id) else {
                    return Vec::new();
                };
                (NodeContainer::Track(id), track.audio_output_node_id, true)
            }
            PortOwner::Clip(id) => {
                let Some(clip) = self.get_clip(id) else {
                    return Vec::new();
                };
                (NodeContainer::Clip(id), clip.audio_output_node_id, false)
            }
            PortOwner::Node(_) => return Vec::new(),
        };
        if requires_structural_merge && !self.structural_sound_merge_is_well_formed(container) {
            return Vec::new();
        }
        let Some(output_node_id) = output_node_id else {
            return Vec::new();
        };
        if !self.container_directly_contains_node(owner, output_node_id) {
            return Vec::new();
        }
        if requires_structural_merge
            && !self.structural_sound_merge_reaches_output(
                container,
                output_node_id,
                &self.connections,
            )
        {
            return Vec::new();
        }
        bound(output_node_id)
    }

    pub(super) fn container_image_sources_with_connections(
        &self,
        owner: PortOwner,
        connections: &[ProjectConnection],
    ) -> Vec<ContainerImageSource> {
        let bound = |node_id| {
            vec![ContainerImageSource {
                source: PortOwner::Node(node_id),
                kind: ContainerImageSourceKind::OutputBinding,
            }]
        };

        let (container, output_node_id, requires_structural_merge) = match owner {
            PortOwner::Composition(id) => {
                let Some(composition) = self.get_composition(id) else {
                    return Vec::new();
                };
                (
                    NodeContainer::Composition(id),
                    composition.output_node_id,
                    true,
                )
            }
            PortOwner::Track(id) => {
                let Some(track) = self.get_track(id) else {
                    return Vec::new();
                };
                (NodeContainer::Track(id), track.output_node_id, true)
            }
            PortOwner::Clip(id) => {
                let Some(clip) = self.get_clip(id) else {
                    return Vec::new();
                };
                (NodeContainer::Clip(id), clip.output_node_id, false)
            }
            PortOwner::Node(_) => return Vec::new(),
        };
        if requires_structural_merge && !self.structural_merge_is_well_formed(container) {
            return Vec::new();
        }
        let Some(output_node_id) = output_node_id else {
            return Vec::new();
        };
        if !self.container_directly_contains_node(owner, output_node_id) {
            return Vec::new();
        }
        if requires_structural_merge
            && !self.structural_merge_reaches_output(container, output_node_id, connections)
        {
            return Vec::new();
        }
        bound(output_node_id)
    }

    pub(super) fn owner_has_image_output(&self, owner: PortOwner) -> bool {
        let address = PortAddress::new(owner, IMAGE_OUTPUT_PORT);
        self.port_definition(&address, PortDirection::Output)
            .is_some_and(|port| port.data_type == PortDataType::Image)
    }

    pub(super) fn owner_has_audio_output(&self, owner: PortOwner) -> bool {
        let address = PortAddress::new(owner, AUDIO_OUTPUT_PORT);
        self.port_definition(&address, PortDirection::Output)
            .is_some_and(|port| port.data_type == PortDataType::Audio)
    }
}
