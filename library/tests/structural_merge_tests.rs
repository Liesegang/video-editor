use anyhow::{Context, Result, bail};
use library::model::project::{
    Composition, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, PortAddress, PortOwner,
    Project, ProjectConnection, ProjectGraphError,
};
use library::model::{BlendMode, Clip, Node, NodeContent, Track};
use uuid::Uuid;

fn address(owner: PortOwner, port: &str) -> PortAddress {
    PortAddress::new(owner, port)
}

fn one_track_project() -> Result<(Project, Uuid, Uuid)> {
    let mut project = Project::new("structural Merge test");
    let (composition, track) = Composition::new("Main", 320, 180, 30.0, 5.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    Ok((project, composition_id, track_id))
}

fn two_track_project() -> Result<(Project, Uuid, Uuid, Uuid)> {
    let mut project = Project::new("two Track structural Merge test");
    let (mut composition, first_track) = Composition::new("Main", 320, 180, 30.0, 5.0);
    let second_track = Track::new("Track 2");
    let composition_id = composition.id;
    let first_track_id = first_track.id;
    let second_track_id = second_track.id;
    composition.track_ids.push(second_track_id);
    project.add_track(first_track)?;
    project.add_track(second_track)?;
    project.add_composition(composition)?;
    Ok((project, composition_id, first_track_id, second_track_id))
}

fn add_clip(project: &mut Project, track_id: Uuid, name: &str) -> Result<Uuid> {
    let clip = Clip::new(name, 0.0, 5.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    Ok(clip_id)
}

fn structural_target(project: &Project, container: NodeContainer) -> Result<PortAddress> {
    let node_id = match container {
        NodeContainer::Composition(id) => {
            project
                .get_composition(id)
                .context("Composition structural Merge owner is missing")?
                .structural_merge_node_id
        }
        NodeContainer::Track(id) => {
            project
                .get_track(id)
                .context("Track structural Merge owner is missing")?
                .structural_merge_node_id
        }
        NodeContainer::Clip(_) => bail!("Clip containers have no structural Merge"),
    };
    Ok(address(PortOwner::Node(node_id), MERGE_IMAGES_PORT))
}

fn target_inputs<'a>(project: &'a Project, target: &PortAddress) -> Vec<&'a ProjectConnection> {
    let mut inputs = project
        .connections
        .iter()
        .filter(|connection| connection.to == *target)
        .collect::<Vec<_>>();
    inputs.sort_by_key(|connection| (connection.order, connection.id));
    inputs
}

fn direct_child_edge<'a>(
    project: &'a Project,
    target: &PortAddress,
    child: PortOwner,
) -> Option<&'a ProjectConnection> {
    let source = address(child, IMAGE_OUTPUT_PORT);
    project
        .connections
        .iter()
        .find(|connection| connection.from == source && connection.to == *target)
}

fn add_merge_node(project: &mut Project, container: NodeContainer, name: &str) -> Result<Uuid> {
    let node = Node::new_merge(name);
    let node_id = node.id;
    project.add_node(node);
    project.attach_node_to_container(container, node_id)?;
    Ok(node_id)
}

#[test]
fn container_insertion_materializes_persisted_merge_nodes_and_prelisted_child_edges() -> Result<()>
{
    let mut project = Project::new("materialization");
    let clip = Clip::new("prelisted Clip", 0.0, 1.0);
    let clip_id = clip.id;
    let (composition, mut track) = Composition::new("Main", 320, 180, 30.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let track_merge_id = track.structural_merge_node_id;
    let composition_merge_id = composition.structural_merge_node_id;
    track.clip_ids.push(clip_id);

    project.add_clip(clip);
    project.add_track(track)?;
    project.add_composition(composition)?;

    for (container, merge_id) in [
        (NodeContainer::Track(track_id), track_merge_id),
        (
            NodeContainer::Composition(composition_id),
            composition_merge_id,
        ),
    ] {
        let node = project
            .get_node(merge_id)
            .context("inserted structural Merge Node is missing")?;
        assert!(matches!(node.content(), NodeContent::Merge));
        assert_eq!(project.find_node_container(merge_id), Some(container));
    }
    assert_eq!(
        target_inputs(
            &project,
            &structural_target(&project, NodeContainer::Track(track_id))?
        )
        .iter()
        .map(|connection| connection.from.owner)
        .collect::<Vec<_>>(),
        vec![PortOwner::Clip(clip_id)]
    );
    assert_eq!(
        target_inputs(
            &project,
            &structural_target(&project, NodeContainer::Composition(composition_id))?
        )
        .iter()
        .map(|connection| connection.from.owner)
        .collect::<Vec<_>>(),
        vec![PortOwner::Track(track_id)]
    );
    assert!(project.validate_connections().is_empty());

    let encoded = project.save()?;
    let decoded = Project::load(&encoded)?;
    assert_eq!(decoded, project);
    Ok(())
}

#[test]
fn required_structural_ids_fail_deserialization_when_omitted() -> Result<()> {
    let (project, _, track_id) = one_track_project()?;
    let persisted = serde_json::to_value(project)?;

    let mut missing_track_annotation = persisted.clone();
    missing_track_annotation["tracks"]
        .as_object_mut()
        .context("serialized tracks are not an object")?
        .get_mut(&track_id.to_string())
        .context("serialized Track is missing")?
        .as_object_mut()
        .context("serialized Track is not an object")?
        .remove("structural_merge_node_id");
    assert!(serde_json::from_value::<Project>(missing_track_annotation).is_err());

    let mut missing_composition_annotation = persisted;
    missing_composition_annotation["compositions"][0]
        .as_object_mut()
        .context("serialized Composition is not an object")?
        .remove("structural_merge_node_id");
    assert!(serde_json::from_value::<Project>(missing_composition_annotation).is_err());
    Ok(())
}

#[test]
fn collision_and_invalid_output_insertion_are_atomic() -> Result<()> {
    let mut collision_project = Project::new("collision");
    let track = Track::new("Track");
    let collision_id = track.structural_merge_node_id;
    let mut colliding_node = Node::new_merge("reserved identity");
    colliding_node.id = collision_id;
    collision_project.add_node(colliding_node);
    let before_collision = collision_project.clone();
    assert_eq!(
        collision_project.add_track(track),
        Err(ProjectGraphError::NodeGraphNodeAlreadyExists(collision_id))
    );
    assert_eq!(collision_project, before_collision);

    let mut invalid_project = Project::new("invalid output");
    let unrelated = Node::new_merge("unrelated result");
    let unrelated_id = unrelated.id;
    invalid_project.add_node(unrelated);
    let mut invalid_track = Track::new("invalid Track");
    let track_id = invalid_track.id;
    let structural_id = invalid_track.structural_merge_node_id;
    invalid_track.node_ids.push(unrelated_id);
    invalid_track.output_node_id = Some(unrelated_id);
    let before_invalid = invalid_project.clone();
    assert_eq!(
        invalid_project.add_track(invalid_track),
        Err(ProjectGraphError::StructuralMergeDoesNotReachOutput {
            container: NodeContainer::Track(track_id),
            node_id: structural_id,
            output_node_id: unrelated_id,
        })
    );
    assert_eq!(invalid_project, before_invalid);
    Ok(())
}

#[test]
fn timeline_reorder_preserves_edge_identity_blend_and_custom_input_slots() -> Result<()> {
    let (mut project, _, track_id) = one_track_project()?;
    let a = add_clip(&mut project, track_id, "A")?;
    let b = add_clip(&mut project, track_id, "B")?;
    let c = add_clip(&mut project, track_id, "C")?;
    let target = structural_target(&project, NodeContainer::Track(track_id))?;
    let custom = add_merge_node(&mut project, NodeContainer::Track(track_id), "custom input")?;
    let custom_id = project.connect_ports(
        address(PortOwner::Node(custom), IMAGE_OUTPUT_PORT),
        target.clone(),
    )?;
    project.reorder_connection(custom_id, 1)?;

    let edge_ids = [a, b, c]
        .into_iter()
        .map(|clip_id| {
            Ok((
                clip_id,
                direct_child_edge(&project, &target, PortOwner::Clip(clip_id))
                    .context("structural Clip edge is missing")?
                    .id,
            ))
        })
        .collect::<Result<std::collections::HashMap<_, _>>>()?;
    project.set_connection_blend_mode(edge_ids[&b], BlendMode::Multiply)?;

    project.attach_clip_to_track_at(track_id, c, Some(0))?;

    assert_eq!(
        project
            .get_track(track_id)
            .context("reordered Track is missing")?
            .clip_ids,
        vec![c, a, b]
    );
    assert_eq!(
        target_inputs(&project, &target)
            .iter()
            .map(|connection| connection.from.owner)
            .collect::<Vec<_>>(),
        vec![
            PortOwner::Clip(c),
            PortOwner::Node(custom),
            PortOwner::Clip(a),
            PortOwner::Clip(b),
        ]
    );
    for clip_id in [a, b, c] {
        assert_eq!(
            direct_child_edge(&project, &target, PortOwner::Clip(clip_id))
                .context("reordered structural Clip edge is missing")?
                .id,
            edge_ids[&clip_id]
        );
    }
    assert_eq!(
        direct_child_edge(&project, &target, PortOwner::Clip(b))
            .context("blended structural Clip edge is missing")?
            .blend_mode,
        BlendMode::Multiply
    );
    Ok(())
}

#[test]
fn deleted_structural_edge_stays_deleted_across_timeline_mutations() -> Result<()> {
    let (mut project, _, track_id) = one_track_project()?;
    let a = add_clip(&mut project, track_id, "A")?;
    let b = add_clip(&mut project, track_id, "B")?;
    let c = add_clip(&mut project, track_id, "C")?;
    let target = structural_target(&project, NodeContainer::Track(track_id))?;
    let deleted_id = direct_child_edge(&project, &target, PortOwner::Clip(b))
        .context("structural edge selected for deletion is missing")?
        .id;
    assert!(project.disconnect_connection(deleted_id));

    project.attach_clip_to_track_at(track_id, c, Some(0))?;
    let d = Clip::new("D", 0.0, 5.0);
    let d_id = d.id;
    project.add_clip(d);
    project.attach_clip_to_track_at(track_id, d_id, Some(1))?;

    assert_eq!(
        project
            .get_track(track_id)
            .context("mutated Track is missing")?
            .clip_ids,
        vec![c, d_id, a, b]
    );
    assert!(direct_child_edge(&project, &target, PortOwner::Clip(b)).is_none());
    assert!(direct_child_edge(&project, &target, PortOwner::Clip(d_id)).is_some());
    assert!(
        project
            .connections
            .iter()
            .all(|connection| connection.id != deleted_id)
    );
    Ok(())
}

#[test]
fn cross_parent_moves_retarget_existing_edges_and_missing_edges_get_fresh_defaults() -> Result<()> {
    let (mut project, _, first_track, second_track) = two_track_project()?;
    let clip_id = add_clip(&mut project, first_track, "moving Clip")?;
    let first_target = structural_target(&project, NodeContainer::Track(first_track))?;
    let second_target = structural_target(&project, NodeContainer::Track(second_track))?;
    let original_id = direct_child_edge(&project, &first_target, PortOwner::Clip(clip_id))
        .context("moving Clip structural edge is missing")?
        .id;
    project.set_connection_blend_mode(original_id, BlendMode::Screen)?;

    project.attach_clip_to_track(second_track, clip_id)?;
    let moved = direct_child_edge(&project, &second_target, PortOwner::Clip(clip_id))
        .context("retargeted structural edge is missing")?;
    assert_eq!(moved.id, original_id);
    assert_eq!(moved.blend_mode, BlendMode::Screen);
    assert!(direct_child_edge(&project, &first_target, PortOwner::Clip(clip_id)).is_none());

    assert!(project.disconnect_connection(original_id));
    project.attach_clip_to_track(first_track, clip_id)?;
    let recreated = direct_child_edge(&project, &first_target, PortOwner::Clip(clip_id))
        .context("recreated structural edge is missing")?;
    assert_ne!(recreated.id, original_id);
    assert_eq!(recreated.blend_mode, BlendMode::Normal);
    Ok(())
}

#[test]
fn reconnect_connect_splice_disconnect_and_reorder_keep_structural_target_consistent() -> Result<()>
{
    let (mut project, _, first_track, second_track) = two_track_project()?;
    let a = add_clip(&mut project, first_track, "A")?;
    let b = add_clip(&mut project, first_track, "B")?;
    let first_target = structural_target(&project, NodeContainer::Track(first_track))?;
    let second_target = structural_target(&project, NodeContainer::Track(second_track))?;

    let b_edge = direct_child_edge(&project, &first_target, PortOwner::Clip(b))
        .context("second Clip structural edge is missing")?
        .id;
    assert!(project.disconnect_connection(b_edge));
    let custom = add_merge_node(
        &mut project,
        NodeContainer::Track(first_track),
        "manual wire source",
    )?;
    let manual_id = project.connect_ports(
        address(PortOwner::Node(custom), IMAGE_OUTPUT_PORT),
        first_target.clone(),
    )?;
    project.reconnect_connection(
        manual_id,
        address(PortOwner::Clip(b), IMAGE_OUTPUT_PORT),
        first_target.clone(),
    )?;
    assert_eq!(
        target_inputs(&project, &first_target)
            .iter()
            .map(|connection| connection.from.owner)
            .collect::<Vec<_>>(),
        vec![PortOwner::Clip(a), PortOwner::Clip(b)]
    );
    assert_eq!(
        project
            .get_track(first_track)
            .context("first Track is missing")?
            .clip_ids,
        vec![a, b]
    );

    project.set_connection_blend_mode(manual_id, BlendMode::Overlay)?;
    project.reconnect_connection(
        manual_id,
        address(PortOwner::Clip(b), IMAGE_OUTPUT_PORT),
        second_target.clone(),
    )?;
    assert!(direct_child_edge(&project, &first_target, PortOwner::Clip(b)).is_none());
    assert_eq!(
        direct_child_edge(&project, &second_target, PortOwner::Clip(b))
            .context("reconnected second Track edge is missing")?
            .id,
        manual_id
    );
    project.attach_clip_to_track(second_track, b)?;
    let adopted = direct_child_edge(&project, &second_target, PortOwner::Clip(b))
        .context("adopted structural edge is missing")?;
    assert_eq!(adopted.id, manual_id);
    assert_eq!(adopted.blend_mode, BlendMode::Overlay);

    let via = add_merge_node(
        &mut project,
        NodeContainer::Track(second_track),
        "spliced Merge",
    )?;
    let upstream_id = project.splice_connection(
        manual_id,
        address(PortOwner::Node(via), MERGE_IMAGES_PORT),
        address(PortOwner::Node(via), IMAGE_OUTPUT_PORT),
    )?;
    let downstream = project
        .connections
        .iter()
        .find(|connection| connection.id == manual_id)
        .context("spliced downstream structural edge is missing")?;
    assert_eq!(downstream.from.owner, PortOwner::Node(via));
    assert_eq!(downstream.to, second_target);
    assert_eq!(downstream.blend_mode, BlendMode::Overlay);
    assert!(direct_child_edge(&project, &second_target, PortOwner::Clip(b)).is_none());
    assert_eq!(
        project
            .get_track(second_track)
            .context("second Track is missing")?
            .clip_ids,
        vec![b]
    );

    project.reorder_connection(manual_id, 0)?;
    assert_eq!(
        project
            .get_track(second_track)
            .context("second Track disappeared after reorder")?
            .clip_ids,
        vec![b]
    );
    assert!(project.disconnect_connection(upstream_id));
    project.attach_clip_to_track_at(second_track, b, Some(0))?;
    assert!(direct_child_edge(&project, &second_target, PortOwner::Clip(b)).is_none());
    Ok(())
}

#[test]
fn removing_children_normalizes_orders_and_structural_nodes_require_container_deletion()
-> Result<()> {
    let (mut project, composition_id, first_track, second_track) = two_track_project()?;
    let a = add_clip(&mut project, first_track, "A")?;
    let b = add_clip(&mut project, first_track, "B")?;
    let c = add_clip(&mut project, first_track, "C")?;
    let target = structural_target(&project, NodeContainer::Track(first_track))?;
    let custom_one = add_merge_node(
        &mut project,
        NodeContainer::Track(first_track),
        "custom one",
    )?;
    let custom_two = add_merge_node(
        &mut project,
        NodeContainer::Track(first_track),
        "custom two",
    )?;
    let custom_one_id = project.connect_ports(
        address(PortOwner::Node(custom_one), IMAGE_OUTPUT_PORT),
        target.clone(),
    )?;
    let custom_two_id = project.connect_ports(
        address(PortOwner::Node(custom_two), IMAGE_OUTPUT_PORT),
        target.clone(),
    )?;
    project.reorder_connection(custom_one_id, 1)?;
    project.reorder_connection(custom_two_id, 3)?;

    project
        .remove_clip(b)
        .context("removed Clip is missing from the Project")?;
    let remaining = target_inputs(&project, &target);
    assert_eq!(
        remaining
            .iter()
            .map(|connection| connection.order)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    let custom_ids = remaining
        .iter()
        .filter(|connection| {
            matches!(
                connection.from.owner,
                PortOwner::Node(id) if id == custom_one || id == custom_two
            )
        })
        .map(|connection| connection.id)
        .collect::<Vec<_>>();
    assert_eq!(custom_ids, vec![custom_one_id, custom_two_id]);
    assert_eq!(
        project
            .get_track(first_track)
            .context("first Track is missing after Clip removal")?
            .clip_ids,
        vec![a, c]
    );

    let structural_id = project
        .get_track(first_track)
        .context("first Track structural Merge owner is missing")?
        .structural_merge_node_id;
    let before_guarded_remove = project.clone();
    assert_eq!(
        project.remove_node(structural_id),
        Err(ProjectGraphError::CannotRemoveStructuralMerge {
            container: NodeContainer::Track(first_track),
            node_id: structural_id,
        })
    );
    assert_eq!(project, before_guarded_remove);

    project
        .remove_track(first_track)
        .context("removed Track is missing from the Project")?;
    assert!(project.get_node(structural_id).is_none());
    let composition_target =
        structural_target(&project, NodeContainer::Composition(composition_id))?;
    let composition_inputs = target_inputs(&project, &composition_target);
    assert_eq!(composition_inputs.len(), 1);
    assert_eq!(
        composition_inputs[0].from.owner,
        PortOwner::Track(second_track)
    );
    assert_eq!(composition_inputs[0].order, 0);
    Ok(())
}

#[test]
fn detaching_children_removes_only_their_persisted_structural_edges() -> Result<()> {
    let (mut project, composition_id, first_track, second_track) = two_track_project()?;
    let a = add_clip(&mut project, first_track, "A")?;
    let b = add_clip(&mut project, first_track, "B")?;
    let track_target = structural_target(&project, NodeContainer::Track(first_track))?;
    let composition_target =
        structural_target(&project, NodeContainer::Composition(composition_id))?;

    assert!(project.detach_clip(a));
    assert_eq!(
        project
            .get_track(first_track)
            .context("first Track is missing after detach")?
            .clip_ids,
        vec![b]
    );
    let track_inputs = target_inputs(&project, &track_target);
    assert_eq!(track_inputs.len(), 1);
    assert_eq!(track_inputs[0].from.owner, PortOwner::Clip(b));
    assert_eq!(track_inputs[0].order, 0);

    assert!(project.detach_track(second_track));
    assert_eq!(
        project
            .get_composition(composition_id)
            .context("Composition is missing after detach")?
            .track_ids,
        vec![first_track]
    );
    let composition_inputs = target_inputs(&project, &composition_target);
    assert_eq!(composition_inputs.len(), 1);
    assert_eq!(
        composition_inputs[0].from.owner,
        PortOwner::Track(first_track)
    );
    assert_eq!(composition_inputs[0].order, 0);
    Ok(())
}

#[test]
fn malformed_loaded_annotations_validate_and_produce_no_image_source() -> Result<()> {
    let (project, _, track_id) = one_track_project()?;
    let missing_id = Uuid::new_v4();
    let mut persisted = serde_json::to_value(&project)?;
    persisted["tracks"]
        .as_object_mut()
        .context("serialized tracks are not an object")?
        .get_mut(&track_id.to_string())
        .context("serialized Track is missing")?["structural_merge_node_id"] =
        serde_json::json!(missing_id);
    let malformed: Project = serde_json::from_value(persisted)?;
    assert!(malformed.validate_connections().contains(
        &ProjectGraphError::StructuralMergeNodeMissing {
            container: NodeContainer::Track(track_id),
            node_id: missing_id,
        }
    ));
    assert!(
        malformed
            .container_image_sources(PortOwner::Track(track_id))
            .is_empty()
    );

    let mut wrong_type_project = project;
    let non_merge = Node::new_fmod("not a Merge");
    let non_merge_id = non_merge.id;
    wrong_type_project.add_node(non_merge);
    wrong_type_project.attach_node_to_container(NodeContainer::Track(track_id), non_merge_id)?;
    let mut persisted = serde_json::to_value(wrong_type_project)?;
    persisted["tracks"]
        .as_object_mut()
        .context("serialized tracks are not an object")?
        .get_mut(&track_id.to_string())
        .context("serialized Track is missing")?["structural_merge_node_id"] =
        serde_json::json!(non_merge_id);
    let wrong_type: Project = serde_json::from_value(persisted)?;
    assert!(wrong_type.validate_connections().contains(
        &ProjectGraphError::StructuralMergeNodeWrongType {
            container: NodeContainer::Track(track_id),
            node_id: non_merge_id,
        }
    ));
    assert!(
        wrong_type
            .container_image_sources(PortOwner::Track(track_id))
            .is_empty()
    );
    Ok(())
}

#[test]
fn no_output_and_downstream_output_are_explicit_and_timeline_safe() -> Result<()> {
    let (mut project, _, track_id) = one_track_project()?;
    let structural_id = project
        .get_track(track_id)
        .context("Track structural Merge owner is missing")?
        .structural_merge_node_id;
    let downstream_id = add_merge_node(
        &mut project,
        NodeContainer::Track(track_id),
        "downstream output",
    )?;
    project.connect_ports(
        address(PortOwner::Node(structural_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(downstream_id), MERGE_IMAGES_PORT),
    )?;
    project.set_output_node(NodeContainer::Track(track_id), Some(downstream_id))?;
    let a = add_clip(&mut project, track_id, "A")?;
    let b = add_clip(&mut project, track_id, "B")?;
    project.attach_clip_to_track_at(track_id, b, Some(0))?;
    project
        .remove_clip(a)
        .context("removed Clip is missing from the Project")?;
    assert_eq!(
        project
            .get_track(track_id)
            .context("Track output owner is missing")?
            .output_node_id,
        Some(downstream_id)
    );
    assert_eq!(
        project.container_image_sources(PortOwner::Track(track_id))[0].source,
        PortOwner::Node(downstream_id)
    );

    let unrelated_id = add_merge_node(
        &mut project,
        NodeContainer::Track(track_id),
        "unrelated branch",
    )?;
    assert_eq!(
        project.set_output_node(NodeContainer::Track(track_id), Some(unrelated_id)),
        Err(ProjectGraphError::StructuralMergeDoesNotReachOutput {
            container: NodeContainer::Track(track_id),
            node_id: structural_id,
            output_node_id: unrelated_id,
        })
    );
    assert_eq!(
        project
            .get_track(track_id)
            .context("Track output owner is missing after rejected binding")?
            .output_node_id,
        Some(downstream_id)
    );

    project.set_output_node(NodeContainer::Track(track_id), None)?;
    assert!(
        project
            .container_image_sources(PortOwner::Track(track_id))
            .is_empty()
    );
    assert!(!project.validate_connections().iter().any(|error| matches!(
        error,
        ProjectGraphError::StructuralMergeDoesNotReachOutput {
            container: NodeContainer::Track(id),
            ..
        } if *id == track_id
    )));
    Ok(())
}
