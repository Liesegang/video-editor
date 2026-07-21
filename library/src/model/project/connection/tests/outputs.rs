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

    for (image_merge_id, sound_merge_id) in [
        {
            let track = project.get_track(track_id).unwrap();
            (
                track.structural_merge_node_id,
                track.structural_sound_merge_node_id,
            )
        },
        {
            let composition = project.get_composition(composition_id).unwrap();
            (
                composition.structural_merge_node_id,
                composition.structural_sound_merge_node_id,
            )
        },
    ] {
        let image_merge = project.get_node(image_merge_id).unwrap();
        let sound_merge = project.get_node(sound_merge_id).unwrap();
        assert_eq!(image_merge.ui_position[0], sound_merge.ui_position[0]);
        assert!(
            image_merge.ui_position[1] + image_merge.ui_size[1] < sound_merge.ui_position[1],
            "model insertion must place Sound Merge below Image Merge before the UI opens"
        );
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
