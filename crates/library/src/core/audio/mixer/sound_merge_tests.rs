use super::tests::{
    TestAudioFiles, add_audio_node, attach_audio_output, connect_attached_audio_output,
};
use super::*;
use crate::model::asset::{Asset, AssetKind};
use crate::model::project::{
    AUDIO_OUTPUT_PORT, FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, IMAGE_OUTPUT_PORT,
    MERGE_SOUNDS_PORT, NUMBER_RESULT_OUTPUT_PORT, PortAddress, PortDataType, PortDefinition,
    PortExposure, PortOwner, PortSide, ProjectGraphError, TIME_PORT,
};
use crate::model::property::{Property, PropertyValue};
use crate::model::{Clip, MediaContent, MediaOutputSelection, Node, NodeContainer};
use ordered_float::OrderedFloat;

fn attach_time_fmod(
    project: &mut Project,
    track_id: uuid::Uuid,
    timed_node_id: uuid::Uuid,
) -> uuid::Uuid {
    let mut fmod = Node::new_fmod("half-second audio loop");
    fmod.set_property(
        FMOD_DIVISOR_INPUT_PORT.to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
    )
    .unwrap();
    let fmod_id = fmod.id;
    project.add_node(fmod);
    project
        .attach_node_to_container(NodeContainer::Track(track_id), fmod_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Track(track_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(fmod_id), FMOD_X_INPUT_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(fmod_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(timed_node_id), TIME_PORT),
        )
        .unwrap();
    fmod_id
}

fn attach_clip_time_fmod(
    project: &mut Project,
    clip_id: uuid::Uuid,
    timed_node_id: uuid::Uuid,
) -> uuid::Uuid {
    let mut fmod = Node::new_fmod("half-second Clip-local audio loop");
    fmod.set_property(
        FMOD_DIVISOR_INPUT_PORT.to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
    )
    .unwrap();
    let fmod_id = fmod.id;
    project.add_node(fmod);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), fmod_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(fmod_id), FMOD_X_INPUT_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(fmod_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(timed_node_id), TIME_PORT),
        )
        .unwrap();
    fmod_id
}

fn bypassed_audio_operation() -> Node {
    let mut persisted = serde_json::to_value(Node::new_merge("bypassed audio operation")).unwrap();
    persisted["content"] = serde_json::json!({
        "type": "PluginOperation",
        "data": {
            "category": "audio_effect",
            "component_id": "not-installed",
            "operation": "audio.effect.v1",
            "declared_ports": [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::input("audio_in", "Audio", PortDataType::Audio),
                PortDefinition::output(
                    AUDIO_OUTPUT_PORT,
                    "Audio",
                    PortDataType::Audio,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
        },
    });
    let mut node: Node = serde_json::from_value(persisted).unwrap();
    node.bypassed = true;
    node
}

#[test]
fn bypass_routes_first_ordered_input_and_disable_is_no_output() {
    let mut project = Project::new("Sound Merge state");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let sound_merge_id = track.structural_sound_merge_node_id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();

    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    for (name, sample) in [("first", 0.25), ("second", 0.5)] {
        let clip = Clip::new(name, 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();
        let node_id = add_audio_node(&mut project, &cache, &mut files, vec![sample; 4]);
        attach_audio_output(&mut project, NodeContainer::Clip(clip_id), node_id);
    }
    let mix = |project: &Project| {
        mix_samples(
            &project.assets,
            project,
            project.get_composition(composition_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &PluginManager::default(),
        )
    };
    assert_eq!(mix(&project), vec![0.75; 4]);

    assert!(project.get_node(sound_merge_id).unwrap().supports_bypass());
    project.get_node_mut(sound_merge_id).unwrap().bypassed = true;
    assert_eq!(
        mix(&project),
        vec![0.25; 4],
        "bypass must retain only the first canonical Sound input"
    );

    project.get_node_mut(sound_merge_id).unwrap().enabled = false;
    assert_eq!(
        mix(&project),
        vec![0.0; 4],
        "disabled Sound Merge must be NoOutput rather than implicit passthrough"
    );
}

#[test]
fn parent_sources_join_children_and_direct_bindings_fail_transactionally() {
    let mut project = Project::new("audio canonical parent nodes");
    let (composition, mut track) = Composition::new("main", 1920, 1080, 30.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    track.properties.set(
        "volume".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
    );
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();

    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let composition_node = add_audio_node(&mut project, &cache, &mut files, vec![1.0; 4]);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), composition_node)
        .unwrap();
    let track_node = add_audio_node(&mut project, &cache, &mut files, vec![2.0; 4]);
    project
        .attach_node_to_container(NodeContainer::Track(track_id), track_node)
        .unwrap();
    let clip = Clip::new("audio clip", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    let clip_node = add_audio_node(&mut project, &cache, &mut files, vec![4.0; 4]);
    attach_audio_output(&mut project, NodeContainer::Clip(clip_id), clip_node);

    let mix = |project: &Project| {
        mix_samples(
            &project.assets,
            project,
            project.get_composition(composition_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &PluginManager::default(),
        )
    };
    assert_eq!(mix(&project), vec![2.0; 4]);

    let before_invalid_binding = project.clone();
    assert!(matches!(
        project.set_audio_output_node(NodeContainer::Track(track_id), Some(track_node)),
        Err(ProjectGraphError::StructuralMergeDoesNotReachOutput { .. })
    ));
    assert_eq!(project, before_invalid_binding);
    connect_attached_audio_output(&mut project, NodeContainer::Track(track_id), track_node);
    assert_eq!(mix(&project), vec![3.0; 4]);

    let before_invalid_binding = project.clone();
    assert!(matches!(
        project.set_audio_output_node(
            NodeContainer::Composition(composition_id),
            Some(composition_node),
        ),
        Err(ProjectGraphError::StructuralMergeDoesNotReachOutput { .. })
    ));
    assert_eq!(project, before_invalid_binding);
    connect_attached_audio_output(
        &mut project,
        NodeContainer::Composition(composition_id),
        composition_node,
    );
    assert_eq!(mix(&project), vec![4.0; 4]);
}

#[test]
fn sound_merge_scope_gates_no_output_and_propagates_explicit_time() {
    let mut project = Project::new("Sound Merge time scope");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let sound_merge_id = track.structural_sound_merge_node_id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let media_id = add_audio_node(
        &mut project,
        &cache,
        &mut files,
        (0..8).map(|sample| sample as f32).collect(),
    );
    attach_audio_output(&mut project, NodeContainer::Track(track_id), media_id);
    let fmod_id = attach_time_fmod(&mut project, track_id, sound_merge_id);
    let mix = |project: &Project| {
        mix_samples(
            &project.assets,
            project,
            project.get_composition(composition_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &PluginManager::default(),
        )
    };
    assert_eq!(mix(&project), vec![0.0, 1.0, 0.0, 1.0]);

    project.get_node_mut(fmod_id).unwrap().enabled = false;
    assert_eq!(mix(&project), vec![0.0; 4]);
}

#[test]
fn bypassed_audio_plugin_scope_gates_no_output_and_propagates_explicit_time() {
    let mut project = Project::new("bypassed audio plugin time scope");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let sound_merge_id = track.structural_sound_merge_node_id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let media_id = add_audio_node(
        &mut project,
        &cache,
        &mut files,
        (0..8).map(|sample| sample as f32).collect(),
    );
    project
        .attach_node_to_container(NodeContainer::Track(track_id), media_id)
        .unwrap();
    let operation = bypassed_audio_operation();
    let operation_id = operation.id;
    project.add_node(operation);
    project
        .attach_node_to_container(NodeContainer::Track(track_id), operation_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(operation_id), "audio_in"),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(operation_id), AUDIO_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(sound_merge_id), MERGE_SOUNDS_PORT),
        )
        .unwrap();
    let fmod_id = attach_time_fmod(&mut project, track_id, operation_id);
    let mix = |project: &Project| {
        mix_samples(
            &project.assets,
            project,
            project.get_composition(composition_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &PluginManager::default(),
        )
    };
    assert_eq!(mix(&project), vec![0.0, 1.0, 0.0, 1.0]);

    project.get_node_mut(fmod_id).unwrap().enabled = false;
    assert_eq!(mix(&project), vec![0.0; 4]);
}

#[test]
fn sound_merge_time_remap_in_nonzero_clip_preserves_composition_activity_space() {
    let mut project = Project::new("Clip-local Sound Merge time scope");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 4.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    let clip = Clip::new("late audio", 2.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let media_id = add_audio_node(
        &mut project,
        &cache,
        &mut files,
        (0..8).map(|sample| sample as f32).collect(),
    );
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), media_id)
        .unwrap();
    let sound_merge = Node::new_sound_merge("Clip Sound Merge");
    let sound_merge_id = sound_merge.id;
    project.add_node(sound_merge);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), sound_merge_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(sound_merge_id), MERGE_SOUNDS_PORT),
        )
        .unwrap();
    project
        .set_audio_output_node(NodeContainer::Clip(clip_id), Some(sound_merge_id))
        .unwrap();
    attach_clip_time_fmod(&mut project, clip_id, sound_merge_id);

    let mixed = mix_samples(
        &project.assets,
        &project,
        project.get_composition(composition_id).unwrap(),
        &cache,
        8,
        4,
        4,
        1,
        &PluginManager::default(),
    );
    assert_eq!(
        mixed,
        vec![0.0, 1.0, 0.0, 1.0],
        "Composition time must gate the Clip while explicit Time supplies source-local media time"
    );
}

#[test]
fn bypass_time_remap_in_nonzero_clip_preserves_composition_activity_space() {
    let mut project = Project::new("Clip-local bypass time scope");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 4.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    let clip = Clip::new("late audio", 2.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let media_id = add_audio_node(
        &mut project,
        &cache,
        &mut files,
        (0..8).map(|sample| sample as f32).collect(),
    );
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), media_id)
        .unwrap();
    let operation = bypassed_audio_operation();
    let operation_id = operation.id;
    project.add_node(operation);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), operation_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(operation_id), "audio_in"),
        )
        .unwrap();
    project
        .set_audio_output_node(NodeContainer::Clip(clip_id), Some(operation_id))
        .unwrap();
    attach_clip_time_fmod(&mut project, clip_id, operation_id);

    let mixed = mix_samples(
        &project.assets,
        &project,
        project.get_composition(composition_id).unwrap(),
        &cache,
        8,
        4,
        4,
        1,
        &PluginManager::default(),
    );
    assert_eq!(
        mixed,
        vec![0.0, 1.0, 0.0, 1.0],
        "bypass must propagate source time without reinterpreting it as Composition time"
    );
}

#[test]
fn malformed_sound_merge_edges_fail_closed_by_source_type_identity_duplicate_and_order() {
    let mut project = Project::new("malformed Sound Merge routes");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    let sound_merge_id = track.structural_sound_merge_node_id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let media_id = add_audio_node(&mut project, &cache, &mut files, vec![1.0; 4]);
    let edge_id =
        attach_audio_output(&mut project, NodeContainer::Track(track_id), media_id).unwrap();
    let target = PortAddress::new(PortOwner::Node(sound_merge_id), MERGE_SOUNDS_PORT);
    let assert_closed = |malformed: &Project| {
        assert!(
            routed_audio_media_nodes(malformed, PortOwner::Composition(composition_id)).is_empty()
        );
        assert_eq!(
            mix_samples(
                &malformed.assets,
                malformed,
                malformed.get_composition(composition_id).unwrap(),
                &cache,
                0,
                4,
                4,
                1,
                &PluginManager::default(),
            ),
            vec![0.0; 4]
        );
    };

    let mut image_to_sounds = project.clone();
    let image_asset = Asset::new("image", "/fixture/image.png", AssetKind::Image);
    let image_node = Node::from_media_converter(
        "image",
        MediaContent::new(image_asset.id, MediaOutputSelection::Image, None, None).unwrap(),
        &[],
        image_asset.path.clone(),
    )
    .unwrap();
    let image_node_id = image_node.id;
    image_to_sounds.assets.push(image_asset);
    image_to_sounds.add_node(image_node);
    image_to_sounds
        .attach_node_to_container(NodeContainer::Track(track_id), image_node_id)
        .unwrap();
    image_to_sounds
        .connections
        .iter_mut()
        .find(|connection| connection.id == edge_id)
        .unwrap()
        .from = PortAddress::new(PortOwner::Node(image_node_id), IMAGE_OUTPUT_PORT);
    assert_closed(&image_to_sounds);
    assert!(image_to_sounds.validate_connections().iter().any(|error| {
        matches!(
            error,
            ProjectGraphError::IncompatiblePortTypes {
                source_type: PortDataType::Image,
                target_type: PortDataType::Audio,
            }
        )
    }));

    let mut wrong_source_port = project.clone();
    let wrong_address = PortAddress::new(PortOwner::Node(media_id), "not_an_audio_output");
    wrong_source_port
        .connections
        .iter_mut()
        .find(|connection| connection.id == edge_id)
        .unwrap()
        .from = wrong_address.clone();
    assert_closed(&wrong_source_port);
    assert!(
        wrong_source_port
            .validate_connections()
            .contains(&ProjectGraphError::PortNotFound(wrong_address))
    );

    let mut duplicate = project.clone();
    let mut duplicate_edge = duplicate
        .connections
        .iter()
        .find(|connection| connection.id == edge_id)
        .unwrap()
        .clone();
    duplicate_edge.id = uuid::Uuid::new_v4();
    duplicate_edge.order = 1;
    duplicate.connections.push(duplicate_edge);
    assert_closed(&duplicate);
    assert!(duplicate.validate_connections().contains(
        &ProjectGraphError::DuplicateVariadicConnection {
            target: target.clone(),
            from: PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
        }
    ));

    let mut order_gap = project;
    order_gap
        .connections
        .iter_mut()
        .find(|connection| connection.id == edge_id)
        .unwrap()
        .order = 3;
    assert_closed(&order_gap);
    assert!(order_gap.validate_connections().contains(
        &ProjectGraphError::NonCanonicalConnectionOrder {
            target,
            expected_order: 0,
            actual_order: 3,
        }
    ));
}
