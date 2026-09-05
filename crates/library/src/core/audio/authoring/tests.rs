use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use ordered_float::OrderedFloat;

use super::*;
use crate::animation::EasingFunction;
use crate::model::authoring::{
    AutomationKeyframe, AutomationTrack, CompositionInstance, DurationPolicy, ModuleConnection,
    ModuleConnectionId, ModuleDefinition, ModuleDefinitionId, ModuleDefinitionSharing,
    ModuleInstance, ModuleInstanceId, ModulePortAddress, PublishedMediaInput,
    PublishedMediaInputId, PublishedParameter, PublishedParameterId, RationalRate,
    TRACK_VISIBILITY_PROPERTY, TimeMap, Timeline, TimelineInterval, TimelineTrack, TimelineTrackId,
    Transition, TransitionAlignment, TransitionId, TransitionMediaType, TransitionProcessor,
};
use crate::model::frame::color::Color;
use crate::model::node::{Node, ValueContent};
use crate::model::project::property::{Property, PropertyValue};
use crate::model::project::{
    MERGE_SOUNDS_PORT, NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT,
    PortDataType, TRANSITION_FROM_INPUT_PORT, TRANSITION_PROGRESS_INPUT_PORT,
    TRANSITION_PROGRESS_PROPERTY,
};

fn frame_time(frame: i64) -> MediaTime {
    MediaTime::new(frame, AUTHORING_AUDIO_SAMPLE_RATE).unwrap()
}

fn project_with_audio_track(frame_count: i64) -> AuthoringProject {
    let mut project = AuthoringProject::new(
        "authoring audio",
        64,
        64,
        RationalRate::new(24, 1).unwrap(),
        frame_time(frame_count),
    )
    .unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    project.tracks.get_mut(&track_id).unwrap().kind = TimelineTrackKind::Audio;
    project
}

fn write_stereo_wave(path: &Path, frames: &[[f32; 2]]) {
    let channels = 2_u16;
    let bits_per_sample = 16_u16;
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = AUTHORING_AUDIO_SAMPLE_RATE * u32::from(block_align);
    let data_len = u32::try_from(frames.len() * usize::from(block_align)).unwrap();
    let mut file = File::create(path).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36_u32 + data_len).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&channels.to_le_bytes()).unwrap();
    file.write_all(&AUTHORING_AUDIO_SAMPLE_RATE.to_le_bytes())
        .unwrap();
    file.write_all(&byte_rate.to_le_bytes()).unwrap();
    file.write_all(&block_align.to_le_bytes()).unwrap();
    file.write_all(&bits_per_sample.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&data_len.to_le_bytes()).unwrap();
    for frame in frames {
        for sample in frame {
            let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
            file.write_all(&pcm.to_le_bytes()).unwrap();
        }
    }
    file.flush().unwrap();
}

fn add_audio_asset(project: &mut AuthoringProject, path: &Path, frame_count: usize) -> Uuid {
    let mut asset = Asset::new("fixture.wav", path.to_str().unwrap(), AssetKind::Audio);
    asset.duration = Some(frame_count as f64 / f64::from(AUTHORING_AUDIO_SAMPLE_RATE));
    let id = asset.id;
    project.assets.push(asset);
    id
}

fn add_asset_item(
    project: &mut AuthoringProject,
    track_id: TimelineTrackId,
    asset_id: Uuid,
    start: i64,
    duration: i64,
    source_start: i64,
) -> TimelineItemId {
    let id = TimelineItemId::new();
    project.items.insert(
        id,
        TimelineItem {
            id,
            track_id,
            name: "Audio".into(),
            source: SourceRef::Asset { asset_id },
            interval: TimelineInterval::new(frame_time(start), frame_time(duration)).unwrap(),
            time_map: TimeMap {
                source_start: frame_time(source_start),
                playback_rate: RationalRate::one(),
            },
            layer: 0,
            parent: None,
            blend_mode: crate::model::BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    id
}

fn set_gain(properties: &mut PropertyMap, gain: f64) {
    properties.set(
        "gain".into(),
        Property::constant(PropertyValue::Number(OrderedFloat(gain))),
    );
}

fn frame(buffer: &[f32], index: usize) -> [f32; 2] {
    [buffer[index * 2], buffer[index * 2 + 1]]
}

fn assert_stereo_near(actual: [f32; 2], expected: [f32; 2]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 0.0002,
            "actual {actual}, expected {expected}"
        );
    }
}

fn add_audio_crossfade(
    project: &mut AuthoringProject,
    from: TimelineItemId,
    to: TimelineItemId,
    edit_frame: i64,
    duration_frames: i64,
) -> TransitionId {
    let id = TransitionId::new();
    project.transitions.insert(
        id,
        Transition {
            id,
            timeline_id: project.root_timeline_id,
            from_item_id: from,
            to_item_id: to,
            edit_point: frame_time(edit_frame),
            duration: frame_time(duration_frames),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::audio_crossfade(),
            parameters: HashMap::new(),
        },
    );
    id
}

fn promote_audio_crossfade_to_module(
    project: &mut AuthoringProject,
    transition_id: TransitionId,
) -> ModuleDefinitionId {
    let (mut definition, contract) = ModuleDefinition::new_transition(
        "Editable Audio Crossfade",
        ModuleDefinitionSharing::Private,
        TransitionMediaType::Audio,
    )
    .unwrap();
    let progress_node_id = definition
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == contract.progress_parameter_id)
        .unwrap()
        .target
        .node_id;
    definition
        .graph
        .nodes
        .get_mut(&progress_node_id)
        .unwrap()
        .set_property(
            TRANSITION_PROGRESS_PROPERTY.to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
        )
        .unwrap();
    let definition_id = definition.id;
    let instance_id = ModuleInstanceId::new();
    project.module_definitions.insert(definition_id, definition);
    project.module_instances.insert(
        instance_id,
        ModuleInstance {
            id: instance_id,
            definition_id,
            parameter_overrides: HashMap::new(),
        },
    );
    project
        .transitions
        .get_mut(&transition_id)
        .unwrap()
        .processor = TransitionProcessor::module(instance_id, TransitionMediaType::Audio);
    project.validate().unwrap();
    definition_id
}

fn square_audio_transition_progress(
    project: &mut AuthoringProject,
    definition_id: ModuleDefinitionId,
) {
    let definition = project.module_definitions.get_mut(&definition_id).unwrap();
    let progress_connection = definition
        .graph
        .connections
        .iter()
        .position(|connection| connection.to.port == TRANSITION_PROGRESS_INPUT_PORT)
        .unwrap();
    let progress_connection = definition.graph.connections.remove(progress_connection);
    let progress_source = progress_connection.from;
    let mix_target = progress_connection.to;
    let square = Node::new_value("Square Progress", ValueContent::Multiply);
    let square_id = square.id;
    definition.graph.nodes.insert(square_id, square);
    let address = |node_id, port: &str| ModulePortAddress {
        node_id,
        port: port.to_string(),
    };
    definition.graph.connections.extend([
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: progress_source.clone(),
            to: address(square_id, NUMERIC_A_INPUT_PORT),
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        },
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: progress_source,
            to: address(square_id, NUMERIC_B_INPUT_PORT),
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        },
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: address(square_id, NUMBER_RESULT_OUTPUT_PORT),
            to: mix_target,
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        },
    ]);
    definition.topology_revision += 1;
    definition.validate().unwrap();
    project.validate().unwrap();
}

fn publish_audio_mix_progress(
    project: &mut AuthoringProject,
    definition_id: ModuleDefinitionId,
) -> PublishedParameterId {
    let definition = project.module_definitions.get_mut(&definition_id).unwrap();
    let progress_connection = definition
        .graph
        .connections
        .iter()
        .position(|connection| connection.to.port == TRANSITION_PROGRESS_INPUT_PORT)
        .map(|index| definition.graph.connections.remove(index))
        .expect("starter Progress connection");
    let parameter_id = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id: parameter_id,
        name: "Mix".to_string(),
        data_type: PortDataType::Number,
        default_value: PropertyValue::Number(OrderedFloat(0.0)),
        target: progress_connection.to,
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
    definition.validate().unwrap();
    parameter_id
}

fn insert_invalid_extra_audio_input(
    project: &mut AuthoringProject,
    definition_id: ModuleDefinitionId,
) {
    let definition = project.module_definitions.get_mut(&definition_id).unwrap();
    let merge = Node::new_sound_merge("Sidechain");
    let merge_id = merge.id;
    definition.graph.nodes.insert(merge_id, merge);
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: PublishedMediaInputId::new(),
        name: "Sidechain".to_string(),
        data_type: PortDataType::Audio,
        target: ModulePortAddress {
            node_id: merge_id,
            port: MERGE_SOUNDS_PORT.to_string(),
        },
        required: true,
        primary: false,
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
}

#[test]
fn adjacent_audio_crossfade_has_deterministic_linear_amplitude_and_half_open_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("from.wav");
    let to_path = directory.path().join("to.wav");
    let from_samples = vec![[0.25; 2]; 12];
    let to_samples = vec![[0.75; 2]; 12];
    write_stereo_wave(&from_path, &from_samples);
    write_stereo_wave(&to_path, &to_samples);

    let mut project = project_with_audio_track(12);
    let from_asset = add_audio_asset(&mut project, &from_path, from_samples.len());
    let to_asset = add_audio_asset(&mut project, &to_path, to_samples.len());
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let from = add_asset_item(&mut project, track_id, from_asset, 0, 5, 0);
    let to = add_asset_item(&mut project, track_id, to_asset, 5, 5, 2);
    add_audio_crossfade(&mut project, from, to, 5, 4);
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(4);
    let rendered = AuthoringAudioMixer::root(&project, &cache)
        .unwrap()
        .render_window(0, 9)
        .unwrap();
    for (index, expected) in [0.25, 0.25, 0.25, 0.25, 0.375, 0.5, 0.625, 0.75, 0.75]
        .into_iter()
        .enumerate()
    {
        assert_stereo_near(frame(&rendered, index), [expected; 2]);
    }
}

#[test]
fn hiding_an_audio_visual_track_does_not_change_its_rendered_audio_samples() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audio-visual-visibility.wav");
    let samples = [[0.25, -0.5], [0.5, -0.25], [-0.75, 0.125], [0.375, 0.625]];
    write_stereo_wave(&path, &samples);

    let mut visible = project_with_audio_track(samples.len() as i64);
    let track_id = visible.timelines[&visible.root_timeline_id].track_order[0];
    visible.tracks.get_mut(&track_id).unwrap().kind = TimelineTrackKind::AudioVisual;
    let asset_id = add_audio_asset(&mut visible, &path, samples.len());
    add_asset_item(&mut visible, track_id, asset_id, 0, samples.len() as i64, 0);
    visible.validate().unwrap();
    let cache = CacheManager::with_audio_chunk_capacity(2);
    let visible_audio = AuthoringAudioMixer::root(&visible, &cache)
        .unwrap()
        .render_window(0, samples.len())
        .unwrap();
    assert!(visible_audio.iter().any(|sample| *sample != 0.0));

    let mut hidden = visible.clone();
    hidden
        .tracks
        .get_mut(&track_id)
        .unwrap()
        .authored_properties
        .set(
            TRACK_VISIBILITY_PROPERTY.to_string(),
            Property::constant(PropertyValue::Boolean(false)),
        );
    hidden.validate().unwrap();
    let hidden_audio = AuthoringAudioMixer::root(&hidden, &cache)
        .unwrap()
        .render_window(0, samples.len())
        .unwrap();
    assert_eq!(
        hidden_audio, visible_audio,
        "the Track Eye owns only image participation, never Audio mute"
    );
}

#[test]
fn default_audio_transition_module_uses_timeline_owned_progress() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("module-from.wav");
    let to_path = directory.path().join("module-to.wav");
    let from_samples = vec![[0.25; 2]; 12];
    let to_samples = vec![[0.75; 2]; 12];
    write_stereo_wave(&from_path, &from_samples);
    write_stereo_wave(&to_path, &to_samples);

    let mut project = project_with_audio_track(12);
    let from_asset = add_audio_asset(&mut project, &from_path, from_samples.len());
    let to_asset = add_audio_asset(&mut project, &to_path, to_samples.len());
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let from = add_asset_item(&mut project, track_id, from_asset, 0, 5, 0);
    let to = add_asset_item(&mut project, track_id, to_asset, 5, 5, 2);
    let transition_id = add_audio_crossfade(&mut project, from, to, 5, 4);
    promote_audio_crossfade_to_module(&mut project, transition_id);

    let cache = CacheManager::with_audio_chunk_capacity(4);
    let rendered = AuthoringAudioMixer::root(&project, &cache)
        .unwrap()
        .render_window(3, 5)
        .unwrap();
    for (index, expected) in [0.25, 0.375, 0.5, 0.625, 0.75].into_iter().enumerate() {
        assert_stereo_near(frame(&rendered, index), [expected; 2]);
    }
}

#[test]
fn audio_transition_module_evaluates_a_custom_progress_graph() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("curved-from.wav");
    let to_path = directory.path().join("curved-to.wav");
    write_stereo_wave(&from_path, &[[0.25; 2]; 12]);
    write_stereo_wave(&to_path, &[[0.75; 2]; 12]);

    let mut project = project_with_audio_track(12);
    let from_asset = add_audio_asset(&mut project, &from_path, 12);
    let to_asset = add_audio_asset(&mut project, &to_path, 12);
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let from = add_asset_item(&mut project, track_id, from_asset, 0, 5, 0);
    let to = add_asset_item(&mut project, track_id, to_asset, 5, 5, 2);
    let transition_id = add_audio_crossfade(&mut project, from, to, 5, 4);
    let definition_id = promote_audio_crossfade_to_module(&mut project, transition_id);
    square_audio_transition_progress(&mut project, definition_id);

    let cache = CacheManager::with_audio_chunk_capacity(4);
    let rendered = AuthoringAudioMixer::root(&project, &cache)
        .unwrap()
        .render_window(5, 1)
        .unwrap();
    // Timeline Progress is .5, then the authored Multiply Node makes it .25:
    // .25 * .75 + .75 * .25 = .375.
    assert_stereo_near(frame(&rendered, 0), [0.375; 2]);
}

#[test]
fn audio_transition_module_evaluates_published_parameter_automation() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("automated-from.wav");
    let to_path = directory.path().join("automated-to.wav");
    write_stereo_wave(&from_path, &[[0.25; 2]; 12]);
    write_stereo_wave(&to_path, &[[0.75; 2]; 12]);

    let mut project = project_with_audio_track(12);
    let from_asset = add_audio_asset(&mut project, &from_path, 12);
    let to_asset = add_audio_asset(&mut project, &to_path, 12);
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let from = add_asset_item(&mut project, track_id, from_asset, 0, 5, 0);
    let to = add_asset_item(&mut project, track_id, to_asset, 5, 5, 2);
    let transition_id = add_audio_crossfade(&mut project, from, to, 5, 4);
    let definition_id = promote_audio_crossfade_to_module(&mut project, transition_id);
    let parameter_id = publish_audio_mix_progress(&mut project, definition_id);
    project.validate().unwrap();
    let cache = CacheManager::with_audio_chunk_capacity(4);
    let authored_default = AuthoringAudioMixer::root(&project, &cache)
        .unwrap()
        .render_window(5, 1)
        .unwrap();
    assert_stereo_near(frame(&authored_default, 0), [0.25; 2]);

    project
        .transitions
        .get_mut(&transition_id)
        .unwrap()
        .processor
        .module_processor_mut()
        .unwrap()
        .automation_tracks
        .insert(
            parameter_id,
            AutomationTrack {
                keyframes: vec![AutomationKeyframe::new(
                    MediaTime::zero(),
                    PropertyValue::Number(OrderedFloat(1.0)),
                    EasingFunction::Linear,
                )],
            },
        );
    project.validate().unwrap();
    let automated = AuthoringAudioMixer::root(&project, &cache)
        .unwrap()
        .render_window(5, 1)
        .unwrap();
    assert_stereo_near(frame(&automated, 0), [0.75; 2]);
}

#[test]
fn audio_transition_extra_published_media_input_is_rejected_during_authoring() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("bound-from.wav");
    let to_path = directory.path().join("bound-to.wav");
    write_stereo_wave(&from_path, &[[0.25; 2]; 12]);
    write_stereo_wave(&to_path, &[[0.75; 2]; 12]);

    let mut project = project_with_audio_track(12);
    let from_asset = add_audio_asset(&mut project, &from_path, 12);
    let to_asset = add_audio_asset(&mut project, &to_path, 12);
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let from = add_asset_item(&mut project, track_id, from_asset, 0, 5, 0);
    let to = add_asset_item(&mut project, track_id, to_asset, 5, 5, 2);
    let transition_id = add_audio_crossfade(&mut project, from, to, 5, 4);
    let definition_id = promote_audio_crossfade_to_module(&mut project, transition_id);
    insert_invalid_extra_audio_input(&mut project, definition_id);

    let error = project
        .validate()
        .expect_err("invalid Audio controls must not reach mixer construction");
    assert!(
        error.contains("supplies only the host-owned A/B"),
        "{error}"
    );
}

#[test]
fn edited_audio_transition_topology_is_explicitly_unsupported() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("custom-from.wav");
    let to_path = directory.path().join("custom-to.wav");
    write_stereo_wave(&from_path, &[[0.25; 2]; 12]);
    write_stereo_wave(&to_path, &[[0.75; 2]; 12]);

    let mut project = project_with_audio_track(12);
    let from_asset = add_audio_asset(&mut project, &from_path, 12);
    let to_asset = add_audio_asset(&mut project, &to_path, 12);
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let from = add_asset_item(&mut project, track_id, from_asset, 0, 5, 0);
    let to = add_asset_item(&mut project, track_id, to_asset, 5, 5, 2);
    let transition_id = add_audio_crossfade(&mut project, from, to, 5, 4);
    let definition_id = promote_audio_crossfade_to_module(&mut project, transition_id);
    project
        .module_definitions
        .get_mut(&definition_id)
        .unwrap()
        .graph
        .connections
        .retain(|connection| connection.to.port != TRANSITION_FROM_INPUT_PORT);
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(4);
    let error = match AuthoringAudioMixer::root(&project, &cache) {
        Ok(_) => panic!("custom Audio Transition topology must not silently fall back"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        AuthoringAudioError::UnsupportedTransitionProcessor {
            transition_id: actual,
            ..
        } if actual == transition_id.as_uuid()
    ));
}

#[test]
fn intentional_audio_overlap_uses_the_same_crossfade_contract() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("overlap-from.wav");
    let to_path = directory.path().join("overlap-to.wav");
    let from_samples = vec![[0.25; 2]; 12];
    let to_samples = vec![[0.75; 2]; 12];
    write_stereo_wave(&from_path, &from_samples);
    write_stereo_wave(&to_path, &to_samples);

    let mut project = project_with_audio_track(12);
    let from_asset = add_audio_asset(&mut project, &from_path, from_samples.len());
    let to_asset = add_audio_asset(&mut project, &to_path, to_samples.len());
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    // Authored overlap exactly owns the Transition interval [3, 7), so the
    // ordinary schedule immediately outside it has one source and remains
    // continuous with progress 0/1.
    let from = add_asset_item(&mut project, track_id, from_asset, 0, 7, 0);
    let to = add_asset_item(&mut project, track_id, to_asset, 3, 7, 0);
    add_audio_crossfade(&mut project, from, to, 5, 4);
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(4);
    let rendered = AuthoringAudioMixer::root(&project, &cache)
        .unwrap()
        .render_window(2, 6)
        .unwrap();
    for (index, expected) in [0.25, 0.25, 0.375, 0.5, 0.625, 0.75]
        .into_iter()
        .enumerate()
    {
        assert_stereo_near(frame(&rendered, index), [expected; 2]);
    }
}

#[test]
fn missing_audio_head_handle_is_a_typed_runtime_diagnostic() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("missing-from.wav");
    let to_path = directory.path().join("missing-to.wav");
    let samples = vec![[0.25; 2]; 12];
    write_stereo_wave(&from_path, &samples);
    write_stereo_wave(&to_path, &samples);

    let mut project = project_with_audio_track(12);
    let from_asset = add_audio_asset(&mut project, &from_path, samples.len());
    let to_asset = add_audio_asset(&mut project, &to_path, samples.len());
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let from = add_asset_item(&mut project, track_id, from_asset, 0, 5, 0);
    let to = add_asset_item(&mut project, track_id, to_asset, 5, 5, 0);
    let transition_id = add_audio_crossfade(&mut project, from, to, 5, 4);
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(4);
    let error = AuthoringAudioMixer::root(&project, &cache)
        .unwrap()
        .render_window(4, 1)
        .unwrap_err();
    assert!(matches!(
        error,
        AuthoringAudioError::TransitionSourceHandleUnavailable(ref detail)
            if detail.transition_id == transition_id.as_uuid()
                && detail.item_id == to.as_uuid()
                && detail.source_time < 0.0
    ));
}

#[test]
fn decoder_eof_during_audio_crossfade_is_not_silently_treated_as_a_handle() {
    let directory = tempfile::tempdir().unwrap();
    let from_path = directory.path().join("short-from.wav");
    let to_path = directory.path().join("long-to.wav");
    let short_samples = vec![[0.25; 2]; 5];
    let long_samples = vec![[0.75; 2]; 12];
    write_stereo_wave(&from_path, &short_samples);
    write_stereo_wave(&to_path, &long_samples);

    let mut project = project_with_audio_track(12);
    let from_asset = add_audio_asset(&mut project, &from_path, short_samples.len());
    let to_asset = add_audio_asset(&mut project, &to_path, long_samples.len());
    project
        .assets
        .iter_mut()
        .find(|asset| asset.id == from_asset)
        .unwrap()
        .duration = None;
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let from = add_asset_item(&mut project, track_id, from_asset, 0, 5, 0);
    let to = add_asset_item(&mut project, track_id, to_asset, 5, 5, 2);
    let transition_id = add_audio_crossfade(&mut project, from, to, 5, 4);
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(4);
    let error = AuthoringAudioMixer::root(&project, &cache)
        .unwrap()
        .render_window(5, 1)
        .unwrap_err();
    assert!(matches!(
        error,
        AuthoringAudioError::TransitionSourceHandleUnavailable(ref detail)
            if detail.transition_id == transition_id.as_uuid()
                && detail.item_id == from.as_uuid()
                && (detail.source_time - 5.0 / f64::from(AUTHORING_AUDIO_SAMPLE_RATE)).abs()
                    < f64::EPSILON
    ));
}

#[test]
fn placement_start_and_source_trim_map_to_exact_samples() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("trim.wav");
    let samples = (0..12)
        .map(|index| {
            let left = 0.025 * (index + 1) as f32;
            [left, -left]
        })
        .collect::<Vec<_>>();
    write_stereo_wave(&path, &samples);

    let mut project = project_with_audio_track(10);
    let asset_id = add_audio_asset(&mut project, &path, samples.len());
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    add_asset_item(&mut project, track_id, asset_id, 2, 4, 3);
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(2);
    let mut mixer = AuthoringAudioMixer::root(&project, &cache).unwrap();
    let rendered = mixer.render_window(0, 8).unwrap();
    assert_stereo_near(frame(&rendered, 0), [0.0, 0.0]);
    assert_stereo_near(frame(&rendered, 1), [0.0, 0.0]);
    for output_frame in 2..6 {
        assert_stereo_near(frame(&rendered, output_frame), samples[output_frame + 1]);
    }
    assert_stereo_near(frame(&rendered, 6), [0.0, 0.0]);
    assert_stereo_near(frame(&rendered, 7), [0.0, 0.0]);
}

#[test]
fn nested_timeline_uses_composition_and_leaf_local_time() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested.wav");
    let samples = (0..12)
        .map(|index| [0.03 * (index + 1) as f32; 2])
        .collect::<Vec<_>>();
    write_stereo_wave(&path, &samples);

    let mut project = project_with_audio_track(12);
    let asset_id = add_audio_asset(&mut project, &path, samples.len());
    let nested_id = TimelineId::new();
    let nested_track_id = TimelineTrackId::new();
    project.timelines.insert(
        nested_id,
        Timeline {
            id: nested_id,
            name: "Nested".into(),
            width: 64,
            height: 64,
            fps: RationalRate::new(24, 1).unwrap(),
            duration: frame_time(8),
            background_color: Color::black(),
            color_profile: "sRGB".into(),
            track_order: vec![nested_track_id],
            authored_properties: PropertyMap::new(),
            published_parameters: Vec::new(),
        },
    );
    project.tracks.insert(
        nested_track_id,
        TimelineTrack {
            id: nested_track_id,
            timeline_id: nested_id,
            name: "Nested audio".into(),
            kind: TimelineTrackKind::Audio,
            authored_properties: PropertyMap::new(),
        },
    );
    add_asset_item(&mut project, nested_track_id, asset_id, 1, 4, 2);

    let root_track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let composition_id = TimelineItemId::new();
    project.items.insert(
        composition_id,
        TimelineItem {
            id: composition_id,
            track_id: root_track_id,
            name: "Nested placement".into(),
            source: SourceRef::Composition(CompositionInstance {
                timeline_id: nested_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            interval: TimelineInterval::new(frame_time(2), frame_time(8)).unwrap(),
            time_map: TimeMap::default(),
            layer: 0,
            parent: None,
            blend_mode: crate::model::BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(2);
    let mut mixer = AuthoringAudioMixer::root(&project, &cache).unwrap();
    let rendered = mixer.render_window(0, 10).unwrap();
    for output_frame in 0..3 {
        assert_stereo_near(frame(&rendered, output_frame), [0.0, 0.0]);
    }
    for output_frame in 3..7 {
        assert_stereo_near(frame(&rendered, output_frame), samples[output_frame - 1]);
    }
    for output_frame in 7..10 {
        assert_stereo_near(frame(&rendered, output_frame), [0.0, 0.0]);
    }
}

#[test]
fn multiple_clips_mix_with_timeline_track_and_item_gain() {
    let directory = tempfile::tempdir().unwrap();
    let left_path = directory.path().join("left.wav");
    let right_path = directory.path().join("right.wav");
    write_stereo_wave(&left_path, &[[0.4; 2]; 8]);
    write_stereo_wave(&right_path, &[[0.2; 2]; 8]);

    let mut project = project_with_audio_track(8);
    let left_asset = add_audio_asset(&mut project, &left_path, 8);
    let right_asset = add_audio_asset(&mut project, &right_path, 8);
    let root_id = project.root_timeline_id;
    let track_id = project.timelines[&root_id].track_order[0];
    set_gain(
        &mut project
            .timelines
            .get_mut(&root_id)
            .unwrap()
            .authored_properties,
        0.5,
    );
    set_gain(
        &mut project
            .tracks
            .get_mut(&track_id)
            .unwrap()
            .authored_properties,
        0.5,
    );
    let left_item = add_asset_item(&mut project, track_id, left_asset, 0, 8, 0);
    let right_item = add_asset_item(&mut project, track_id, right_asset, 0, 8, 0);
    project.items.get_mut(&right_item).unwrap().layer = 1;
    set_gain(
        &mut project
            .items
            .get_mut(&left_item)
            .unwrap()
            .authored_properties,
        0.5,
    );
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(4);
    let mut mixer = AuthoringAudioMixer::root(&project, &cache).unwrap();
    let rendered = mixer.render_window(0, 4).unwrap();
    // 0.4 * (Timeline .5 * Track .5 * Item .5)
    // + 0.2 * (Timeline .5 * Track .5) = 0.1.
    for output_frame in 0..4 {
        assert_stereo_near(frame(&rendered, output_frame), [0.1; 2]);
    }
}

#[test]
fn active_decode_failure_is_an_error_not_successful_silence() {
    let mut project = project_with_audio_track(4);
    let missing = std::env::temp_dir().join(format!("missing-{}.wav", Uuid::new_v4()));
    let asset_id = add_audio_asset(&mut project, &missing, 4);
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    add_asset_item(&mut project, track_id, asset_id, 0, 4, 0);
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(2);
    let mut mixer = AuthoringAudioMixer::root(&project, &cache).unwrap();
    let error = mixer.render_window(0, 1).unwrap_err();
    assert!(matches!(error, AuthoringAudioError::Decode { .. }));
}

#[test]
fn video_assets_are_explicitly_reported_as_unsupported() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("video.mp4");
    std::fs::write(&path, b"not decoded in the audio-only vertical slice").unwrap();
    let mut project = project_with_audio_track(4);
    let asset = Asset::new("video", path.to_str().unwrap(), AssetKind::Video);
    let asset_id = asset.id;
    project.assets.push(asset);
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    add_asset_item(&mut project, track_id, asset_id, 0, 4, 0);
    project.validate().unwrap();

    let cache = CacheManager::with_audio_chunk_capacity(2);
    let mixer = AuthoringAudioMixer::root(&project, &cache).unwrap();
    assert_eq!(mixer.unsupported_video_assets(), &[asset_id]);
}

#[test]
fn realtime_device_format_controls_window_shape_and_bound() {
    let project = project_with_audio_track(48_000);
    let cache = CacheManager::with_audio_chunk_capacity(2);
    let timeline_id = project.root_timeline_id;
    let mut mixer =
        AuthoringAudioMixer::new_with_format(&project, &cache, timeline_id, 44_100, 6).unwrap();

    assert_eq!(mixer.render_window(0, 7).unwrap().len(), 42);
    let error = mixer.render_window(0, 44_101).unwrap_err();
    assert!(matches!(error, AuthoringAudioError::InvalidRequest(_)));
    assert!(AuthoringAudioMixer::new_with_format(&project, &cache, timeline_id, 0, 2).is_err());
}

mod transition_instance_tests;
