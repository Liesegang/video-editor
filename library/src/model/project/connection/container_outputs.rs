use super::*;

/// Why a container exposes a particular owner as part of its image output.
/// This is a derived graph projection, not persisted project state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ContainerImageSourceKind {
    /// The container's `output_node_id` selects one directly contained Node.
    OutputBinding,
    /// No output binding exists, so the ordered direct children are composed.
    DerivedChild,
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
    /// The container's `audio_output_node_id` selects one directly contained Node.
    OutputBinding,
    /// No audio output binding exists, so ordered direct child containers mix.
    DerivedChild,
}

/// One ordered dependency of a Composition, Track, or Clip audio output.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ContainerAudioSource {
    pub source: PortOwner,
    pub kind: ContainerAudioSourceKind,
}

impl Project {
    /// Return the authoritative, ordered image dependencies for a container.
    ///
    /// An explicit output binding always replaces fallback composition. Without
    /// one, a Composition derives from its ordered Tracks and a Track derives
    /// from its ordered Clips. Direct Nodes are an internal graph implementation
    /// detail and never become an implicit image output; a Clip therefore needs
    /// an explicit output binding. Missing owners and leaf Nodes have no sources.
    pub fn container_image_sources(&self, owner: PortOwner) -> Vec<ContainerImageSource> {
        self.container_image_sources_with_connections(owner, &self.connections)
    }

    /// Return the authoritative, ordered audio dependencies for a container.
    ///
    /// An explicit Audio binding replaces fallback mixing. Without one, a
    /// Composition derives from ordered Tracks and a Track derives from
    /// ordered Clips. A Clip with no binding has a stable typed Audio port
    /// whose runtime value is NoOutput.
    pub fn container_audio_sources(&self, owner: PortOwner) -> Vec<ContainerAudioSource> {
        let derived = |source| ContainerAudioSource {
            source,
            kind: ContainerAudioSourceKind::DerivedChild,
        };
        let bound = |node_id| {
            vec![ContainerAudioSource {
                source: PortOwner::Node(node_id),
                kind: ContainerAudioSourceKind::OutputBinding,
            }]
        };

        let (container, output_node_id) = match owner {
            PortOwner::Composition(id) => {
                let Some(composition) = self.get_composition(id) else {
                    return Vec::new();
                };
                (
                    NodeContainer::Composition(id),
                    composition.audio_output_node_id,
                )
            }
            PortOwner::Track(id) => {
                let Some(track) = self.get_track(id) else {
                    return Vec::new();
                };
                (NodeContainer::Track(id), track.audio_output_node_id)
            }
            PortOwner::Clip(id) => {
                let Some(clip) = self.get_clip(id) else {
                    return Vec::new();
                };
                (NodeContainer::Clip(id), clip.audio_output_node_id)
            }
            PortOwner::Node(_) => return Vec::new(),
        };
        if let Some(output_node_id) = output_node_id {
            if !self.container_directly_contains_node(owner, output_node_id) {
                return Vec::new();
            }
            return bound(output_node_id);
        }

        self.direct_child_owners(container)
            .into_iter()
            .filter(|source| self.owner_has_audio_output(*source))
            .map(derived)
            .collect()
    }

    pub(super) fn container_image_sources_with_connections(
        &self,
        owner: PortOwner,
        _connections: &[ProjectConnection],
    ) -> Vec<ContainerImageSource> {
        let derived = |source| ContainerImageSource {
            source,
            kind: ContainerImageSourceKind::DerivedChild,
        };
        let bound = |node_id| {
            vec![ContainerImageSource {
                source: PortOwner::Node(node_id),
                kind: ContainerImageSourceKind::OutputBinding,
            }]
        };

        let (container, output_node_id) = match owner {
            PortOwner::Composition(id) => {
                let Some(composition) = self.get_composition(id) else {
                    return Vec::new();
                };
                (NodeContainer::Composition(id), composition.output_node_id)
            }
            PortOwner::Track(id) => {
                let Some(track) = self.get_track(id) else {
                    return Vec::new();
                };
                (NodeContainer::Track(id), track.output_node_id)
            }
            PortOwner::Clip(id) => {
                let Some(clip) = self.get_clip(id) else {
                    return Vec::new();
                };
                (NodeContainer::Clip(id), clip.output_node_id)
            }
            PortOwner::Node(_) => return Vec::new(),
        };
        if let Some(output_node_id) = output_node_id {
            return bound(output_node_id);
        }

        self.direct_child_owners(container)
            .into_iter()
            .filter(|source| self.owner_has_image_output(*source))
            .map(derived)
            .collect()
    }

    fn direct_child_owners(&self, container: NodeContainer) -> Vec<PortOwner> {
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
