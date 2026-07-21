use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::model::NodeContent;
use crate::model::project::{NodeContainer, Project, ProjectGraphError};

impl Project {
    pub(super) fn validate_composition_instances(&self) -> Vec<ProjectGraphError> {
        let mut dependencies = HashMap::<Uuid, Vec<(Uuid, Uuid)>>::new();
        let mut errors = Vec::new();
        for node in self.nodes.values() {
            let NodeContent::CompositionInstance(instance) = node.content() else {
                continue;
            };
            match self.find_node_container(node.id) {
                Some(NodeContainer::Clip(_)) => {}
                Some(container) => {
                    errors.push(ProjectGraphError::CompositionInstanceOutsideClip {
                        node_id: node.id,
                        container,
                    });
                    continue;
                }
                // Containment validation already reports detached Nodes.
                None => continue,
            }
            if self.get_composition(instance.composition_id).is_none() {
                errors.push(ProjectGraphError::CompositionInstanceTargetNotFound {
                    node_id: node.id,
                    composition_id: instance.composition_id,
                });
                continue;
            }
            let Some(containing_composition_id) = self.find_containing_composition(node.id) else {
                continue;
            };
            dependencies
                .entry(containing_composition_id)
                .or_default()
                .push((instance.composition_id, node.id));
        }

        for (containing_composition_id, targets) in &dependencies {
            for (target_composition_id, node_id) in targets {
                if composition_dependency_reaches(
                    *target_composition_id,
                    *containing_composition_id,
                    &dependencies,
                    &mut HashSet::new(),
                ) {
                    errors.push(ProjectGraphError::CompositionInstanceCycle {
                        node_id: *node_id,
                        containing_composition_id: *containing_composition_id,
                        target_composition_id: *target_composition_id,
                    });
                }
            }
        }
        errors
    }
}

fn composition_dependency_reaches(
    current: Uuid,
    target: Uuid,
    dependencies: &HashMap<Uuid, Vec<(Uuid, Uuid)>>,
    visited: &mut HashSet<Uuid>,
) -> bool {
    if current == target {
        return true;
    }
    if !visited.insert(current) {
        return false;
    }
    dependencies.get(&current).is_some_and(|items| {
        items.iter().any(|(dependency, _)| {
            composition_dependency_reaches(*dependency, target, dependencies, visited)
        })
    })
}
