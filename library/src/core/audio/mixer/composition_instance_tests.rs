use super::tests::{TestAudioFiles, add_audio_node, attach_audio_output};
use super::*;
use crate::model::project::{
    FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, NUMBER_RESULT_OUTPUT_PORT, PortAddress, PortOwner,
    TIME_PORT,
};
use crate::model::property::{Property, PropertyValue};
use crate::model::{Clip, CompositionInstanceContent, Node, NodeContainer};
use ordered_float::OrderedFloat;

fn attach_composition_instance(
    project: &mut Project,
    track_id: uuid::Uuid,
    composition_id: uuid::Uuid,
    start_time: f64,
    duration: f64,
    trim_in: f64,
) -> (uuid::Uuid, uuid::Uuid) {
    let mut clip = Clip::new("composition instance", start_time, duration);
    clip.trim_in = OrderedFloat(trim_in);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    let node = Node::new_composition_instance(
        "composition instance",
        CompositionInstanceContent { composition_id },
    );
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();
    project
        .set_audio_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .unwrap();
    (clip_id, node_id)
}

fn attach_malformed_composition_instance(
    project: &mut Project,
    track_id: uuid::Uuid,
    composition_id: uuid::Uuid,
) {
    let clip = Clip::new("malformed composition instance", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    let node = Node::new_composition_instance(
        "malformed composition instance",
        CompositionInstanceContent { composition_id },
    );
    let node_id = node.id;
    project.add_node(node);
    // Model mutation APIs reject this state. Direct assignment models a
    // malformed pre-v1 payload so request-path failure can be tested.
    let clip = project.get_clip_mut(clip_id).unwrap();
    clip.node_ids.push(node_id);
    clip.audio_output_node_id = Some(node_id);
}

fn add_target_audio(
    project: &mut Project,
    cache: &CacheManager,
    files: &mut TestAudioFiles,
    track_id: uuid::Uuid,
    samples: Vec<f32>,
) -> uuid::Uuid {
    let media_id = add_audio_node(project, cache, files, samples);
    let clip = Clip::new("source audio", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    attach_audio_output(project, NodeContainer::Clip(clip_id), media_id);
    media_id
}

#[test]
fn instances_mix_the_same_definition_with_independent_clip_times() {
    let mut project = Project::new("independent composition instance audio");
    let (target, target_track) = Composition::new("source", 16, 16, 4.0, 2.0);
    let target_id = target.id;
    let target_track_id = target_track.id;
    project.add_track(target_track);
    project.add_composition(target);
    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let media_id = add_target_audio(
        &mut project,
        &cache,
        &mut files,
        target_track_id,
        (0..8).map(|sample| sample as f32).collect(),
    );

    let (parent, parent_track) = Composition::new("parent", 16, 16, 4.0, 1.0);
    let parent_id = parent.id;
    let parent_track_id = parent_track.id;
    project.add_track(parent_track);
    project.add_composition(parent);
    let (first_instance_clip_id, _) =
        attach_composition_instance(&mut project, parent_track_id, target_id, 0.0, 1.0, 0.0);
    attach_composition_instance(&mut project, parent_track_id, target_id, 0.0, 1.0, 0.5);

    assert_eq!(
        routed_audio_media_nodes(&project, PortOwner::Clip(first_instance_clip_id)),
        vec![media_id],
        "waveform discovery must recurse through the instance Audio route"
    );

    let target_before = project.get_composition(target_id).unwrap().clone();
    assert_eq!(
        mix_samples(
            &project.assets,
            &project,
            project.get_composition(parent_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &PluginManager::default(),
        ),
        vec![2.0, 4.0, 6.0, 8.0]
    );
    assert_eq!(project.get_composition(target_id), Some(&target_before));
}

#[test]
fn instance_honors_explicit_fmod_time_with_expression_divisor() {
    let mut project = Project::new("composition instance audio time override");
    let (target, target_track) = Composition::new("source", 16, 16, 4.0, 2.0);
    let target_id = target.id;
    let target_track_id = target_track.id;
    project.add_track(target_track);
    project.add_composition(target);
    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    add_target_audio(
        &mut project,
        &cache,
        &mut files,
        target_track_id,
        (0..8).map(|sample| sample as f32).collect(),
    );

    let (parent, parent_track) = Composition::new("parent", 16, 16, 4.0, 1.0);
    let parent_id = parent.id;
    let parent_track_id = parent_track.id;
    project.add_track(parent_track);
    project.add_composition(parent);
    let (instance_clip_id, instance_id) =
        attach_composition_instance(&mut project, parent_track_id, target_id, 0.0, 1.0, 0.0);
    let mut fmod = Node::new_fmod("half-second loop");
    fmod.set_property(
        FMOD_DIVISOR_INPUT_PORT.to_string(),
        Property::expression(
            "value".to_string(),
            PropertyValue::Number(OrderedFloat(0.5)),
        ),
    )
    .unwrap();
    let fmod_id = fmod.id;
    project.add_node(fmod);
    project
        .attach_node_to_container(NodeContainer::Clip(instance_clip_id), fmod_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(instance_clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(fmod_id), FMOD_X_INPUT_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(fmod_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(instance_id), TIME_PORT),
        )
        .unwrap();

    let composition = project.get_composition(parent_id).unwrap();
    assert_eq!(
        mix_samples(
            &project.assets,
            &project,
            composition,
            &cache,
            0,
            4,
            4,
            1,
            &PluginManager::default(),
        ),
        vec![0.0, 1.0, 0.0, 1.0]
    );
    let windows = audio_window_requests_for_composition(
        &project,
        composition,
        0,
        4,
        4,
        &PluginManager::default(),
    );
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].first_source_frame, 0);
    assert_eq!(windows[0].last_source_frame, 2);
}

#[test]
fn unreachable_invalid_instances_do_not_mute_valid_audio_requests() {
    let mut project = Project::new("request local composition instance validation");
    let (root, root_track) = Composition::new("root", 16, 16, 4.0, 1.0);
    let root_id = root.id;
    let root_track_id = root_track.id;
    project.add_track(root_track);
    project.add_composition(root);
    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let media_id = add_audio_node(&mut project, &cache, &mut files, vec![1.0; 4]);
    let valid_clip = Clip::new("valid", 0.0, 1.0);
    let valid_clip_id = valid_clip.id;
    project.add_clip(valid_clip);
    project
        .attach_clip_to_track(root_track_id, valid_clip_id)
        .unwrap();
    attach_audio_output(&mut project, NodeContainer::Clip(valid_clip_id), media_id);

    let (unreachable, unreachable_track) = Composition::new("unreachable", 16, 16, 4.0, 1.0);
    let unreachable_track_id = unreachable_track.id;
    project.add_track(unreachable_track);
    project.add_composition(unreachable);
    attach_malformed_composition_instance(&mut project, unreachable_track_id, uuid::Uuid::new_v4());
    assert!(!project.validate_connections().is_empty());

    assert_eq!(
        mix_samples(
            &project.assets,
            &project,
            project.get_composition(root_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &PluginManager::default(),
        ),
        vec![1.0; 4]
    );
}

#[test]
fn reachable_self_and_transitive_instance_cycles_fail_closed() {
    let mut project = Project::new("recursive composition instance audio");
    let (root, root_track) = Composition::new("root", 16, 16, 4.0, 1.0);
    let root_id = root.id;
    let root_track_id = root_track.id;
    project.add_track(root_track);
    project.add_composition(root);
    attach_malformed_composition_instance(&mut project, root_track_id, root_id);

    let (first, first_track) = Composition::new("first", 16, 16, 4.0, 1.0);
    let first_id = first.id;
    let first_track_id = first_track.id;
    project.add_track(first_track);
    project.add_composition(first);
    let (second, second_track) = Composition::new("second", 16, 16, 4.0, 1.0);
    let second_id = second.id;
    let second_track_id = second_track.id;
    project.add_track(second_track);
    project.add_composition(second);
    attach_composition_instance(&mut project, first_track_id, second_id, 0.0, 1.0, 0.0);
    attach_malformed_composition_instance(&mut project, second_track_id, first_id);
    attach_malformed_composition_instance(&mut project, root_track_id, first_id);

    let cache = CacheManager::new();
    let plugins = PluginManager::default();
    let composition = project.get_composition(root_id).unwrap();
    assert_eq!(
        mix_samples(
            &project.assets,
            &project,
            composition,
            &cache,
            0,
            4,
            4,
            1,
            &plugins,
        ),
        vec![0.0; 4]
    );
    assert!(
        audio_window_requests_for_composition(&project, composition, 0, 4, 4, &plugins).is_empty()
    );
    assert!(
        routed_audio_media_nodes(&project, PortOwner::Composition(root_id)).is_empty(),
        "waveform discovery must fail closed on recursive instance topology"
    );
}
