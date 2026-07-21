use super::*;

#[test]
fn malformed_foreign_image_binding_is_no_output_after_deserialization() {
    let mut project = Project::new("malformed foreign image binding");
    let (composition, track) = Composition::new("Main", 64, 64, 24.0, 2.0);
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let first_clip = Clip::new("First", 0.0, 1.0);
    let first_clip_id = first_clip.id;
    project.add_clip(first_clip);
    project
        .attach_clip_to_track(track_id, first_clip_id)
        .unwrap();

    let second_clip = Clip::new("Second", 1.0, 1.0);
    let second_clip_id = second_clip.id;
    project.add_clip(second_clip);
    project
        .attach_clip_to_track(track_id, second_clip_id)
        .unwrap();

    let foreign_node = Node::new_merge("Foreign Image");
    let foreign_node_id = foreign_node.id;
    project.add_node(foreign_node);
    project
        .attach_node_to_container(NodeContainer::Clip(second_clip_id), foreign_node_id)
        .unwrap();

    let mut persisted = serde_json::to_value(&project).unwrap();
    let first = persisted["clips"]
        .as_object_mut()
        .unwrap()
        .get_mut(&first_clip_id.to_string())
        .unwrap();
    first["output_node_id"] = serde_json::json!(foreign_node_id);

    let malformed: Project = serde_json::from_value(persisted).unwrap();
    assert_eq!(
        malformed.find_node_container(foreign_node_id),
        Some(NodeContainer::Clip(second_clip_id))
    );
    assert!(
        malformed
            .container_image_sources(PortOwner::Clip(first_clip_id))
            .is_empty()
    );
}

#[test]
fn container_insertion_places_typed_structural_merges_without_overlap() {
    let mut project = Project::new("structural merge placement");
    let (composition, track) = Composition::new("Main", 64, 64, 24.0, 2.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();

    let composition = project.get_composition(composition_id).unwrap();
    let track = project.get_track(track_id).unwrap();
    let contains = |outer_position: [f32; 2],
                    outer_size: [f32; 2],
                    inner_position: [f32; 2],
                    inner_size: [f32; 2]| {
        inner_position[0] >= outer_position[0]
            && inner_position[1] >= outer_position[1]
            && inner_position[0] + inner_size[0] <= outer_position[0] + outer_size[0]
            && inner_position[1] + inner_size[1] <= outer_position[1] + outer_size[1]
    };
    let overlaps = |left_position: [f32; 2],
                    left_size: [f32; 2],
                    right_position: [f32; 2],
                    right_size: [f32; 2]| {
        left_position[0] < right_position[0] + right_size[0]
            && left_position[0] + left_size[0] > right_position[0]
            && left_position[1] < right_position[1] + right_size[1]
            && left_position[1] + left_size[1] > right_position[1]
    };
    assert!(contains(
        composition.ui_position,
        composition.ui_size,
        track.ui_position,
        track.ui_size,
    ));

    for (container_position, container_size, image_merge_id, sound_merge_id) in [
        {
            let track = project.get_track(track_id).unwrap();
            (
                track.ui_position,
                track.ui_size,
                track.structural_merge_node_id,
                track.structural_sound_merge_node_id,
            )
        },
        {
            let composition = project.get_composition(composition_id).unwrap();
            (
                composition.ui_position,
                composition.ui_size,
                composition.structural_merge_node_id,
                composition.structural_sound_merge_node_id,
            )
        },
    ] {
        let image_merge = project.get_node(image_merge_id).unwrap();
        let sound_merge = project.get_node(sound_merge_id).unwrap();
        assert!(contains(
            container_position,
            container_size,
            image_merge.ui_position,
            image_merge.ui_size,
        ));
        assert!(contains(
            container_position,
            container_size,
            sound_merge.ui_position,
            sound_merge.ui_size,
        ));
        assert_eq!(image_merge.ui_position[0], sound_merge.ui_position[0]);
        assert!(
            image_merge.ui_position[1] + image_merge.ui_size[1] < sound_merge.ui_position[1],
            "model insertion must place Sound Merge below Image Merge before the UI opens"
        );
    }

    for merge_id in [
        composition.structural_merge_node_id,
        composition.structural_sound_merge_node_id,
    ] {
        let merge = project.get_node(merge_id).unwrap();
        assert!(!overlaps(
            track.ui_position,
            track.ui_size,
            merge.ui_position,
            merge.ui_size,
        ));
        assert!(
            track.ui_position[0] + track.ui_size[0] < merge.ui_position[0],
            "both typed Track -> Composition structural edges must run left-to-right"
        );
    }
}

#[test]
fn structural_validation_reports_missing_duplicate_and_noncanonical_typed_edges() {
    let mut project = Project::new("malformed structural edges");
    let (composition, track) = Composition::new("Main", 64, 64, 24.0, 2.0);
    let track_id = track.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    for name in ["First", "Second"] {
        let clip = Clip::new(name, 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();
    }
    assert!(project.validate_connections().is_empty());
    let track = project.get_track(track_id).unwrap();
    let first_child = PortOwner::Clip(track.clip_ids[0]);
    for (merge_id, source_port, target_port) in [
        (
            track.structural_merge_node_id,
            IMAGE_OUTPUT_PORT,
            MERGE_IMAGES_PORT,
        ),
        (
            track.structural_sound_merge_node_id,
            AUDIO_OUTPUT_PORT,
            MERGE_SOUNDS_PORT,
        ),
    ] {
        let target = PortAddress::new(PortOwner::Node(merge_id), target_port);
        let edge = project
            .connections
            .iter()
            .find(|connection| {
                connection.from == PortAddress::new(first_child, source_port)
                    && connection.to == target
            })
            .unwrap()
            .clone();

        let mut missing = project.clone();
        missing
            .connections
            .retain(|connection| connection.id != edge.id);
        assert!(missing.validate_connections().contains(
            &ProjectGraphError::MissingStructuralEdge {
                container: NodeContainer::Track(track_id),
                node_id: merge_id,
                child: first_child,
            }
        ));

        let mut duplicate = project.clone();
        let mut duplicate_edge = edge.clone();
        duplicate_edge.id = Uuid::new_v4();
        duplicate_edge.order = duplicate.connections.len() as i64;
        duplicate.connections.push(duplicate_edge);
        assert!(duplicate.validate_connections().contains(
            &ProjectGraphError::DuplicateStructuralChildEdge {
                container: NodeContainer::Track(track_id),
                node_id: merge_id,
                child: first_child,
            }
        ));

        let mut wrong_order = project.clone();
        wrong_order
            .connections
            .iter_mut()
            .find(|connection| connection.id == edge.id)
            .unwrap()
            .order = 7;
        assert!(wrong_order.validate_connections().contains(
            &ProjectGraphError::StructuralOrderMismatch {
                container: NodeContainer::Track(track_id),
                node_id: merge_id,
                child: first_child,
                expected_order: 0,
                actual_order: 7,
            }
        ));

        let mut wrong_source_port = project.clone();
        wrong_source_port
            .connections
            .iter_mut()
            .find(|connection| connection.id == edge.id)
            .unwrap()
            .from
            .port = "not_the_typed_output".to_string();
        assert!(wrong_source_port.validate_connections().contains(
            &ProjectGraphError::MissingStructuralEdge {
                container: NodeContainer::Track(track_id),
                node_id: merge_id,
                child: first_child,
            }
        ));
    }
}

#[test]
fn image_and_sound_structural_merges_share_ordered_canonical_contract() {
    let mut project = Project::new("typed structural merges");
    let (composition, track) = Composition::new("Main", 64, 64, 24.0, 2.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();

    let first = Clip::new("First", 0.0, 1.0);
    let first_id = first.id;
    project.add_clip(first);
    project.attach_clip_to_track(track_id, first_id).unwrap();
    let second = Clip::new("Second", 1.0, 1.0);
    let second_id = second.id;
    project.add_clip(second);
    project.attach_clip_to_track(track_id, second_id).unwrap();

    let track = project.get_track(track_id).unwrap();
    let contracts = [
        (
            track.structural_merge_node_id,
            IMAGE_OUTPUT_PORT,
            MERGE_IMAGES_PORT,
            PortDataType::Image,
        ),
        (
            track.structural_sound_merge_node_id,
            AUDIO_OUTPUT_PORT,
            MERGE_SOUNDS_PORT,
            PortDataType::Audio,
        ),
    ];
    for (merge_id, source_port, target_port, data_type) in contracts {
        let mut edges = project
            .connections
            .iter()
            .filter(|connection| {
                connection.to == PortAddress::new(PortOwner::Node(merge_id), target_port)
            })
            .collect::<Vec<_>>();
        edges.sort_by_key(|connection| (connection.order, connection.id));
        assert_eq!(
            edges
                .iter()
                .map(|connection| {
                    (
                        connection.from.port.clone(),
                        connection.from.owner,
                        connection.order,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (source_port.to_string(), PortOwner::Clip(first_id), 0),
                (source_port.to_string(), PortOwner::Clip(second_id), 1),
            ]
        );
        assert_eq!(
            project
                .port_definition(
                    &PortAddress::new(PortOwner::Node(merge_id), target_port),
                    PortDirection::Input,
                )
                .map(|port| (port.data_type, port.multiplicity)),
            Some((data_type, PortMultiplicity::Variadic))
        );
    }

    let composition = project.get_composition(composition_id).unwrap();
    assert_eq!(
        project.container_audio_sources(PortOwner::Composition(composition_id)),
        vec![ContainerAudioSource {
            source: PortOwner::Node(composition.structural_sound_merge_node_id),
            kind: ContainerAudioSourceKind::OutputBinding,
        }]
    );

    let sound_target = PortAddress::new(
        PortOwner::Node(track.structural_sound_merge_node_id),
        MERGE_SOUNDS_PORT,
    );
    let first_sound_edge = project
        .connections
        .iter()
        .find(|connection| {
            connection.from == PortAddress::new(PortOwner::Clip(first_id), AUDIO_OUTPUT_PORT)
                && connection.to == sound_target
        })
        .unwrap()
        .id;
    project.reorder_connection(first_sound_edge, 1).unwrap();
    assert_eq!(
        project.get_track(track_id).unwrap().clip_ids,
        vec![second_id, first_id]
    );
    for target_port in [MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT] {
        let merge_id = if target_port == MERGE_IMAGES_PORT {
            project
                .get_track(track_id)
                .unwrap()
                .structural_merge_node_id
        } else {
            project
                .get_track(track_id)
                .unwrap()
                .structural_sound_merge_node_id
        };
        let mut owners = project
            .connections
            .iter()
            .filter(|connection| {
                connection.to == PortAddress::new(PortOwner::Node(merge_id), target_port)
            })
            .map(|connection| (connection.order, connection.from.owner))
            .collect::<Vec<_>>();
        owners.sort_by_key(|(order, owner)| (*order, owner.id()));
        assert_eq!(
            owners
                .into_iter()
                .map(|(_, owner)| owner)
                .collect::<Vec<_>>(),
            vec![PortOwner::Clip(second_id), PortOwner::Clip(first_id)]
        );
    }
}
