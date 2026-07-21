use anyhow::{Context, Result, anyhow};
use library::model::project::{
    CompositionSettingsError, DURATION_PORT, FPS_PORT, FRAME_PORT, IMAGE_OUTPUT_PORT,
    MERGE_IMAGES_PORT, NodeContainer, NodeGraphBundle, PortDataType, PortDefinition, PortDirection,
    PortOwner, ProjectConnection, ProjectGraphError, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::{Asset, AssetKind, Node, Track};
use uuid::Uuid;

use super::graph_support::{
    add_clip, add_node, address, graph_output, plugin_operation_node, project_with_composition,
    solid_node,
};

#[test]
fn descendant_value_cannot_override_ancestor_scope_but_internal_metadata_can_feed_child()
-> Result<()> {
    let (mut project, composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "scope cycle")?;
    let operation = plugin_operation_node(
        "Scope Value",
        "utility",
        "dev.example.scope-value",
        "number.produce",
        vec![
            PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
            graph_output("value", "Value", PortDataType::Number),
        ],
    );
    let operation_id = operation.id;
    project.insert_node_graph(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(vec![operation], Vec::new(), None),
    )?;

    let source = address(PortOwner::Node(operation_id), "value");
    for target in [
        address(PortOwner::Clip(clip_id), TIME_PORT),
        address(PortOwner::Track(track_id), DURATION_PORT),
        address(PortOwner::Composition(composition_id), DURATION_PORT),
    ] {
        assert_eq!(
            project.connect_ports(source.clone(), target.clone()),
            Err(ProjectGraphError::ConnectionCycle {
                from: PortOwner::Node(operation_id),
                to: target.owner,
            })
        );
    }
    let structural_connection_count = project.connections.len();

    for read_only in [
        address(PortOwner::Track(track_id), FPS_PORT),
        address(PortOwner::Composition(composition_id), FRAME_PORT),
    ] {
        assert_eq!(
            project.connect_ports(source.clone(), read_only.clone()),
            Err(ProjectGraphError::PortNotFound(read_only))
        );
    }

    project.connect_ports(
        address(PortOwner::Clip(clip_id), FPS_PORT),
        address(PortOwner::Node(operation_id), TIME_PORT),
    )?;
    assert_eq!(project.connections.len(), structural_connection_count + 1);
    assert!(project.validate_connections().is_empty());
    Ok(())
}

#[test]
fn node_graph_bundle_commit_and_structural_failure_are_atomic() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "atomic graph")?;

    let invalid = plugin_operation_node(
        "Duplicate ports",
        "effector",
        "dev.example.invalid",
        "shape.transform",
        vec![
            graph_output(SHAPE_OUTPUT_PORT, "First", PortDataType::Shape),
            graph_output(SHAPE_OUTPUT_PORT, "Second", PortDataType::Shape),
        ],
    );
    let invalid_id = invalid.id;
    let before_invalid_insert = project.clone();
    assert_eq!(
        project.insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![invalid], Vec::new(), None),
        ),
        Err(ProjectGraphError::DuplicateNodePort {
            node_id: invalid_id,
            key: SHAPE_OUTPUT_PORT.to_string(),
            direction: PortDirection::Output,
        })
    );
    assert_eq!(project, before_invalid_insert);

    let detached = solid_node("detached");
    let detached_id = detached.id;
    let unrelated_connection = ProjectConnection::new(
        address(PortOwner::Composition(_composition_id), TIME_PORT),
        address(PortOwner::Track(track_id), TIME_PORT),
        0,
    );
    let unrelated_connection_id = unrelated_connection.id;
    let before_unrelated_wire = project.clone();
    assert_eq!(
        project.insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(
                vec![detached],
                vec![unrelated_connection],
                Some(detached_id),
            ),
        ),
        Err(ProjectGraphError::NodeGraphConnectionOutsideBundle(
            unrelated_connection_id,
        ))
    );
    assert_eq!(project, before_unrelated_wire);

    let source = solid_node("source");
    let source_id = source.id;
    let merge = Node::new_merge("merge");
    let merge_id = merge.id;
    let malformed_connection = ProjectConnection::new(
        address(PortOwner::Node(source_id), "missing_output"),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        0,
    );
    let before_bad_wire = project.clone();
    assert_eq!(
        project.insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(
                vec![source.clone(), merge.clone()],
                vec![malformed_connection],
                Some(merge_id),
            ),
        ),
        Err(ProjectGraphError::PortNotFound(address(
            PortOwner::Node(source_id),
            "missing_output",
        )))
    );
    assert_eq!(project, before_bad_wire);

    let connection = ProjectConnection::new(
        address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        3,
    );
    let connection_id = connection.id;
    project.insert_node_graph_at(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(vec![source, merge], vec![connection], Some(merge_id)),
        Some(0),
    )?;

    assert_eq!(
        project
            .get_clip(clip_id)
            .context("atomic graph Clip must exist")?
            .node_ids,
        vec![source_id, merge_id]
    );
    assert_eq!(
        project
            .get_clip(clip_id)
            .context("atomic graph Clip must exist")?
            .output_node_id,
        Some(merge_id)
    );
    let inserted_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)
        .context("bundled connection must be committed")?;
    assert_eq!(inserted_connection.order, 3);
    assert!(project.validate_connections().is_empty());
    Ok(())
}

#[test]
fn containment_is_exact_and_reparenting_does_not_duplicate_ownership() -> Result<()> {
    let (mut project, composition_id, first_track_id) = project_with_composition();
    let second_track = Track::new("second");
    let second_track_id = second_track.id;
    assert!(
        project.add_track(second_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.attach_track_to_composition(composition_id, second_track_id)?;

    let clip_id = add_clip(&mut project, first_track_id, "movable")?;
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("contained once"),
    )?;
    project.attach_clip_to_track(second_track_id, clip_id)?;
    project
        .attach_node_to_container(NodeContainer::Track(first_track_id), node_id)
        .map_err(|error| anyhow!(error))?;

    assert!(
        !project
            .get_track(first_track_id)
            .context("first Track must exist")?
            .clip_ids
            .contains(&clip_id)
    );
    assert_eq!(
        project
            .get_track(second_track_id)
            .context("second Track must exist")?
            .clip_ids,
        vec![clip_id]
    );
    assert!(
        project
            .get_clip(clip_id)
            .context("movable Clip must exist")?
            .node_ids
            .is_empty()
    );
    let first_track = project
        .get_track(first_track_id)
        .context("first Track must exist")?;
    assert_eq!(
        first_track.node_ids,
        vec![first_track.structural_merge_node_id, node_id]
    );
    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Track(first_track_id))
    );
    assert!(project.validate_containment().is_empty());

    let orphan = solid_node("orphan");
    let orphan_id = orphan.id;
    project.add_node(orphan);
    assert!(
        project
            .validate_containment()
            .contains(&ProjectGraphError::NodeHasNoContainer(orphan_id))
    );
    Ok(())
}

#[test]
fn validation_reports_identity_and_composition_invariants() -> Result<()> {
    let (project, composition_id, track_id) = project_with_composition();

    let mut duplicate_composition = project.clone();
    duplicate_composition
        .compositions
        .push(duplicate_composition.compositions[0].clone());
    assert!(
        duplicate_composition
            .validate_connections()
            .contains(&ProjectGraphError::DuplicateCompositionId(composition_id))
    );

    let mut bad_track_key = project.clone();
    let track = bad_track_key
        .tracks
        .get(&track_id)
        .context("fixture Track must exist")?
        .clone();
    let wrong_track_key = Uuid::new_v4();
    bad_track_key.tracks.insert(wrong_track_key, track);
    assert!(
        bad_track_key
            .validate_connections()
            .contains(&ProjectGraphError::TrackKeyMismatch {
                key: wrong_track_key,
                entity_id: track_id,
            })
    );
    let issue = ProjectGraphError::TrackKeyMismatch {
        key: wrong_track_key,
        entity_id: track_id,
    };
    let serialized_issue = serde_json::to_value(&issue)?;
    assert_eq!(serialized_issue["code"], "track_key_mismatch");
    assert_eq!(
        serialized_issue["context"]["key"],
        wrong_track_key.to_string()
    );
    assert_eq!(
        serialized_issue["context"]["entity_id"],
        track_id.to_string()
    );

    let mut invalid_settings = project.clone();
    invalid_settings
        .get_composition_mut(composition_id)
        .context("fixture Composition must exist")?
        .width = 0;
    assert!(invalid_settings.validate_connections().contains(
        &ProjectGraphError::InvalidCompositionSettings {
            composition_id,
            reason: CompositionSettingsError::WidthZero,
        }
    ));

    let mut unrepresentable_frame_count = project.clone();
    let composition = unrepresentable_frame_count
        .get_composition_mut(composition_id)
        .context("fixture Composition must exist")?;
    composition.fps = f64::MAX;
    composition.duration = f64::MAX;
    assert!(unrepresentable_frame_count.validate_connections().contains(
        &ProjectGraphError::InvalidCompositionSettings {
            composition_id,
            reason: CompositionSettingsError::FrameCountOutOfRange,
        }
    ));

    let mut invalid_work_area = project;
    invalid_work_area
        .get_composition_mut(composition_id)
        .context("fixture Composition must exist")?
        .work_area_out = 301;
    assert!(invalid_work_area.validate_connections().contains(
        &ProjectGraphError::InvalidCompositionWorkArea {
            composition_id,
            work_area_in: 0,
            work_area_out: 301,
            frame_count: 300,
        }
    ));
    Ok(())
}

#[test]
fn validation_reports_clip_node_asset_and_connection_identity_corruption() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip")?;
    let source_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("source"),
    )?;
    let merge_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new_merge("merge"),
    )?;
    let connection_id = project.connect_ports(
        address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )?;
    let duplicate_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)
        .context("fixture connection must exist")?
        .clone();
    project.connections.push(duplicate_connection);

    let clip = project
        .get_clip(clip_id)
        .context("fixture Clip must exist")?
        .clone();
    let wrong_clip_key = Uuid::new_v4();
    project.clips.insert(wrong_clip_key, clip);
    let node = project
        .get_node(source_id)
        .context("fixture source Node must exist")?
        .clone();
    let wrong_node_key = Uuid::new_v4();
    project.nodes.insert(wrong_node_key, node);
    let asset = Asset::new("duplicate", "duplicate.png", AssetKind::Image);
    let asset_id = asset.id;
    project.assets.push(asset.clone());
    project.assets.push(asset);

    let errors = project.validate_connections();
    assert!(errors.contains(&ProjectGraphError::ClipKeyMismatch {
        key: wrong_clip_key,
        entity_id: clip_id,
    }));
    assert!(errors.contains(&ProjectGraphError::NodeKeyMismatch {
        key: wrong_node_key,
        entity_id: source_id,
    }));
    assert!(errors.contains(&ProjectGraphError::DuplicateAssetId(asset_id)));
    assert!(errors.contains(&ProjectGraphError::DuplicateConnectionId(connection_id)));
    Ok(())
}
