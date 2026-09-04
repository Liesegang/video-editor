mod support;

use anyhow::{Context, Result, anyhow};
use std::sync::Arc;

use library::core::framing::FrameEvaluator;
use library::editor::project_service::GeneratorNodeRequest;
use library::model::Track;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroupKind, FrameItem};
use library::model::project::{
    Composition, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, PortAddress, PortOwner,
    Project, ProjectGraphError,
};
use library::plugin::properties::ConstantEvaluator;
use library::plugin::{PluginManager, PropertyEvaluatorRegistry};
use uuid::Uuid;

use support::generator_node;

fn project_with_tracks(track_names: &[&str]) -> Result<(Project, Uuid, Vec<Uuid>)> {
    assert!(!track_names.is_empty());

    let mut project = Project::new("track reorder test");
    let (mut composition, first_track) = Composition::new(track_names[0], 1920, 1080, 30.0, 10.0);
    let composition_id = composition.id;
    let mut track_ids = vec![first_track.id];
    assert!(
        project.add_track(first_track).is_ok(),
        "container structural Merge insertion must succeed"
    );

    for name in &track_names[1..] {
        let track = Track::new(name);
        track_ids.push(track.id);
        composition.track_ids.push(track.id);
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
    }
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

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
        project.attach_node_to_container(NodeContainer::Track(track_id), node_id)?;
        let structural_merge_node_id = project
            .get_track(track_id)
            .context("Track must exist after insertion")?
            .structural_merge_node_id;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(node_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(structural_merge_node_id), MERGE_IMAGES_PORT),
        )?;
    }

    Ok((project, composition_id, track_ids))
}

fn evaluated_track_order(project: &Project, composition_id: Uuid) -> Result<Vec<Uuid>> {
    let mut property_evaluators = PropertyEvaluatorRegistry::new();
    assert!(
        property_evaluators
            .register("constant", Arc::new(ConstantEvaluator))
            .is_none()
    );
    let property_evaluators = Arc::new(property_evaluators);
    let plugin_manager = Arc::new(PluginManager::default());
    let composition = project
        .get_composition(composition_id)
        .context("Composition must exist for evaluation")?;

    let items = FrameEvaluator::new(
        project,
        composition,
        property_evaluators,
        plugin_manager.as_ref(),
    )
    .evaluate(0, 1.0, None)?
    .items;
    fn collect_tracks(items: &[FrameItem], track_ids: &mut Vec<Uuid>) {
        for item in items {
            let FrameItem::Group(group) = item else {
                continue;
            };
            if group.kind == FrameGroupKind::Track {
                track_ids.push(group.source_id);
            } else {
                collect_tracks(&group.items, track_ids);
            }
        }
    }
    let mut track_ids = Vec::new();
    collect_tracks(&items, &mut track_ids);
    if track_ids.is_empty() {
        return Err(anyhow!("evaluated Composition contained no Track groups"));
    }
    Ok(track_ids)
}

#[test]
fn authoritative_track_move_handles_up_down_first_last_and_no_op() -> Result<()> {
    let (mut project, composition_id, ids) = project_with_tracks(&["A", "B", "C", "D"])?;

    assert!(project.move_track_within_composition(composition_id, ids[2], 0)?);
    assert_eq!(
        project
            .get_composition(composition_id)
            .context("Composition must remain after upward move")?
            .track_ids,
        vec![ids[2], ids[0], ids[1], ids[3]]
    );

    assert!(project.move_track_within_composition(composition_id, ids[2], usize::MAX)?);
    assert_eq!(
        project
            .get_composition(composition_id)
            .context("Composition must remain after last-slot move")?
            .track_ids,
        vec![ids[0], ids[1], ids[3], ids[2]]
    );

    assert!(project.move_track_within_composition(composition_id, ids[3], 1)?);
    assert_eq!(
        project
            .get_composition(composition_id)
            .context("Composition must remain after downward move")?
            .track_ids,
        vec![ids[0], ids[3], ids[1], ids[2]]
    );

    assert!(!project.move_track_within_composition(composition_id, ids[3], 1)?);
    Ok(())
}

#[test]
fn track_move_rejects_cross_composition_reparenting_without_mutation() -> Result<()> {
    let (mut project, first_composition_id, first_ids) = project_with_tracks(&["A", "B"])?;
    let (second_composition, second_track) = Composition::new("Other", 1920, 1080, 30.0, 10.0);
    let second_composition_id = second_composition.id;
    let second_track_id = second_track.id;
    assert!(
        project.add_track(second_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(second_composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let before_first = project
        .get_composition(first_composition_id)
        .context("first Composition must exist")?
        .track_ids
        .clone();
    let before_second = project
        .get_composition(second_composition_id)
        .context("second Composition must exist")?
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
            .context("first Composition must remain")?
            .track_ids,
        before_first
    );
    assert_eq!(
        project
            .get_composition(second_composition_id)
            .context("second Composition must remain")?
            .track_ids,
        before_second
    );
    assert_eq!(first_ids, before_first);
    Ok(())
}

#[test]
fn frame_evaluation_observes_track_order_immediately_after_the_atomic_move() -> Result<()> {
    let (mut project, composition_id, ids) = project_with_tracks(&["Bottom", "Middle", "Top"])?;
    assert_eq!(evaluated_track_order(&project, composition_id)?, ids);

    assert!(project.move_track_within_composition(composition_id, ids[2], 0)?);

    assert_eq!(
        evaluated_track_order(&project, composition_id)?,
        vec![ids[2], ids[0], ids[1]],
        "FrameEvaluator must read the new Composition.track_ids order directly"
    );
    Ok(())
}
