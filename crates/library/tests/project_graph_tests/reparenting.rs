use anyhow::{Context, Result, anyhow};
use library::model::project::{
    DURATION_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT, NodeContainer,
    PortDataType, PortDefinition, PortOwner, Project, ProjectConnection, ProjectGraphError,
    RESOLUTION_PORT, TIME_PORT,
};
use library::model::{BlendMode, Composition, Node, Track};
use uuid::Uuid;

use super::graph_support::{
    add_clip, add_node, address, bind_downstream_merge, frame, frame_for_composition, graph_output,
    plugin_operation_node, project_with_composition, solid_node, structural_merge_id,
};

fn container_owner(container: NodeContainer) -> PortOwner {
    match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}

fn container_output(project: &Project, container: NodeContainer) -> Result<Option<Uuid>> {
    match container {
        NodeContainer::Composition(id) => Ok(project
            .get_composition(id)
            .with_context(|| format!("Composition {id} must exist"))?
            .output_node_id),
        NodeContainer::Track(id) => Ok(project
            .get_track(id)
            .with_context(|| format!("Track {id} must exist"))?
            .output_node_id),
        NodeContainer::Clip(id) => Ok(project
            .get_clip(id)
            .with_context(|| format!("Clip {id} must exist"))?
            .output_node_id),
    }
}

#[test]
fn attaching_a_node_to_any_missing_container_is_atomic() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "original container")?;
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("original output"),
    )?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .map_err(|error| anyhow!(error))?;
    let original = project;

    let missing_composition = Uuid::new_v4();
    let missing_track = Uuid::new_v4();
    let missing_clip = Uuid::new_v4();
    let cases = [
        (
            NodeContainer::Composition(missing_composition),
            ProjectGraphError::CompositionNotFound(missing_composition),
        ),
        (
            NodeContainer::Track(missing_track),
            ProjectGraphError::TrackNotFound(missing_track),
        ),
        (
            NodeContainer::Clip(missing_clip),
            ProjectGraphError::ClipNotFound(missing_clip),
        ),
    ];

    for (container, expected_error) in cases {
        let mut attempted = original.clone();
        assert_eq!(
            attempted.attach_node_to_container_at(container, node_id, Some(0)),
            Err(expected_error)
        );
        assert_eq!(attempted, original);
    }
    Ok(())
}

#[test]
fn track_and_clip_reparent_remap_only_direct_parent_metadata_and_still_render() -> Result<()> {
    let (mut project, first_composition_id, first_track_id) = project_with_composition();
    let (second_composition, second_track) = Composition::new("second", 320, 180, 30.0, 10.0);
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

    let clip_id = add_clip(&mut project, first_track_id, "movable clip")?;
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("render after move"),
    )?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .map_err(|error| anyhow!(error))?;

    let track_connection_id = project.connect_ports(
        address(PortOwner::Composition(first_composition_id), TIME_PORT),
        address(PortOwner::Track(first_track_id), TIME_PORT),
    )?;
    let clip_connection_id = project.connect_ports(
        address(PortOwner::Track(first_track_id), DURATION_PORT),
        address(PortOwner::Clip(clip_id), DURATION_PORT),
    )?;
    let unrelated_connection_id = project.connect_ports(
        address(
            PortOwner::Composition(second_composition_id),
            RESOLUTION_PORT,
        ),
        address(PortOwner::Track(second_track_id), RESOLUTION_PORT),
    )?;
    let original_track_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == track_connection_id)
        .context("track metadata connection must exist")?
        .clone();
    let original_clip_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == clip_connection_id)
        .context("Clip metadata connection must exist")?
        .clone();
    let original_unrelated_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == unrelated_connection_id)
        .context("unrelated metadata connection must exist")?
        .clone();

    project.attach_track_to_composition_at(second_composition_id, first_track_id, Some(0))?;
    let moved_track_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == track_connection_id)
        .context("moved Track metadata connection must exist")?;
    assert_eq!(
        moved_track_connection.from,
        address(PortOwner::Composition(second_composition_id), TIME_PORT)
    );
    assert_eq!(moved_track_connection.to, original_track_connection.to);
    assert_eq!(
        moved_track_connection.order,
        original_track_connection.order
    );
    assert_eq!(moved_track_connection.id, original_track_connection.id);
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == unrelated_connection_id)
            .context("unrelated metadata connection must remain")?,
        &original_unrelated_connection
    );

    project.attach_clip_to_track_at(second_track_id, clip_id, Some(0))?;
    let moved_clip_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == clip_connection_id)
        .context("moved Clip metadata connection must exist")?;
    assert_eq!(
        moved_clip_connection.from,
        address(PortOwner::Track(second_track_id), DURATION_PORT)
    );
    assert_eq!(moved_clip_connection.to, original_clip_connection.to);
    assert_eq!(moved_clip_connection.order, original_clip_connection.order);
    assert_eq!(moved_clip_connection.id, original_clip_connection.id);
    assert!(project.validate_connections().is_empty());
    assert!(frame_for_composition(&project, 1, 0)?.object_count() > 0);
    Ok(())
}

#[test]
fn direct_node_reparent_remaps_metadata_for_every_container_pair() -> Result<()> {
    for source_kind in 0..3 {
        for destination_kind in 0..3 {
            let mut project = Project::new("node parent matrix");
            let (first_composition, first_track) = Composition::new("first", 320, 180, 30.0, 10.0);
            let first_composition_id = first_composition.id;
            let first_track_id = first_track.id;
            assert!(
                project.add_track(first_track).is_ok(),
                "container structural Merge insertion must succeed"
            );
            assert!(
                project.add_composition(first_composition).is_ok(),
                "container structural Merge insertion must succeed"
            );
            let first_clip_id = add_clip(&mut project, first_track_id, "first clip")?;

            let (second_composition, second_track) =
                Composition::new("second", 320, 180, 30.0, 10.0);
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
            let second_clip_id = add_clip(&mut project, second_track_id, "second clip")?;

            let sources = [
                NodeContainer::Composition(first_composition_id),
                NodeContainer::Track(first_track_id),
                NodeContainer::Clip(first_clip_id),
            ];
            let destinations = [
                NodeContainer::Composition(second_composition_id),
                NodeContainer::Track(second_track_id),
                NodeContainer::Clip(second_clip_id),
            ];
            let source = sources[source_kind];
            let destination = destinations[destination_kind];
            let moved_node_id = add_node(&mut project, source, Node::new_merge("moved"))?;
            let destination_output_id = add_node(
                &mut project,
                destination,
                Node::new_merge("destination output"),
            )?;
            if matches!(source, NodeContainer::Clip(_)) {
                project.set_output_node(source, Some(moved_node_id))?;
            } else {
                bind_downstream_merge(&mut project, source, moved_node_id)?;
            }
            if matches!(destination, NodeContainer::Clip(_)) {
                project.set_output_node(destination, Some(destination_output_id))?;
            } else {
                bind_downstream_merge(&mut project, destination, destination_output_id)?;
            }
            let connection_id = project.connect_ports(
                address(container_owner(source), TIME_PORT),
                address(PortOwner::Node(moved_node_id), TIME_PORT),
            )?;
            let original = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)
                .context("source metadata connection must exist")?
                .clone();

            project
                .attach_node_to_container_at(destination, moved_node_id, Some(0))
                .map_err(|error| anyhow!(error))?;

            let remapped = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)
                .context("remapped metadata connection must exist")?;
            assert_eq!(
                remapped.from,
                address(container_owner(destination), TIME_PORT),
                "source kind {source_kind}, destination kind {destination_kind}"
            );
            assert_eq!(remapped.id, original.id);
            assert_eq!(remapped.order, original.order);
            assert_eq!(remapped.to, original.to);
            assert_eq!(container_output(&project, source)?, None);
            assert_eq!(
                container_output(&project, destination)?,
                Some(destination_output_id)
            );
            assert_eq!(
                project.find_node_container(moved_node_id),
                Some(destination)
            );
            assert!(project.validate_connections().is_empty());
        }
    }
    Ok(())
}

#[test]
fn same_parent_reorder_preserves_metadata_connections_and_output_binding() -> Result<()> {
    let (mut project, composition_id, first_track_id) = project_with_composition();
    let second_track = Track::new("second");
    let second_track_id = second_track.id;
    assert!(
        project.add_track(second_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.attach_track_to_composition(composition_id, second_track_id)?;
    project.connect_ports(
        address(PortOwner::Composition(composition_id), TIME_PORT),
        address(PortOwner::Track(first_track_id), TIME_PORT),
    )?;

    let first_clip_id = add_clip(&mut project, second_track_id, "first")?;
    let second_clip_id = add_clip(&mut project, second_track_id, "second")?;
    project.connect_ports(
        address(PortOwner::Track(second_track_id), TIME_PORT),
        address(PortOwner::Clip(first_clip_id), TIME_PORT),
    )?;
    let first_node_id = add_node(
        &mut project,
        NodeContainer::Clip(second_clip_id),
        solid_node("first node"),
    )?;
    let second_node_id = add_node(
        &mut project,
        NodeContainer::Clip(second_clip_id),
        solid_node("second node"),
    )?;
    project
        .set_output_node(NodeContainer::Clip(second_clip_id), Some(first_node_id))
        .map_err(|error| anyhow!(error))?;
    project.connect_ports(
        address(PortOwner::Clip(second_clip_id), TIME_PORT),
        address(PortOwner::Node(first_node_id), TIME_PORT),
    )?;
    let original_connections = project.connections.clone();

    project.attach_track_to_composition_at(composition_id, first_track_id, Some(1))?;
    project.attach_clip_to_track_at(second_track_id, first_clip_id, Some(1))?;
    project
        .attach_node_to_container_at(NodeContainer::Clip(second_clip_id), first_node_id, Some(1))
        .map_err(|error| anyhow!(error))?;

    assert_eq!(
        project
            .get_composition(composition_id)
            .context("Composition must exist")?
            .track_ids,
        vec![second_track_id, first_track_id]
    );
    assert_eq!(
        project
            .get_track(second_track_id)
            .context("second Track must exist")?
            .clip_ids,
        vec![second_clip_id, first_clip_id]
    );
    assert_eq!(
        project
            .get_clip(second_clip_id)
            .context("second Clip must exist")?
            .node_ids,
        vec![second_node_id, first_node_id]
    );
    assert_eq!(project.connections.len(), original_connections.len());
    for original in &original_connections {
        let current = project
            .connections
            .iter()
            .find(|connection| connection.id == original.id)
            .context("same-parent reorder must preserve connection identity")?;
        assert_eq!(current.from, original.from);
        assert_eq!(current.to, original.to);
        assert_eq!(current.blend_mode, original.blend_mode);
        if !matches!(
            original.to.port.as_str(),
            MERGE_IMAGES_PORT | MERGE_SOUNDS_PORT
        ) {
            assert_eq!(current.order, original.order);
        }
    }
    assert_eq!(
        project
            .get_clip(second_clip_id)
            .context("second Clip must exist")?
            .output_node_id,
        Some(first_node_id)
    );
    assert!(project.validate_connections().is_empty());
    Ok(())
}

#[test]
fn node_reparent_preserves_graph_image_wires_ids_orders_targets_and_rendering() -> Result<()> {
    let (mut project, _composition_id, first_track_id) = project_with_composition();
    let second_track = Track::new("second");
    let second_track_id = second_track.id;
    assert!(
        project.add_track(second_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let composition_id = project.compositions[0].id;
    project.attach_track_to_composition(composition_id, second_track_id)?;
    let first_source_id = add_node(
        &mut project,
        NodeContainer::Track(second_track_id),
        solid_node("first source"),
    )?;
    let second_source_id = add_node(
        &mut project,
        NodeContainer::Track(second_track_id),
        solid_node("second source"),
    )?;
    let merge_id = add_node(
        &mut project,
        NodeContainer::Track(first_track_id),
        Node::new_merge("moved merge"),
    )?;
    project.connect_ports(
        address(PortOwner::Node(first_source_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )?;
    let second_image_connection_id = project.connect_ports(
        address(PortOwner::Node(second_source_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )?;
    project.set_connection_blend_mode(second_image_connection_id, BlendMode::Overlay)?;
    project.reorder_connection(second_image_connection_id, 0)?;
    let metadata_connection_id = project.connect_ports(
        address(PortOwner::Track(first_track_id), TIME_PORT),
        address(PortOwner::Node(merge_id), TIME_PORT),
    )?;
    project.connect_ports(
        address(
            PortOwner::Node(structural_merge_id(
                &project,
                NodeContainer::Track(second_track_id),
            )?),
            IMAGE_OUTPUT_PORT,
        ),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )?;
    let original_connections = project.connections.clone();

    project
        .attach_node_to_container(NodeContainer::Track(second_track_id), merge_id)
        .map_err(|error| anyhow!(error))?;
    project
        .set_output_node(NodeContainer::Track(second_track_id), Some(merge_id))
        .map_err(|error| anyhow!(error))?;

    for original in &original_connections {
        let current = project
            .connections
            .iter()
            .find(|connection| connection.id == original.id)
            .context("reparented connection must exist")?;
        assert_eq!(current.id, original.id);
        assert_eq!(current.order, original.order);
        assert_eq!(current.blend_mode, original.blend_mode);
        assert_eq!(current.to, original.to);
        if original.id == metadata_connection_id {
            assert_eq!(
                current.from,
                address(PortOwner::Track(second_track_id), TIME_PORT)
            );
        } else {
            assert_eq!(current.from, original.from);
        }
    }
    assert!(project.validate_connections().is_empty());
    assert!(frame(&project, 0)?.object_count() > 0);
    Ok(())
}

#[test]
fn cycle_created_by_reparent_rolls_back_containment_output_and_all_wires() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "cycle clip")?;
    let node_id = add_node(
        &mut project,
        NodeContainer::Track(track_id),
        plugin_operation_node(
            "cycle value",
            "utility",
            "dev.example.cycle-value",
            "value.produce",
            vec![
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                graph_output("value", "Value", PortDataType::Number),
                graph_output(IMAGE_OUTPUT_PORT, "Image", PortDataType::Image),
            ],
        ),
    )?;
    project.connect_ports(
        address(PortOwner::Track(track_id), TIME_PORT),
        address(PortOwner::Node(node_id), TIME_PORT),
    )?;
    project.connect_ports(
        address(PortOwner::Node(node_id), "value"),
        address(PortOwner::Clip(clip_id), DURATION_PORT),
    )?;
    assert!(project.validate_connections().is_empty());
    let original = project.clone();

    let result = project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id);

    assert!(matches!(
        result,
        Err(ProjectGraphError::ConnectionCycle { .. })
    ));
    assert_eq!(project, original);
    Ok(())
}

#[test]
fn unremappable_direct_parent_source_fails_without_mutating_project() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "destination")?;
    let node_id = add_node(
        &mut project,
        NodeContainer::Track(track_id),
        Node::new_merge("moved"),
    )?;
    let missing_source = address(PortOwner::Track(track_id), "missing_metadata");
    project.connections.push(ProjectConnection::new(
        missing_source.clone(),
        address(PortOwner::Node(node_id), TIME_PORT),
        0,
    ));
    let original = project.clone();

    assert_eq!(
        project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id),
        Err(ProjectGraphError::PortNotFound(missing_source))
    );
    assert_eq!(project, original);
    Ok(())
}
