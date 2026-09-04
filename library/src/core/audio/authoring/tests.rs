use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use ordered_float::OrderedFloat;

use super::*;
use crate::model::authoring::{
    CompositionInstance, DurationPolicy, RationalRate, TimeMap, Timeline, TimelineInterval,
    TimelineTrack, TimelineTrackId,
};
use crate::model::frame::color::Color;
use crate::model::project::property::{Property, PropertyValue};

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
