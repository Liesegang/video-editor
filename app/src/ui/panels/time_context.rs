//! Read-only presentation of the runtime Time scope.
//!
//! Runtime inheritance remains owned by `EvaluationScope`. This module only
//! describes that existing scope for UI surfaces; it never authors a graph
//! connection or stores derived state in `Project`.

use library::model::project::{PortAddress, PortDirection, PortOwner, Project, TIME_PORT};
use library::model::NodeContainer;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TimeSourceState {
    Explicit {
        connection_id: Uuid,
        from: PortAddress,
    },
    Inherited {
        from: PortAddress,
    },
    Timeline {
        composition_id: Uuid,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TimeSourcePresentation {
    pub(crate) label: String,
    pub(crate) tooltip: String,
}

/// Describe the effective Time source for an owner that declares the
/// canonical `time` input. Generic numeric Nodes such as Fmod deliberately
/// return `None`, even when they happen to live inside a timed container.
pub(crate) fn time_source_state(project: &Project, owner: PortOwner) -> Option<TimeSourceState> {
    let target = PortAddress::new(owner, TIME_PORT);
    project.port_definition(&target, PortDirection::Input)?;

    if let Some(connection) = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .min_by_key(|connection| (connection.order, connection.id))
    {
        return Some(TimeSourceState::Explicit {
            connection_id: connection.id,
            from: connection.from.clone(),
        });
    }

    match owner {
        PortOwner::Composition(composition_id) => project
            .get_composition(composition_id)
            .map(|_| TimeSourceState::Timeline { composition_id }),
        PortOwner::Track(track_id) => {
            project
                .find_composition_for_track(track_id)
                .map(|composition_id| TimeSourceState::Inherited {
                    from: PortAddress::new(PortOwner::Composition(composition_id), TIME_PORT),
                })
        }
        PortOwner::Clip(clip_id) => {
            project
                .find_track_for_clip(clip_id)
                .map(|track_id| TimeSourceState::Inherited {
                    from: PortAddress::new(PortOwner::Track(track_id), TIME_PORT),
                })
        }
        PortOwner::Node(node_id) => project.find_node_container(node_id).map(|container| {
            let owner = match container {
                NodeContainer::Composition(id) => PortOwner::Composition(id),
                NodeContainer::Track(id) => PortOwner::Track(id),
                NodeContainer::Clip(id) => PortOwner::Clip(id),
            };
            TimeSourceState::Inherited {
                from: PortAddress::new(owner, TIME_PORT),
            }
        }),
    }
}

impl TimeSourceState {
    pub(crate) fn presentation(&self, project: &Project) -> TimeSourcePresentation {
        match self {
            Self::Explicit { from, .. } => {
                let source = source_label(project, from);
                TimeSourcePresentation {
                    label: format!("Explicit · {source}"),
                    tooltip: format!("Time is supplied by the authored connection from {source}."),
                }
            }
            Self::Inherited { from } => {
                let container = owner_label(project, from.owner);
                TimeSourcePresentation {
                    label: format!("Inherited · {container}"),
                    tooltip: format!(
                        "No explicit Time connection. Runtime Time is inherited from the immediate {container} container."
                    ),
                }
            }
            Self::Timeline { composition_id } => {
                let composition = owner_label(project, PortOwner::Composition(*composition_id));
                TimeSourcePresentation {
                    label: format!("Timeline · {composition}"),
                    tooltip: format!(
                        "The root {composition} uses the current Timeline time unless its Time input is explicitly connected."
                    ),
                }
            }
        }
    }

    pub(crate) const fn kind_key(&self) -> &'static str {
        match self {
            Self::Explicit { .. } => "explicit",
            Self::Inherited { .. } => "inherited",
            Self::Timeline { .. } => "timeline",
        }
    }

    pub(crate) fn source(&self) -> Option<&PortAddress> {
        match self {
            Self::Explicit { from, .. } | Self::Inherited { from } => Some(from),
            Self::Timeline { .. } => None,
        }
    }

    pub(crate) const fn connection_id(&self) -> Option<Uuid> {
        match self {
            Self::Explicit { connection_id, .. } => Some(*connection_id),
            Self::Inherited { .. } | Self::Timeline { .. } => None,
        }
    }

    pub(crate) fn qa_metadata(&self, target: PortOwner) -> serde_json::Value {
        serde_json::json!({
            "state": self.kind_key(),
            "editable": false,
            "target": {
                "owner": target,
                "port": TIME_PORT,
            },
            "source": self.source().map(|source| serde_json::json!({
                "owner": source.owner,
                "port": source.port,
            })),
            "connection_id": self.connection_id(),
            "runtime_authority": "evaluation_scope",
            "presentation_persisted": false,
            "derived_state_persisted": false,
            "authored_connection_persisted": matches!(self, Self::Explicit { .. }),
        })
    }
}

fn source_label(project: &Project, address: &PortAddress) -> String {
    let owner = owner_label(project, address.owner);
    let port = project
        .port_definition(address, PortDirection::Output)
        .map_or_else(|| address.port.clone(), |definition| definition.label);
    format!("{owner}.{port}")
}

fn owner_label(project: &Project, owner: PortOwner) -> String {
    match owner {
        PortOwner::Composition(id) => project.get_composition(id).map_or_else(
            || "Missing Composition".to_string(),
            |composition| format!("Composition {}", composition.name),
        ),
        PortOwner::Track(id) => project.get_track(id).map_or_else(
            || "Missing Track".to_string(),
            |track| format!("Track {}", track.name),
        ),
        PortOwner::Clip(id) => project.get_clip(id).map_or_else(
            || "Missing Clip".to_string(),
            |clip| format!("Clip {}", clip.name),
        ),
        PortOwner::Node(id) => project.get_node(id).map_or_else(
            || "Missing Node".to_string(),
            |node| format!("Node {}", node.name),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::HistoryManager;
    use library::model::{Clip, Composition, Node, Project, ValueContent};

    struct Fixture {
        project: Project,
        composition: Uuid,
        track: Uuid,
        clip: Uuid,
        timed_node: Uuid,
        fmod: Uuid,
    }

    fn fixture() -> Fixture {
        let mut project = Project::new("Time context");
        let (composition, track) = Composition::new("Main", 640, 360, 30.0, 8.0);
        let composition_id = composition.id;
        let track_id = track.id;
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );

        let clip = Clip::new("Opening", 0.0, 4.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let timed = Node::new_merge("Composite");
        let timed_node = timed.id;
        project.add_node(timed);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), timed_node)
            .unwrap();

        let fmod_node = Node::new_value("Fmod", ValueContent::Fmod);
        let fmod = fmod_node.id;
        project.add_node(fmod_node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), fmod)
            .unwrap();

        Fixture {
            project,
            composition: composition_id,
            track: track_id,
            clip: clip_id,
            timed_node,
            fmod,
        }
    }

    #[test]
    fn query_follows_immediate_runtime_scope_and_explicit_override() {
        let mut fixture = fixture();
        assert_eq!(
            time_source_state(
                &fixture.project,
                PortOwner::Composition(fixture.composition)
            ),
            Some(TimeSourceState::Timeline {
                composition_id: fixture.composition,
            })
        );
        assert_eq!(
            time_source_state(&fixture.project, PortOwner::Track(fixture.track)),
            Some(TimeSourceState::Inherited {
                from: PortAddress::new(PortOwner::Composition(fixture.composition), TIME_PORT,),
            })
        );
        assert_eq!(
            time_source_state(&fixture.project, PortOwner::Clip(fixture.clip)),
            Some(TimeSourceState::Inherited {
                from: PortAddress::new(PortOwner::Track(fixture.track), TIME_PORT),
            })
        );
        assert_eq!(
            time_source_state(&fixture.project, PortOwner::Node(fixture.timed_node)),
            Some(TimeSourceState::Inherited {
                from: PortAddress::new(PortOwner::Clip(fixture.clip), TIME_PORT),
            })
        );
        assert_eq!(
            time_source_state(&fixture.project, PortOwner::Node(fixture.fmod)),
            None,
            "generic Fmod has no canonical implicit Time input"
        );

        let connection_id = fixture
            .project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(fixture.clip), TIME_PORT),
                PortAddress::new(PortOwner::Node(fixture.timed_node), TIME_PORT),
            )
            .unwrap();
        assert_eq!(
            time_source_state(&fixture.project, PortOwner::Node(fixture.timed_node)),
            Some(TimeSourceState::Explicit {
                connection_id,
                from: PortAddress::new(PortOwner::Clip(fixture.clip), TIME_PORT),
            })
        );
    }

    #[test]
    fn query_and_presentation_do_not_mutate_serialization_or_history() {
        let fixture = fixture();
        let project_before = fixture.project.clone();
        let json_before = serde_json::to_value(&fixture.project).unwrap();
        let mut history = HistoryManager::new();
        history.push_project_state(fixture.project.clone());
        let undo_before = history.undo_depth();
        let redo_before = history.redo_depth();

        let state =
            time_source_state(&fixture.project, PortOwner::Node(fixture.timed_node)).unwrap();
        let presentation = state.presentation(&fixture.project);
        let metadata = state.qa_metadata(PortOwner::Node(fixture.timed_node));

        assert!(presentation.label.starts_with("Inherited · Clip Opening"));
        assert_eq!(metadata["runtime_authority"], "evaluation_scope");
        assert_eq!(metadata["presentation_persisted"], false);
        assert_eq!(metadata["derived_state_persisted"], false);
        assert_eq!(fixture.project, project_before);
        assert_eq!(serde_json::to_value(&fixture.project).unwrap(), json_before);
        assert_eq!(fixture.project.connections, project_before.connections);
        assert_eq!(history.undo_depth(), undo_before);
        assert_eq!(history.redo_depth(), redo_before);
    }
}
