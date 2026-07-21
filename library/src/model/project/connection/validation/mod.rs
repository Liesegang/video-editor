mod composition_instances;

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::model::{BlendMode, NodeContent, native_node_descriptor};

use super::super::{NodeContainer, Project, ProjectGraphError};
use super::ports::is_graph_connectable_type;
use super::{
    DURATION_PORT, MERGE_IMAGES_PORT, PortAddress, PortDataType, PortDirection, PortExposure,
    PortMultiplicity, PortOwner, ProjectConnection, RESOLUTION_PORT, TIME_PORT,
};

impl Project {
    pub fn validate_connections(&self) -> Vec<ProjectGraphError> {
        let mut errors = self.validate_containment();
        errors.extend(self.validate_composition_instances());
        errors.extend(self.validate_plugin_operation_contracts());
        errors.extend(self.validate_native_catalog_contracts());
        errors.extend(self.validate_container_output_ports());
        let mut targets: HashMap<&PortAddress, Vec<&ProjectConnection>> = HashMap::new();
        for connection in &self.connections {
            targets.entry(&connection.to).or_default().push(connection);
            errors.extend(self.validate_connection(connection));
        }
        for (target, mut inputs) in targets {
            let Some(definition) = self.port_definition(target, PortDirection::Input) else {
                continue;
            };
            match definition.multiplicity {
                PortMultiplicity::Single if inputs.len() > 1 => {
                    errors.push(ProjectGraphError::DuplicateInputConnection {
                        target: target.clone(),
                    })
                }
                PortMultiplicity::Variadic => {
                    inputs.sort_by_key(|item| (item.order, item.id));
                    for pair in inputs.windows(2) {
                        if pair[0].order == pair[1].order {
                            errors.push(ProjectGraphError::DuplicateConnectionOrder {
                                target: target.clone(),
                                order: pair[0].order,
                            });
                        }
                    }
                    let mut sources = HashSet::new();
                    for (expected_order, input) in inputs.into_iter().enumerate() {
                        if input.order != expected_order as i64 {
                            errors.push(ProjectGraphError::NonCanonicalConnectionOrder {
                                target: target.clone(),
                                expected_order: expected_order as i64,
                                actual_order: input.order,
                            });
                        }
                        if !sources.insert(&input.from) {
                            errors.push(ProjectGraphError::DuplicateVariadicConnection {
                                target: target.clone(),
                                from: input.from.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        errors
    }

    fn validate_container_output_ports(&self) -> Vec<ProjectGraphError> {
        let image_errors = self
            .compositions
            .iter()
            .filter_map(|composition| {
                composition
                    .output_node_id
                    .map(|node_id| (NodeContainer::Composition(composition.id), node_id))
            })
            .chain(self.tracks.values().filter_map(|track| {
                track
                    .output_node_id
                    .map(|node_id| (NodeContainer::Track(track.id), node_id))
            }))
            .chain(self.clips.values().filter_map(|clip| {
                clip.output_node_id
                    .map(|node_id| (NodeContainer::Clip(clip.id), node_id))
            }))
            .filter_map(|(container, node_id)| {
                self.get_node(node_id)
                    .is_some_and(|_| !self.owner_has_image_output(PortOwner::Node(node_id)))
                    .then_some(ProjectGraphError::OutputNodeHasNoImagePort { node_id, container })
            })
            .collect::<Vec<_>>();
        let audio_errors = self
            .compositions
            .iter()
            .filter_map(|composition| {
                composition
                    .audio_output_node_id
                    .map(|node_id| (NodeContainer::Composition(composition.id), node_id))
            })
            .chain(self.tracks.values().filter_map(|track| {
                track
                    .audio_output_node_id
                    .map(|node_id| (NodeContainer::Track(track.id), node_id))
            }))
            .chain(self.clips.values().filter_map(|clip| {
                clip.audio_output_node_id
                    .map(|node_id| (NodeContainer::Clip(clip.id), node_id))
            }))
            .filter_map(|(container, node_id)| {
                self.get_node(node_id)
                    .is_some_and(|_| !self.owner_has_audio_output(PortOwner::Node(node_id)))
                    .then_some(ProjectGraphError::OutputNodeHasNoAudioPort { node_id, container })
            });
        image_errors.into_iter().chain(audio_errors).collect()
    }

    fn validate_plugin_operation_contracts(&self) -> Vec<ProjectGraphError> {
        let mut errors = Vec::new();
        for node in self.nodes.values() {
            let NodeContent::PluginOperation(operation) = node.content() else {
                continue;
            };
            for (field, value) in [
                ("category", operation.category.as_str()),
                ("component_id", operation.component_id.as_str()),
                ("operation", operation.operation.as_str()),
            ] {
                if value.trim().is_empty() {
                    errors.push(ProjectGraphError::EmptyPluginOperationIdentifier {
                        node_id: node.id,
                        field: field.to_string(),
                    });
                }
            }

            let mut port_keys = HashSet::new();
            // PluginOperation ports are a complete persisted execution
            // contract. Metadata such as Time must be declared explicitly by
            // the operation; injecting a hidden catalog here would create a
            // second port authority and duplicate the persisted Time input.
            for port in operation.declared_ports.iter().cloned() {
                if port.key.trim().is_empty() {
                    errors.push(ProjectGraphError::EmptyNodePortKey {
                        node_id: node.id,
                        direction: port.direction,
                    });
                    continue;
                }
                if !port_keys.insert((port.direction, port.key.clone())) {
                    errors.push(ProjectGraphError::DuplicateNodePort {
                        node_id: node.id,
                        key: port.key.clone(),
                        direction: port.direction,
                    });
                }
                if port.exposure != PortExposure::Graph {
                    errors.push(ProjectGraphError::InvalidNodePortExposure {
                        node_id: node.id,
                        key: port.key.clone(),
                        exposure: port.exposure,
                    });
                }
                if port.direction == PortDirection::Output
                    && port.multiplicity == PortMultiplicity::Variadic
                {
                    errors.push(ProjectGraphError::VariadicNodeOutput {
                        node_id: node.id,
                        key: port.key,
                    });
                }
            }
        }
        errors
    }

    fn validate_native_catalog_contracts(&self) -> Vec<ProjectGraphError> {
        self.nodes
            .values()
            .filter_map(|node| {
                let NodeContent::NativeOperation(operation) = node.content() else {
                    return None;
                };
                native_node_descriptor(&operation.catalog_id)
                    .is_none()
                    .then(|| ProjectGraphError::UnknownNativeCatalogId {
                        node_id: node.id,
                        catalog_id: operation.catalog_id.clone(),
                    })
            })
            .collect()
    }

    /// Returns structured, serializable diagnostics for the complete
    /// authoritative Project without mutating or repairing it. Besides typed
    /// connections this includes containment, entity identity, Composition
    /// settings/work-area invariants, and duplicate persistent IDs.
    pub fn validation_issues(&self) -> Vec<ProjectGraphError> {
        self.validate_connections()
    }

    pub fn validate_connection(&self, connection: &ProjectConnection) -> Vec<ProjectGraphError> {
        let mut errors = self.validate_connection_blend_mode(connection);
        let Some(source) = self.port_definition(&connection.from, PortDirection::Output) else {
            errors.push(ProjectGraphError::PortNotFound(connection.from.clone()));
            return errors;
        };
        let Some(target) = self.port_definition(&connection.to, PortDirection::Input) else {
            errors.push(ProjectGraphError::PortNotFound(connection.to.clone()));
            return errors;
        };
        if !target.data_type.accepts(source.data_type) {
            errors.push(ProjectGraphError::IncompatiblePortTypes {
                source_type: source.data_type,
                target_type: target.data_type,
            });
        }
        if source.exposure == PortExposure::Internal
            && !self.is_internal_port_target(connection.from.owner, connection.to.owner)
        {
            errors.push(ProjectGraphError::InternalPortEscapesContainer {
                source_owner: connection.from.owner,
                target_owner: connection.to.owner,
            });
        }
        if !is_graph_connectable_type(source.data_type) {
            errors.push(ProjectGraphError::UnsupportedConnectionType {
                connection_id: connection.id,
                data_type: source.data_type,
            });
        }
        if source.exposure != PortExposure::Internal
            && is_graph_connectable_type(source.data_type)
            && self.connection_is_cyclic(connection, &self.connections)
        {
            errors.push(ProjectGraphError::ConnectionCycle {
                from: connection.from.owner,
                to: connection.to.owner,
            });
        }
        errors
    }

    pub(super) fn validate_connection_blend_mode(
        &self,
        connection: &ProjectConnection,
    ) -> Vec<ProjectGraphError> {
        if connection.blend_mode == BlendMode::Normal {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let source_is_image = self
            .port_definition(&connection.from, PortDirection::Output)
            .is_some_and(|source| source.data_type == PortDataType::Image);
        if !source_is_image {
            errors.push(ProjectGraphError::ConnectionBlendRequiresImageSource {
                connection_id: connection.id,
                blend_mode: connection.blend_mode,
            });
        }

        let target_is_merge_images = connection.to.port == MERGE_IMAGES_PORT
            && matches!(
                connection.to.owner,
                PortOwner::Node(node_id)
                    if self
                        .get_node(node_id)
                        .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
            )
            && self
                .port_definition(&connection.to, PortDirection::Input)
                .is_some_and(|target| {
                    target.data_type == PortDataType::Image
                        && target.multiplicity == PortMultiplicity::Variadic
                });
        if !target_is_merge_images {
            errors.push(ProjectGraphError::ConnectionBlendRequiresMergeImagesInput {
                connection_id: connection.id,
                blend_mode: connection.blend_mode,
                target: connection.to.clone(),
            });
        }
        errors
    }

    pub(super) fn is_internal_port_target(&self, source: PortOwner, target: PortOwner) -> bool {
        match source {
            PortOwner::Composition(id) => match target {
                PortOwner::Track(track) => self.find_composition_for_track(track) == Some(id),
                PortOwner::Node(node) => {
                    self.find_node_container(node) == Some(NodeContainer::Composition(id))
                }
                _ => false,
            },
            PortOwner::Track(id) => match target {
                PortOwner::Clip(clip) => self.find_track_for_clip(clip) == Some(id),
                PortOwner::Node(node) => {
                    self.find_node_container(node) == Some(NodeContainer::Track(id))
                }
                _ => false,
            },
            PortOwner::Clip(id) => {
                matches!(target, PortOwner::Node(node) if self.find_node_container(node) == Some(NodeContainer::Clip(id)))
            }
            PortOwner::Node(_) => false,
        }
    }

    pub(super) fn connection_is_cyclic(
        &self,
        candidate: &ProjectConnection,
        connections: &[ProjectConnection],
    ) -> bool {
        if self.descendant_scope_override_is_cyclic(candidate) {
            return true;
        }
        let dependencies = self.graph_dependencies(connections, Some(candidate.id));
        is_reachable(
            candidate.from.owner,
            candidate.to.owner,
            &dependencies,
            &mut HashSet::new(),
        )
    }

    /// A Node output is evaluated in the scope inherited from its containing
    /// Clip/Track/Composition. Feeding that value back into a metadata input
    /// on the same container or one of its ancestors makes the source scope
    /// depend on itself even when the Node does not produce an Image.
    fn descendant_scope_override_is_cyclic(&self, connection: &ProjectConnection) -> bool {
        if !matches!(
            connection.to.port.as_str(),
            TIME_PORT | DURATION_PORT | RESOLUTION_PORT
        ) {
            return false;
        }
        let PortOwner::Node(source_node_id) = connection.from.owner else {
            return false;
        };
        match connection.to.owner {
            PortOwner::Composition(composition_id) => {
                self.find_containing_composition(source_node_id) == Some(composition_id)
            }
            PortOwner::Track(track_id) => self.find_parent_track(source_node_id) == Some(track_id),
            PortOwner::Clip(clip_id) => self.find_parent_clip(source_node_id) == Some(clip_id),
            PortOwner::Node(_) => false,
        }
    }

    /// Build owner-level evaluation dependencies for rendered image flow and
    /// typed/value graph connections. Container `Internal` outputs are scope
    /// metadata roots (time, frame, FPS, duration, resolution), so they do not
    /// depend on the container's rendered children and must not create a false
    /// container <-> child cycle here.
    fn graph_dependencies(
        &self,
        connections: &[ProjectConnection],
        skipped: Option<Uuid>,
    ) -> HashMap<PortOwner, Vec<PortOwner>> {
        let mut dependencies: HashMap<PortOwner, Vec<PortOwner>> = HashMap::new();
        for composition in &self.compositions {
            let owner = PortOwner::Composition(composition.id);
            let dependencies_for_owner = dependencies.entry(owner).or_default();
            dependencies_for_owner.extend(
                self.container_image_sources_with_connections(owner, connections)
                    .into_iter()
                    .map(|source| source.source),
            );
            dependencies_for_owner.extend(
                self.container_audio_sources(owner)
                    .into_iter()
                    .map(|source| source.source),
            );
        }
        for track in self.tracks.values() {
            let owner = PortOwner::Track(track.id);
            let dependencies_for_owner = dependencies.entry(owner).or_default();
            dependencies_for_owner.extend(
                self.container_image_sources_with_connections(owner, connections)
                    .into_iter()
                    .map(|source| source.source),
            );
            dependencies_for_owner.extend(
                self.container_audio_sources(owner)
                    .into_iter()
                    .map(|source| source.source),
            );
        }
        for clip in self.clips.values() {
            let owner = PortOwner::Clip(clip.id);
            let dependencies_for_owner = dependencies.entry(owner).or_default();
            dependencies_for_owner.extend(
                self.container_image_sources_with_connections(owner, connections)
                    .into_iter()
                    .map(|source| source.source),
            );
            dependencies_for_owner.extend(
                self.container_audio_sources(owner)
                    .into_iter()
                    .map(|source| source.source),
            );
        }
        for connection in connections {
            if Some(connection.id) == skipped {
                continue;
            }
            let creates_dependency = self
                .port_definition(&connection.from, PortDirection::Output)
                .is_some_and(|port| {
                    port.exposure != PortExposure::Internal
                        && is_graph_connectable_type(port.data_type)
                });
            if creates_dependency {
                dependencies
                    .entry(connection.to.owner)
                    .or_default()
                    .push(connection.from.owner);
            }
        }
        for node in self.nodes.values() {
            let NodeContent::CompositionInstance(instance) = node.content() else {
                continue;
            };
            dependencies
                .entry(PortOwner::Node(node.id))
                .or_default()
                .push(PortOwner::Composition(instance.composition_id));
        }
        dependencies
    }
}

fn is_reachable(
    current: PortOwner,
    target: PortOwner,
    dependencies: &HashMap<PortOwner, Vec<PortOwner>>,
    visited: &mut HashSet<PortOwner>,
) -> bool {
    if current == target {
        return true;
    }
    if !visited.insert(current) {
        return false;
    }
    dependencies.get(&current).is_some_and(|items| {
        items
            .iter()
            .any(|item| is_reachable(*item, target, dependencies, visited))
    })
}
