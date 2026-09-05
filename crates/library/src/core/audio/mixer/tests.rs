use super::property_evaluation::{AudioPropertyContext, volume_at};
use super::*;
use crate::model::project::{
    AUDIO_OUTPUT_PORT, MERGE_SOUNDS_PORT, PortAddress, PortDataType, PortDefinition, PortExposure,
    PortOwner, PortSide,
};
use crate::model::property::{Property, PropertyMap, PropertyValue};
use crate::model::{Clip, MediaContent, NodeContainer, Track};
use ordered_float::OrderedFloat;
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct TestAudioFiles(Vec<PathBuf>);

impl TestAudioFiles {
    fn create(&mut self) -> String {
        let path =
            std::env::temp_dir().join(format!("ruvie-audio-mixer-{}.source", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"identity-only audio test source").unwrap();
        self.0.push(path.clone());
        path.to_string_lossy().into_owned()
    }
}

impl Drop for TestAudioFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            if let Err(error) = std::fs::remove_file(path) {
                log::warn!("failed to remove audio mixer test source: {error}");
            }
        }
    }
}

pub(super) fn add_audio_node(
    project: &mut Project,
    cache_manager: &CacheManager,
    files: &mut TestAudioFiles,
    samples: Vec<f32>,
) -> uuid::Uuid {
    add_media_node(
        project,
        cache_manager,
        files,
        samples,
        AssetKind::Audio,
        None,
        true,
    )
    .0
}

fn add_media_node(
    project: &mut Project,
    cache_manager: &CacheManager,
    files: &mut TestAudioFiles,
    samples: Vec<f32>,
    kind: AssetKind,
    audio_stream_index: Option<usize>,
    enabled: bool,
) -> (uuid::Uuid, String) {
    let is_video = matches!(&kind, AssetKind::Video);
    let mut asset = Asset::new("audio", &files.create(), kind);
    if is_video {
        // Preserve a distinct visual stream while the Media Node selects
        // its embedded audio stream explicitly.
        asset.stream_index = Some(0);
    }
    let format = AudioDecodeFormat::new(4, 1).unwrap();
    let source = AudioSourceKey::read(&asset.path, audio_stream_index, format).unwrap();
    let samples_per_chunk = format.chunk_frames() as usize * usize::from(format.channels);
    for (chunk_index, chunk_samples) in samples.chunks(samples_per_chunk).enumerate() {
        cache_manager.put_audio_chunk(
            AudioChunk::new(
                AudioChunkKey {
                    source: source.clone(),
                    chunk_index: chunk_index as u64,
                },
                chunk_samples.to_vec(),
            )
            .unwrap(),
        );
    }
    let mut node = Node::from_media_converter(
        "audio",
        MediaContent {
            asset_id: asset.id,
            output_selection: if is_video {
                crate::model::MediaOutputSelection::ImageAndAudio
            } else {
                crate::model::MediaOutputSelection::Audio
            },
            stream_index: is_video.then_some(0),
            audio_stream_index,
        },
        &[],
        asset.path.clone(),
    )
    .expect("empty audio converter definition is valid");
    node.enabled = enabled;
    let node_id = node.id;
    let path = asset.path.clone();
    project.assets.push(asset);
    project.add_node(node);
    (node_id, path)
}

fn set_volume(properties: &mut PropertyMap, volume: f64) {
    properties.set(
        "volume".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(volume))),
    );
}

pub(super) fn attach_audio_output(
    project: &mut Project,
    container: NodeContainer,
    node_id: uuid::Uuid,
) -> Option<uuid::Uuid> {
    project
        .attach_node_to_container(container, node_id)
        .unwrap();
    connect_attached_audio_output(project, container, node_id)
}

pub(super) fn connect_attached_audio_output(
    project: &mut Project,
    container: NodeContainer,
    node_id: uuid::Uuid,
) -> Option<uuid::Uuid> {
    let merge_id = match container {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.structural_sound_merge_node_id),
        NodeContainer::Track(id) => project
            .get_track(id)
            .map(|track| track.structural_sound_merge_node_id),
        NodeContainer::Clip(_) => {
            project
                .set_audio_output_node(container, Some(node_id))
                .unwrap();
            return None;
        }
    }?;
    Some(
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(node_id), AUDIO_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_SOUNDS_PORT),
            )
            .unwrap(),
    )
}

#[test]
fn audio_expression_volume_uses_authored_fallback_without_per_sample_cpython() {
    let plugin_manager = PluginManager::default();
    let evaluators = plugin_manager.get_property_evaluators();
    let context = AudioPropertyContext::new(evaluators.as_ref(), 24.0, (100, 50));
    let mut properties = PropertyMap::new();
    properties.set(
        "volume".to_string(),
        Property::expression(
            "value + time + fps + width / 100".to_string(),
            PropertyValue::Number(OrderedFloat(0.5)),
        ),
    );
    assert_eq!(volume_at(&properties, 2.0, &context, "node:test"), 0.5);
    assert_eq!(volume_at(&properties, 2.1, &context, "node:test"), 0.5);
    assert_eq!(context.reported_diagnostic_count(), 1);

    properties.set(
        "volume".to_string(),
        Property::expression(
            "1 / 0".to_string(),
            PropertyValue::Number(OrderedFloat(0.25)),
        ),
    );
    assert_eq!(volume_at(&properties, 2.0, &context, "node:test"), 0.25);
    assert_eq!(context.reported_diagnostic_count(), 2);

    properties.set(
        "volume".to_string(),
        Property::keyframe(vec![
            crate::model::property::Keyframe::new(
                0.0,
                PropertyValue::Number(OrderedFloat(0.75)),
                crate::animation::EasingFunction::Expression {
                    text: "t * t".to_string(),
                },
            ),
            crate::model::property::Keyframe::new(
                1.0,
                PropertyValue::Number(OrderedFloat(1.0)),
                crate::animation::EasingFunction::Linear,
            ),
        ]),
    );
    assert_eq!(volume_at(&properties, 0.5, &context, "node:test"), 0.75);
    assert_eq!(context.reported_diagnostic_count(), 3);
}

#[test]
fn malformed_or_unregistered_audio_properties_are_silent() {
    let plugin_manager = PluginManager::default();
    let evaluators = plugin_manager.get_property_evaluators();
    let context = AudioPropertyContext::new(evaluators.as_ref(), 24.0, (100, 50));
    for property in [
        Property {
            evaluator: "expression".to_string(),
            properties: std::collections::HashMap::from([(
                "expression".to_string(),
                PropertyValue::String("1".to_string()),
            )]),
        },
        Property {
            evaluator: "not-installed".to_string(),
            properties: std::collections::HashMap::from([(
                "value".to_string(),
                PropertyValue::Number(OrderedFloat(1.0)),
            )]),
        },
    ] {
        let mut properties = PropertyMap::new();
        properties.set("volume".to_string(), property);
        assert_eq!(volume_at(&properties, 0.0, &context, "node:test"), 0.0);
    }
}

#[derive(Clone, Copy, Debug)]
enum TestNodeScope {
    Composition,
    Track,
    Clip,
}

fn assert_disabled_media_contract(scope: TestNodeScope) {
    let plugin_manager = PluginManager::default();
    let mut project = Project::new("disabled audio contract");
    let (composition, mut track) = Composition::new("main", 16, 16, 4.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    if !matches!(scope, TestNodeScope::Composition) {
        set_volume(&mut track.properties, 0.5);
    }
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let container = match scope {
        TestNodeScope::Composition => NodeContainer::Composition(composition_id),
        TestNodeScope::Track => NodeContainer::Track(track_id),
        TestNodeScope::Clip => {
            // At 4 Hz, only output frames 1 and 2 are inside [0.25, 0.75).
            let mut clip = Clip::new("half-open", 0.25, 0.5);
            set_volume(&mut clip.properties, 0.5);
            let clip_id = clip.id;
            project.add_clip(clip);
            project.attach_clip_to_track(track_id, clip_id).unwrap();
            NodeContainer::Clip(clip_id)
        }
    };

    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let mut enabled_sources = HashSet::new();
    let mut disabled_sources = HashSet::new();
    let mut disabled_node_id = None;
    let mut enabled_connection_id = None;
    for (sample, kind, stream_index, enabled) in [
        (1.0, AssetKind::Audio, None, true),
        (10.0, AssetKind::Audio, None, false),
    ] {
        let (node_id, path) = add_media_node(
            &mut project,
            &cache,
            &mut files,
            vec![sample; 8],
            kind,
            stream_index,
            enabled,
        );
        if enabled {
            enabled_connection_id = attach_audio_output(&mut project, container, node_id);
        } else {
            project
                .attach_node_to_container(container, node_id)
                .unwrap();
            disabled_node_id = Some(node_id);
        }
        let source = (path, stream_index);
        if enabled {
            enabled_sources.insert(source);
        } else {
            disabled_sources.insert(source);
        }
    }

    let composition = project.get_composition(composition_id).unwrap();
    let expected = match scope {
        TestNodeScope::Composition => vec![1.0; 4],
        // Track volume is applied once to direct Track audio.
        TestNodeScope::Track => vec![0.5; 4],
        // Clip and Track gains are both 0.5, and the Clip end is excluded.
        TestNodeScope::Clip => vec![0.0, 0.25, 0.25, 0.0],
    };
    assert_eq!(
        mix_samples(
            &project.assets,
            &project,
            project.get_composition(composition_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &plugin_manager,
        ),
        expected
    );
    assert_eq!(
        render_samples(
            &project.assets,
            &project,
            composition,
            &cache,
            0,
            4,
            4,
            1,
            &plugin_manager,
        ),
        expected
    );

    let requests =
        audio_window_requests_for_composition(&project, composition, 0, 4, 4, &plugin_manager);
    let requested_sources = requests
        .iter()
        .map(|request| (request.source.path.clone(), request.source.stream_index))
        .collect::<HashSet<_>>();
    assert_eq!(requested_sources, enabled_sources);
    assert!(requested_sources.is_disjoint(&disabled_sources));
    assert_eq!(requests.len(), 1);
    let expected_bounds = match scope {
        TestNodeScope::Composition | TestNodeScope::Track => (0, 4),
        TestNodeScope::Clip => (0, 2),
    };
    assert!(requests.iter().all(|request| {
        (request.first_source_frame, request.last_source_frame) == expected_bounds
    }));

    let disabled_node_id = disabled_node_id.unwrap();
    if let Some(connection_id) = enabled_connection_id {
        let target = project
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .unwrap()
            .to
            .clone();
        project
            .reconnect_connection(
                connection_id,
                PortAddress::new(PortOwner::Node(disabled_node_id), AUDIO_OUTPUT_PORT),
                target,
            )
            .unwrap();
    } else {
        project
            .set_audio_output_node(container, Some(disabled_node_id))
            .unwrap();
    }
    let composition = project.get_composition(composition_id).unwrap();
    assert_eq!(
        mix_samples(
            &project.assets,
            &project,
            project.get_composition(composition_id).unwrap(),
            &cache,
            0,
            4,
            4,
            1,
            &plugin_manager,
        ),
        vec![0.0; 4],
        "a disabled Node bound to Audio must be NoOutput"
    );
    assert!(
        audio_window_requests_for_composition(&project, composition, 0, 4, 4, &plugin_manager,)
            .is_empty()
    );
}

#[test]
fn disabled_composition_media_is_no_output_for_mix_render_and_prefetch() {
    assert_disabled_media_contract(TestNodeScope::Composition);
}

#[test]
fn disabled_track_media_is_no_output_for_mix_render_and_prefetch() {
    assert_disabled_media_contract(TestNodeScope::Track);
}

#[test]
fn disabled_clip_media_is_no_output_for_mix_render_and_prefetch() {
    assert_disabled_media_contract(TestNodeScope::Clip);
}

#[test]
fn mixes_every_top_level_track_and_all_audio_nodes() {
    let mut project = Project::new("audio test");
    let (composition, first_track) = Composition::new("main", 1920, 1080, 30.0, 1.0);
    let composition_id = composition.id;
    let first_track_id = first_track.id;
    assert!(
        project.add_track(first_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let second_track = Track::new("second");
    let second_track_id = second_track.id;
    assert!(
        project.add_track(second_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project
        .attach_track_to_composition(composition_id, second_track_id)
        .unwrap();

    let cache_manager = CacheManager::new();
    let mut files = TestAudioFiles::default();
    for (track_id, sample) in [(first_track_id, 0.25), (second_track_id, 0.5)] {
        let clip = Clip::new("audio clip", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let node_id = add_audio_node(&mut project, &cache_manager, &mut files, vec![sample; 4]);
        attach_audio_output(&mut project, NodeContainer::Clip(clip_id), node_id);
    }

    let composition = project.get_composition(composition_id).unwrap();
    let mixed = mix_samples(
        &project.assets,
        &project,
        composition,
        &cache_manager,
        0,
        4,
        4,
        1,
        &PluginManager::default(),
    );

    assert_eq!(mixed, vec![0.75; 4]);
}

#[test]
fn malformed_duplicate_container_child_fails_closed_to_silence() {
    let mut project = Project::new("duplicate audio reachability");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    // A directly loaded malformed Project can repeat a child identity. The
    // authoring API rejects it, so reproduce the persisted corruption only
    // after materializing the required structural Merge state.
    let mut persisted = serde_json::to_value(project).unwrap();
    persisted["compositions"][0]["track_ids"] = serde_json::json!([track_id, track_id]);
    let mut project: Project = serde_json::from_value(persisted).unwrap();
    let clip = Clip::new("audio", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let node_id = add_audio_node(&mut project, &cache, &mut files, vec![0.5; 4]);
    attach_audio_output(&mut project, NodeContainer::Clip(clip_id), node_id);
    let mixed = mix_samples(
        &project.assets,
        &project,
        project.get_composition(composition_id).unwrap(),
        &cache,
        0,
        4,
        4,
        1,
        &PluginManager::default(),
    );
    assert_eq!(mixed, vec![0.0; 4]);
}

#[test]
fn applies_clip_timing_trim_and_stretch_per_output_frame() {
    let mut project = Project::new("audio timing test");
    let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 2.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let mut clip = Clip::new("retimed", 0.5, 1.0);
    clip.trim_in = OrderedFloat(0.25);
    clip.time_stretch = OrderedFloat(2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let cache_manager = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let node_id = add_audio_node(
        &mut project,
        &cache_manager,
        &mut files,
        (0..12).map(|sample| sample as f32).collect(),
    );
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();
    project
        .set_audio_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .unwrap();

    let composition = project.get_composition(composition_id).unwrap();
    let mixed = mix_samples(
        &project.assets,
        &project,
        composition,
        &cache_manager,
        0,
        6,
        4,
        1,
        &PluginManager::default(),
    );

    // t=0.5 maps to source 0.25 (sample 1), then advances two source
    // samples for each output sample because time_stretch is 2.
    assert_eq!(mixed, vec![0.0, 0.0, 1.0, 3.0, 5.0, 7.0]);
}

#[test]
fn unsupported_audio_plugin_operation_is_no_output_unless_explicitly_bypassed() {
    let mut project = Project::new("unsupported audio operation");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let clip = Clip::new("audio clip", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let media_id = add_audio_node(&mut project, &cache, &mut files, vec![1.0; 8]);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), media_id)
        .unwrap();

    let mut persisted = serde_json::to_value(Node::new_merge("unsupported audio effect")).unwrap();
    persisted["content"] = serde_json::json!({
        "type": "PluginOperation",
        "data": {
            "category": "audio_effect",
            "component_id": "not-installed",
            "operation": "audio.effect.v1",
            "declared_ports": [
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
    let operation: Node = serde_json::from_value(persisted).unwrap();
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

    assert!(
        routed_audio_media_nodes(&project, PortOwner::Clip(clip_id)).is_empty(),
        "waveform discovery must not present unsupported operations as pass-through audio"
    );

    let composition = project.get_composition(composition_id).unwrap();
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
        vec![0.0; 4]
    );
    assert!(
        audio_window_requests_for_composition(
            &project,
            composition,
            0,
            4,
            4,
            &PluginManager::default(),
        )
        .is_empty()
    );

    assert!(project.get_node(operation_id).unwrap().supports_bypass());
    project.get_node_mut(operation_id).unwrap().bypassed = true;
    assert_eq!(
        routed_audio_media_nodes(&project, PortOwner::Clip(clip_id)),
        vec![media_id]
    );
    assert_eq!(
        mix_samples(
            &project.assets,
            &project,
            project.get_composition(composition_id).unwrap(),
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
fn source_window_uses_clip_local_time_and_explicit_audio_stream() {
    let mut project = Project::new("late clip audio window");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 200.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let mut clip = Clip::new("late retimed clip", 100.0, 2.0);
    clip.trim_in = OrderedFloat(0.25);
    clip.time_stretch = OrderedFloat(2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let mut asset = Asset::new("multi stream", "/fixture/multi_audio.mkv", AssetKind::Video);
    asset.stream_index = Some(0);
    let asset_id = asset.id;
    project.assets.push(asset);
    let node = Node::from_media_converter(
        "audio stream two",
        MediaContent {
            asset_id,
            output_selection: crate::model::MediaOutputSelection::ImageAndAudio,
            stream_index: Some(0),
            audio_stream_index: Some(2),
        },
        &[],
        "/fixture/multi_audio.mkv".to_string(),
    )
    .expect("empty audio converter definition is valid");
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();
    project
        .set_audio_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .unwrap();

    // The global window starts before the Clip (99.75s). Its contributing
    // samples at 100.0..100.75 map to source frames 1,3,5,7; frame 8 is
    // additionally required for interpolation.
    let requests = audio_window_requests_for_composition(
        &project,
        project.get_composition(composition_id).unwrap(),
        399,
        5,
        4,
        &PluginManager::default(),
    );
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].source.stream_index, Some(2));
    assert_eq!(requests[0].first_source_frame, 1);
    assert_eq!(requests[0].last_source_frame, 8);
}

#[test]
fn visual_stream_is_never_reused_as_embedded_audio_selection() {
    let mut video = Asset::new("video", "/fixture/video.mkv", AssetKind::Video);
    video.stream_index = Some(0);
    let mut audio = Asset::new("audio", "/fixture/audio.mka", AssetKind::Audio);
    audio.stream_index = Some(2);
    let mut media = MediaContent {
        asset_id: video.id,
        output_selection: crate::model::MediaOutputSelection::ImageAndAudio,
        stream_index: Some(0),
        audio_stream_index: None,
    };

    assert_eq!(audio_stream_index_for_media(&video, &media), None);
    media.audio_stream_index = Some(1);
    assert_eq!(audio_stream_index_for_media(&video, &media), Some(1));
    media.asset_id = audio.id;
    media.audio_stream_index = None;
    assert_eq!(audio_stream_index_for_media(&audio, &media), Some(2));
}

#[test]
fn composition_range_is_half_open_for_direct_and_scheduled_audio() {
    let mut project = Project::new("composition audio range");
    let (composition, track) = Composition::new("main", 16, 16, 4.0, 1.0);
    let composition_id = composition.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let cache = CacheManager::new();
    let mut files = TestAudioFiles::default();
    let node_id = add_audio_node(&mut project, &cache, &mut files, vec![1.0; 8]);
    attach_audio_output(
        &mut project,
        NodeContainer::Composition(composition_id),
        node_id,
    );
    let composition = project.get_composition(composition_id).unwrap();

    let mixed = mix_samples(
        &project.assets,
        &project,
        composition,
        &cache,
        2,
        4,
        4,
        1,
        &PluginManager::default(),
    );
    assert_eq!(mixed, vec![1.0, 1.0, 0.0, 0.0]);
    assert!(
        audio_window_requests_for_composition(
            &project,
            composition,
            4,
            4,
            4,
            &PluginManager::default(),
        )
        .is_empty()
    );
}
