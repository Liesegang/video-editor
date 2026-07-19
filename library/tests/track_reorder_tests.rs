mod support;

use std::sync::Arc;

use library::core::framing::FrameEvaluator;
use library::editor::project_service::GeneratorNodeRequest;
use library::model::Track;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroupKind, FrameItem};
use library::model::project::{Composition, NodeContainer, Project, ProjectGraphError};
use library::plugin::properties::ConstantEvaluator;
use library::plugin::{PluginManager, PropertyEvaluatorRegistry};
use uuid::Uuid;

use support::generator_node;

fn project_with_tracks(track_names: &[&str]) -> (Project, Uuid, Vec<Uuid>) {
    assert!(!track_names.is_empty());

    let mut project = Project::new("track reorder test");
    let (mut composition, first_track) = Composition::new(track_names[0], 1920, 1080, 30.0, 10.0);
    let composition_id = composition.id;
    let mut track_ids = vec![first_track.id];
    project.add_track(first_track);

    for name in &track_names[1..] {
        let track = Track::new(name);
        track_ids.push(track.id);
        composition.track_ids.push(track.id);
        project.add_track(track);
    }
    project.add_composition(composition);

    // Keep every Track renderable so FrameEvaluator preserves one top-level
    // item per Track instead of collapsing empty aggregates into a single
    // Composition absence.
    for (index, track_id) in track_ids.iter().copied().enumerate() {
        let node = generator_node(
            &format!("solid {index}"),
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        );
        let node_id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Track(track_id), node_id)
            .unwrap();
        project
            .set_output_node(NodeContainer::Track(track_id), Some(node_id))
            .unwrap();
    }

    (project, composition_id, track_ids)
}

fn evaluated_track_order(project: &Project, composition_id: Uuid) -> Vec<Uuid> {
    let mut property_evaluators = PropertyEvaluatorRegistry::new();
    assert!(
        property_evaluators
            .register("constant", Arc::new(ConstantEvaluator))
            .is_none()
    );
    let property_evaluators = Arc::new(property_evaluators);
    let plugin_manager = Arc::new(PluginManager::default());
    let composition = project.get_composition(composition_id).unwrap();

    FrameEvaluator::new(project, composition, property_evaluators, plugin_manager)
        .evaluate(0, 1.0, None)
        .unwrap()
        .items
        .into_iter()
        .map(|item| match item {
            FrameItem::Group(group) if group.kind == FrameGroupKind::Track => group.source_id,
            other => panic!("expected a top-level Track frame group, got {other:?}"),
        })
        .collect()
}

#[test]
fn authoritative_track_move_handles_up_down_first_last_and_no_op() {
    let (mut project, composition_id, ids) = project_with_tracks(&["A", "B", "C", "D"]);

    assert!(
        project
            .move_track_within_composition(composition_id, ids[2], 0)
            .unwrap()
    );
    assert_eq!(
        project.get_composition(composition_id).unwrap().track_ids,
        vec![ids[2], ids[0], ids[1], ids[3]]
    );

    assert!(
        project
            .move_track_within_composition(composition_id, ids[2], usize::MAX)
            .unwrap()
    );
    assert_eq!(
        project.get_composition(composition_id).unwrap().track_ids,
        vec![ids[0], ids[1], ids[3], ids[2]]
    );

    assert!(
        project
            .move_track_within_composition(composition_id, ids[3], 1)
            .unwrap()
    );
    assert_eq!(
        project.get_composition(composition_id).unwrap().track_ids,
        vec![ids[0], ids[3], ids[1], ids[2]]
    );

    assert!(
        !project
            .move_track_within_composition(composition_id, ids[3], 1)
            .unwrap()
    );
}

#[test]
fn track_move_rejects_cross_composition_reparenting_without_mutation() {
    let (mut project, first_composition_id, first_ids) = project_with_tracks(&["A", "B"]);
    let (second_composition, second_track) = Composition::new("Other", 1920, 1080, 30.0, 10.0);
    let second_composition_id = second_composition.id;
    let second_track_id = second_track.id;
    project.add_track(second_track);
    project.add_composition(second_composition);

    let before_first = project
        .get_composition(first_composition_id)
        .unwrap()
        .track_ids
        .clone();
    let before_second = project
        .get_composition(second_composition_id)
        .unwrap()
        .track_ids
        .clone();

    assert_eq!(
        project.move_track_within_composition(first_composition_id, second_track_id, 0),
        Err(ProjectGraphError::TrackNotInComposition {
            track_id: second_track_id,
            composition_id: first_composition_id,
        })
    );
    assert_eq!(
        project
            .get_composition(first_composition_id)
            .unwrap()
            .track_ids,
        before_first
    );
    assert_eq!(
        project
            .get_composition(second_composition_id)
            .unwrap()
            .track_ids,
        before_second
    );
    assert_eq!(first_ids, before_first);
}

#[test]
fn frame_evaluation_observes_track_order_immediately_after_the_atomic_move() {
    let (mut project, composition_id, ids) = project_with_tracks(&["Bottom", "Middle", "Top"]);
    assert_eq!(evaluated_track_order(&project, composition_id), ids);

    assert!(
        project
            .move_track_within_composition(composition_id, ids[2], 0)
            .unwrap()
    );

    assert_eq!(
        evaluated_track_order(&project, composition_id),
        vec![ids[2], ids[0], ids[1]],
        "FrameEvaluator must read the new Composition.track_ids order directly"
    );
}
