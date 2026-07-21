use anyhow::{Context, Result, anyhow};
use library::model::frame::color::Color;
use library::model::project::{
    FMOD_X_INPUT_PORT, NUMBER_RESULT_OUTPUT_PORT, NodeContainer, PortOwner, Project, TIME_PORT,
};
use library::model::{Clip, Composition, CompositionInstanceContent, Node};

use super::graph_support::{
    add_node, address, connect_source_to_structural_merge, frame, frame_for_composition,
    object_source_ids, project_with_composition, solid_node,
};

#[test]
fn disabled_and_out_of_range_nodes_never_expose_preview_source_identity() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip = Clip::new("short clip", 1.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("visual"),
    )?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .map_err(|error| anyhow!(error))?;

    assert!(object_source_ids(&frame(&project, 0)?.items).is_empty());
    assert_eq!(
        object_source_ids(&frame(&project, 30)?.items),
        vec![node_id]
    );
    project
        .get_node_mut(node_id)
        .context("visual Node must exist")?
        .enabled = false;
    assert!(object_source_ids(&frame(&project, 30)?.items).is_empty());
    Ok(())
}

#[test]
fn composition_duration_gates_direct_composition_and_track_nodes() -> Result<()> {
    let (mut project, composition_id, track_id) = project_with_composition();
    let composition_node_id = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        solid_node("composition direct"),
    )?;
    let track_node_id = add_node(
        &mut project,
        NodeContainer::Track(track_id),
        solid_node("track direct"),
    )?;
    connect_source_to_structural_merge(
        &mut project,
        NodeContainer::Track(track_id),
        PortOwner::Node(track_node_id),
    )?;

    let active_ids = object_source_ids(&frame(&project, 299)?.items);
    assert!(active_ids.contains(&track_node_id));

    connect_source_to_structural_merge(
        &mut project,
        NodeContainer::Composition(composition_id),
        PortOwner::Node(composition_node_id),
    )?;
    assert!(object_source_ids(&frame(&project, 299)?.items).contains(&composition_node_id));

    let expected_background = project
        .get_composition(composition_id)
        .context("root Composition must exist")?
        .background_color
        .clone();
    let at_end = frame(&project, 300)?;
    assert_eq!(at_end.background_color, expected_background);
    assert!(
        at_end.items.is_empty(),
        "the root raster boundary may materialize its background, but direct Nodes must be NoOutput at the half-open duration end"
    );

    project
        .set_output_node(NodeContainer::Composition(composition_id), None)
        .map_err(|error| anyhow!(error))?;
    assert!(
        frame(&project, 300)?.items.is_empty(),
        "Track-direct Nodes must inherit the same Composition activity gate"
    );

    let composition = project
        .get_composition_mut(composition_id)
        .context("root Composition must remain mutable")?;
    composition.duration = 0.0;
    composition.work_area_in = 0;
    composition.work_area_out = 0;
    assert!(
        frame(&project, 0)?.items.is_empty(),
        "a zero-duration Composition has no active timeline instant"
    );
    Ok(())
}

#[test]
fn composition_instance_does_not_materialize_target_background_after_its_duration() -> Result<()> {
    let (mut project, parent_id, parent_track_id) = project_with_composition();
    let parent_background = Color {
        r: 7,
        g: 11,
        b: 13,
        a: 255,
    };
    project
        .get_composition_mut(parent_id)
        .context("parent Composition must exist")?
        .background_color = parent_background.clone();

    let (mut target, target_track) = Composition::new("short target", 320, 180, 30.0, 1.0);
    target.background_color = Color {
        r: 200,
        g: 100,
        b: 50,
        a: 255,
    };
    let target_id = target.id;
    assert!(
        project.add_track(target_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(target).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let target_node_id = add_node(
        &mut project,
        NodeContainer::Composition(target_id),
        solid_node("target direct"),
    )?;
    connect_source_to_structural_merge(
        &mut project,
        NodeContainer::Composition(target_id),
        PortOwner::Node(target_node_id),
    )?;

    let instance_clip = Clip::new("short target placement", 0.0, 10.0);
    let instance_clip_id = instance_clip.id;
    project.add_clip(instance_clip);
    project.attach_clip_to_track(parent_track_id, instance_clip_id)?;
    let instance = Node::new_composition_instance(
        "short target instance",
        CompositionInstanceContent {
            composition_id: target_id,
        },
    );
    let instance_id = add_node(
        &mut project,
        NodeContainer::Clip(instance_clip_id),
        instance,
    )?;
    project
        .set_output_node(NodeContainer::Clip(instance_clip_id), Some(instance_id))
        .map_err(|error| anyhow!(error))?;
    project
        .set_audio_output_node(NodeContainer::Clip(instance_clip_id), Some(instance_id))
        .map_err(|error| anyhow!(error))?;

    assert!(
        object_source_ids(&frame(&project, 29)?.items).contains(&target_node_id),
        "the nested Composition must remain active immediately before its duration"
    );
    let at_target_end = frame(&project, 30)?;
    assert_eq!(at_target_end.background_color, parent_background);
    assert!(
        at_target_end.items.is_empty(),
        "an inactive nested Composition must be NoOutput, not a materialized background group"
    );

    let sibling_id = add_node(
        &mut project,
        NodeContainer::Composition(parent_id),
        solid_node("active sibling"),
    )?;
    let sibling_connection_id = connect_source_to_structural_merge(
        &mut project,
        NodeContainer::Composition(parent_id),
        PortOwner::Node(sibling_id),
    )?;
    assert_eq!(
        object_source_ids(&frame(&project, 30)?.items),
        vec![sibling_id],
        "Merge must skip an inactive nested Composition without suppressing its active sibling"
    );

    assert!(project.disconnect_connection(sibling_connection_id));
    let local_clip = Clip::new("local composition instance", 5.0, 2.0);
    let local_clip_id = local_clip.id;
    project.add_clip(local_clip);
    project.attach_clip_to_track(parent_track_id, local_clip_id)?;
    let local_instance = Node::new_composition_instance(
        "local short target instance",
        CompositionInstanceContent {
            composition_id: target_id,
        },
    );
    let local_instance_id = add_node(
        &mut project,
        NodeContainer::Clip(local_clip_id),
        local_instance,
    )?;
    project
        .set_output_node(NodeContainer::Clip(local_clip_id), Some(local_instance_id))
        .map_err(|error| anyhow!(error))?;
    assert!(
        object_source_ids(&frame(&project, 179)?.items).contains(&target_node_id),
        "an unsynced Composition Instance must use Clip-local time before the target duration"
    );
    assert!(
        frame(&project, 180)?.items.is_empty(),
        "an unsynced Composition Instance must become NoOutput at the target's local duration boundary"
    );
    Ok(())
}

#[test]
fn explicit_fmod_time_loop_cannot_resurrect_a_composition_after_its_duration() -> Result<()> {
    let mut project = Project::new("Composition activity before Time remap");
    let (target, target_track) = Composition::new("short target", 320, 180, 30.0, 1.0);
    let target_id = target.id;
    assert!(
        project.add_track(target_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(target).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let (driver, driver_track) = Composition::new("time driver", 320, 180, 30.0, 10.0);
    let driver_id = driver.id;
    assert!(
        project.add_track(driver_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(driver).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let target_node_id = add_node(
        &mut project,
        NodeContainer::Composition(target_id),
        solid_node("short target output"),
    )?;
    connect_source_to_structural_merge(
        &mut project,
        NodeContainer::Composition(target_id),
        PortOwner::Node(target_node_id),
    )?;

    let fmod = Node::new_fmod("one-second loop");
    let fmod_id = add_node(&mut project, NodeContainer::Composition(driver_id), fmod)?;
    project.connect_ports(
        address(PortOwner::Composition(driver_id), TIME_PORT),
        address(PortOwner::Node(fmod_id), FMOD_X_INPUT_PORT),
    )?;
    project.connect_ports(
        address(PortOwner::Node(fmod_id), NUMBER_RESULT_OUTPUT_PORT),
        address(PortOwner::Composition(target_id), TIME_PORT),
    )?;

    assert!(
        object_source_ids(&frame_for_composition(&project, 0, 29)?.items).contains(&target_node_id)
    );
    assert!(
        frame_for_composition(&project, 0, 30)?.items.is_empty(),
        "global t=1 is outside the target; its explicit t mod 1 = 0 remap must not reactivate it"
    );
    Ok(())
}
