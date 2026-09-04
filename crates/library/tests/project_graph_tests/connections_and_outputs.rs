use anyhow::{Context, Result, anyhow};
use library::model::frame::entity::FrameItem;
use library::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, PortDirection,
    PortMultiplicity, PortOwner, Project, ProjectGraphError,
};
use library::model::{Node, Track};
use library::plugin::PluginManager;
use uuid::Uuid;

use super::graph_support::{
    add_clip, add_node, address, connect_source_to_structural_merge, find_group, frame,
    object_source_ids, project_with_composition, solid_node, structural_merge_id,
};

#[test]
fn single_inputs_replace_while_variadic_inputs_reorder_disconnect_and_roundtrip() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip")?;
    let first_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("first"),
    )?;
    let second_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("second"),
    )?;
    let transform_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        PluginManager::default().create_image_transform_operation_node()?,
    )?;
    let merge_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_merge("merge"),
    )?;

    let single_target = address(PortOwner::Node(transform_id), IMAGE_INPUT_PORT);
    let first_single = project.connect_ports(
        address(PortOwner::Node(first_id), IMAGE_OUTPUT_PORT),
        single_target.clone(),
    )?;
    let second_single = project.connect_ports(
        address(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
        single_target.clone(),
    )?;
    assert_ne!(first_single, second_single);
    let singles = project
        .connections
        .iter()
        .filter(|connection| connection.to == single_target)
        .collect::<Vec<_>>();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].from.owner, PortOwner::Node(second_id));

    let merge_target = address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    assert_eq!(
        project
            .port_definition(&merge_target, PortDirection::Input)
            .context("Merge variadic input definition must exist")?
            .multiplicity,
        PortMultiplicity::Variadic
    );
    let first_connection = project.connect_ports(
        address(PortOwner::Node(first_id), IMAGE_OUTPUT_PORT),
        merge_target.clone(),
    )?;
    let second_connection = project.connect_ports(
        address(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
        merge_target.clone(),
    )?;
    assert_eq!(
        project
            .connections
            .iter()
            .filter(|connection| connection.to == merge_target)
            .count(),
        2
    );
    project.reorder_connection(second_connection, 0)?;
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == second_connection)
            .context("second Merge connection must exist")?
            .order,
        0
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection)
            .context("first Merge connection must exist")?
            .order,
        1
    );

    let loaded = Project::load(&project.save()?)?;
    assert_eq!(loaded.connections, project.connections);
    assert!(project.disconnect_connection(second_connection));
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection)
            .context("remaining Merge connection must exist")?
            .order,
        0
    );
    Ok(())
}

#[test]
fn image_cycles_include_connections_containment_and_explicit_outputs() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip")?;
    let first_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_merge("first merge"),
    )?;
    let second_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_merge("second merge"),
    )?;
    project.connect_ports(
        address(PortOwner::Node(first_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(second_id), MERGE_IMAGES_PORT),
    )?;
    let reverse = project.connect_ports(
        address(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(first_id), MERGE_IMAGES_PORT),
    );
    assert_eq!(
        reverse,
        Err(ProjectGraphError::ConnectionCycle {
            from: PortOwner::Node(second_id),
            to: PortOwner::Node(first_id),
        })
    );

    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(first_id))
        .map_err(|error| anyhow!(error))?;
    let container_cycle = project.connect_ports(
        address(PortOwner::Clip(clip_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(first_id), MERGE_IMAGES_PORT),
    );
    assert_eq!(
        container_cycle,
        Err(ProjectGraphError::ConnectionCycle {
            from: PortOwner::Clip(clip_id),
            to: PortOwner::Node(first_id),
        })
    );
    Ok(())
}

#[test]
fn setting_an_output_rejects_a_preexisting_reverse_edge_atomically() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip")?;
    let feedback_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_merge("feedback merge"),
    )?;
    let valid_output_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_merge("valid output"),
    )?;

    project.connect_ports(
        address(PortOwner::Clip(clip_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(feedback_id), MERGE_IMAGES_PORT),
    )?;
    let before = project.clone();

    assert_eq!(
        project.set_output_node(NodeContainer::Clip(clip_id), Some(feedback_id)),
        Err(ProjectGraphError::ConnectionCycle {
            from: PortOwner::Clip(clip_id),
            to: PortOwner::Node(feedback_id),
        })
    );
    assert_eq!(project, before, "a rejected output binding must not mutate");

    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(valid_output_id))
        .map_err(|error| anyhow!(error))?;
    assert_eq!(
        project
            .get_clip(clip_id)
            .context("Clip must exist")?
            .output_node_id,
        Some(valid_output_id)
    );
    project
        .set_output_node(NodeContainer::Clip(clip_id), None)
        .map_err(|error| anyhow!(error))?;
    assert_eq!(
        project
            .get_clip(clip_id)
            .context("Clip must exist")?
            .output_node_id,
        None
    );
    Ok(())
}

#[test]
fn clip_does_not_adopt_direct_image_nodes_without_an_explicit_output() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip")?;
    let first_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("first"),
    )?;
    let second_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("second"),
    )?;

    assert_eq!(
        project
            .get_clip(clip_id)
            .context("Clip must exist")?
            .node_ids,
        vec![first_id, second_id]
    );
    assert_eq!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        Vec::<PortOwner>::new(),
        "ordered graph membership must not choose a Clip image output"
    );
    assert_eq!(frame(&project, 0)?.object_count(), 0);

    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(second_id))
        .map_err(|error| anyhow!(error))?;
    let rendered = frame(&project, 0)?;
    let clip_group = find_group(&rendered.items, clip_id).context("Clip group must render")?;
    assert_eq!(clip_group.items.len(), 1);
    assert_eq!(
        match &clip_group.items[0] {
            FrameItem::Group(group) => group.source_id,
            FrameItem::Object(_) => Uuid::nil(),
        },
        second_id
    );
    Ok(())
}

#[test]
fn direct_track_and_composition_output_nodes_keep_their_interactive_source_identity() -> Result<()>
{
    let (mut track_project, _composition_id, track_id) = project_with_composition();
    let track_node_id = add_node(
        &mut track_project,
        NodeContainer::Track(track_id),
        solid_node("direct track output"),
    )?;
    connect_source_to_structural_merge(
        &mut track_project,
        NodeContainer::Track(track_id),
        PortOwner::Node(track_node_id),
    )?;
    assert_eq!(
        object_source_ids(&frame(&track_project, 0)?.items),
        vec![track_node_id]
    );

    let (mut composition_project, composition_id, _track_id) = project_with_composition();
    let composition_node_id = add_node(
        &mut composition_project,
        NodeContainer::Composition(composition_id),
        solid_node("direct composition output"),
    )?;
    connect_source_to_structural_merge(
        &mut composition_project,
        NodeContainer::Composition(composition_id),
        PortOwner::Node(composition_node_id),
    )?;
    assert_eq!(
        object_source_ids(&frame(&composition_project, 0)?.items),
        vec![composition_node_id]
    );
    Ok(())
}

#[test]
fn structural_outputs_are_explicit_and_direct_helpers_remain_unwired() -> Result<()> {
    let (mut project, composition_id, first_track_id) = project_with_composition();
    let first_clip_id = add_clip(&mut project, first_track_id, "first clip")?;
    let second_clip_id = add_clip(&mut project, first_track_id, "second clip")?;
    for (clip_id, name) in [
        (first_clip_id, "first clip image"),
        (second_clip_id, "second clip image"),
    ] {
        let node_id = add_node(&mut project, NodeContainer::Clip(clip_id), solid_node(name))?;
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
            .map_err(|error| anyhow!(error))?;
    }
    let direct_track_node_id = add_node(
        &mut project,
        NodeContainer::Track(first_track_id),
        solid_node("track graph helper"),
    )?;

    let second_track = Track::new("second track");
    let second_track_id = second_track.id;
    assert!(
        project.add_track(second_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.attach_track_to_composition(composition_id, second_track_id)?;
    let third_clip_id = add_clip(&mut project, second_track_id, "third clip")?;
    let third_clip_node_id = add_node(
        &mut project,
        NodeContainer::Clip(third_clip_id),
        solid_node("third clip image"),
    )?;
    project
        .set_output_node(NodeContainer::Clip(third_clip_id), Some(third_clip_node_id))
        .map_err(|error| anyhow!(error))?;
    let direct_composition_node_id = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        solid_node("composition graph helper"),
    )?;

    assert_eq!(
        project
            .container_image_sources(PortOwner::Track(first_track_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Node(structural_merge_id(
            &project,
            NodeContainer::Track(first_track_id)
        )?)]
    );
    assert_eq!(
        project
            .container_image_sources(PortOwner::Composition(composition_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Node(structural_merge_id(
            &project,
            NodeContainer::Composition(composition_id)
        )?)]
    );
    let track_target = address(
        PortOwner::Node(structural_merge_id(
            &project,
            NodeContainer::Track(first_track_id),
        )?),
        MERGE_IMAGES_PORT,
    );
    assert_eq!(
        project
            .connections
            .iter()
            .filter(|connection| connection.to == track_target)
            .map(|connection| connection.from.owner)
            .collect::<Vec<_>>(),
        vec![
            PortOwner::Clip(first_clip_id),
            PortOwner::Clip(second_clip_id)
        ]
    );
    assert!(
        !project
            .container_image_sources(PortOwner::Track(first_track_id))
            .iter()
            .any(|source| source.source == PortOwner::Node(direct_track_node_id))
    );
    assert!(
        !project
            .container_image_sources(PortOwner::Composition(composition_id))
            .iter()
            .any(|source| source.source == PortOwner::Node(direct_composition_node_id))
    );
    Ok(())
}
