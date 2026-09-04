use super::super::{NodeContainer, PortAddress, Project, ProjectGraphError};
use super::contract::{StructuralMergeKind, container_owner};

impl Project {
    pub(in crate::model::project) fn validate_structural_merges(&self) -> Vec<ProjectGraphError> {
        let containers = self
            .compositions
            .iter()
            .map(|composition| NodeContainer::Composition(composition.id))
            .chain(
                self.tracks
                    .values()
                    .map(|track| NodeContainer::Track(track.id)),
            );
        let mut errors = Vec::new();
        for container in containers {
            for kind in StructuralMergeKind::ALL {
                self.validate_structural_merge_for(container, kind, &mut errors);
            }
        }
        errors
    }

    fn validate_structural_merge_for(
        &self,
        container: NodeContainer,
        kind: StructuralMergeKind,
        errors: &mut Vec<ProjectGraphError>,
    ) {
        let Some(node_id) = self.structural_merge_id_for(container, kind) else {
            return;
        };
        let Some(node) = self.get_node(node_id) else {
            errors.push(ProjectGraphError::StructuralMergeNodeMissing { container, node_id });
            return;
        };
        if !self.container_directly_contains_node(container_owner(container), node_id) {
            errors.push(ProjectGraphError::StructuralMergeNodeOutsideContainer {
                container,
                node_id,
            });
            return;
        }
        if !kind.node_matches(node) {
            errors.push(ProjectGraphError::StructuralMergeNodeWrongType { container, node_id });
            return;
        }

        let output_node_id = match (container, kind) {
            (NodeContainer::Composition(id), StructuralMergeKind::Image) => self
                .get_composition(id)
                .and_then(|composition| composition.output_node_id),
            (NodeContainer::Composition(id), StructuralMergeKind::Sound) => self
                .get_composition(id)
                .and_then(|composition| composition.audio_output_node_id),
            (NodeContainer::Track(id), StructuralMergeKind::Image) => {
                self.get_track(id).and_then(|track| track.output_node_id)
            }
            (NodeContainer::Track(id), StructuralMergeKind::Sound) => self
                .get_track(id)
                .and_then(|track| track.audio_output_node_id),
            (NodeContainer::Clip(_), _) => None,
        };
        if let Some(output_node_id) = output_node_id
            && self.find_node_container(output_node_id) == Some(container)
            && !self.structural_merge_reaches_output_for(
                container,
                output_node_id,
                &self.connections,
                kind,
            )
        {
            errors.push(ProjectGraphError::StructuralMergeDoesNotReachOutput {
                container,
                node_id,
                output_node_id,
            });
        }

        let Some(target) = self.structural_merge_target_for(container, kind) else {
            return;
        };
        // A parent may be inserted before a prelisted child. Containment
        // validation reports that missing entity; structural edges become
        // mandatory as soon as the child exists and exposes its typed port.
        let children = self
            .structural_child_owners(container)
            .into_iter()
            .filter(|child| {
                self.port_definition(
                    &PortAddress::new(*child, kind.source_port()),
                    super::super::PortDirection::Output,
                )
                .is_some()
            })
            .collect::<Vec<_>>();
        for (expected_order, child) in children.into_iter().enumerate() {
            let source = PortAddress::new(child, kind.source_port());
            let matching = self
                .connections
                .iter()
                .filter(|connection| connection.to == target && connection.from == source)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                errors.push(ProjectGraphError::MissingStructuralEdge {
                    container,
                    node_id,
                    child,
                });
            } else if matching.len() > 1 {
                errors.push(ProjectGraphError::DuplicateStructuralChildEdge {
                    container,
                    node_id,
                    child,
                });
            } else if matching[0].order != expected_order as i64 {
                errors.push(ProjectGraphError::StructuralOrderMismatch {
                    container,
                    node_id,
                    child,
                    expected_order: expected_order as i64,
                    actual_order: matching[0].order,
                });
            }
        }
    }
}
