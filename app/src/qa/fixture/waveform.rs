//! Deterministic Audio-only and embedded-Video-audio Timeline waveform fixture.

use super::FixtureInfo;
use library::editor::project_service::MediaNodeRequest;
use library::editor::ProjectService;
use library::model::project::{PortAddress, PortOwner, TIME_PORT};
use library::model::{Asset, AssetKind, Clip, Composition, NodeContainer, Project, Track};
use uuid::Uuid;

pub(super) const COMPOSITION_ID: Uuid = Uuid::from_u128(0x8_100);
pub(super) const AUDIO_TRACK_ID: Uuid = Uuid::from_u128(0x8_201);
pub(super) const VIDEO_TRACK_ID: Uuid = Uuid::from_u128(0x8_202);
pub(super) const AUDIO_CLIP_ID: Uuid = Uuid::from_u128(0x8_301);
pub(super) const VIDEO_CLIP_ID: Uuid = Uuid::from_u128(0x8_302);
pub(super) const AUDIO_NODE_ID: Uuid = Uuid::from_u128(0x8_401);
pub(super) const VIDEO_NODE_ID: Uuid = Uuid::from_u128(0x8_402);
pub(super) const AUDIO_ASSET_ID: Uuid = Uuid::from_u128(0x8_501);
pub(super) const VIDEO_ASSET_ID: Uuid = Uuid::from_u128(0x8_502);

const AUDIO_PATH: &str = "test_data/test_sound2.mp3";
const VIDEO_PATH: &str = "test_data/e2e_media/multi_audio.mkv";

pub(super) fn install(
    project: &mut Project,
    factory: &ProjectService,
) -> Result<FixtureInfo, String> {
    project.name = "RuViE Audio Waveform QA".to_string();

    let (mut composition, _) = Composition::new("Waveform QA", 640, 360, 30.0, 10.0);
    composition.id = COMPOSITION_ID;
    composition.track_ids = vec![AUDIO_TRACK_ID, VIDEO_TRACK_ID];

    let mut audio_track = Track::new("Audio-only source");
    audio_track.id = AUDIO_TRACK_ID;
    audio_track.clip_ids = vec![AUDIO_CLIP_ID];
    let mut video_track = Track::new("Video embedded audio");
    video_track.id = VIDEO_TRACK_ID;
    video_track.clip_ids = vec![VIDEO_CLIP_ID];

    let mut audio_clip = Clip::new("Long MP3", 0.5, 6.0);
    audio_clip.id = AUDIO_CLIP_ID;
    audio_clip.trim_in = 1.0.into();
    audio_clip.time_stretch = 1.0.into();
    audio_clip.node_ids = vec![AUDIO_NODE_ID];
    audio_clip.audio_output_node_id = Some(AUDIO_NODE_ID);

    let mut video_clip = Clip::new("Embedded AAC stream", 1.5, 1.5);
    video_clip.id = VIDEO_CLIP_ID;
    video_clip.trim_in = 0.2.into();
    video_clip.time_stretch = 0.75.into();
    video_clip.node_ids = vec![VIDEO_NODE_ID];
    video_clip.output_node_id = Some(VIDEO_NODE_ID);
    video_clip.audio_output_node_id = Some(VIDEO_NODE_ID);

    let mut audio_asset = Asset::new("Long MP3", AUDIO_PATH, AssetKind::Audio);
    audio_asset.id = AUDIO_ASSET_ID;
    audio_asset.duration = Some(73.332_018);
    let mut video_asset = Asset::new("Multi-audio MKV", VIDEO_PATH, AssetKind::Video);
    video_asset.id = VIDEO_ASSET_ID;
    video_asset.duration = Some(3.128);
    video_asset.width = Some(8);
    video_asset.height = Some(6);
    video_asset.fps = Some(5.0);
    video_asset.stream_index = Some(0);

    let mut audio_node = factory
        .create_media_node(
            "Long MP3",
            MediaNodeRequest::Audio {
                asset_id: AUDIO_ASSET_ID,
                file_path: AUDIO_PATH.to_string(),
                audio_stream_index: None,
            },
            640,
            360,
            1,
            1,
        )
        .map_err(|error| format!("cannot create waveform Audio node: {error}"))?;
    audio_node.id = AUDIO_NODE_ID;
    let mut video_node = factory
        .create_media_node(
            "Embedded AAC stream",
            MediaNodeRequest::Video {
                asset_id: VIDEO_ASSET_ID,
                file_path: VIDEO_PATH.to_string(),
                stream_index: Some(0),
                audio_stream_index: Some(1),
            },
            640,
            360,
            8,
            6,
        )
        .map_err(|error| format!("cannot create waveform Video node: {error}"))?;
    video_node.id = VIDEO_NODE_ID;

    project
        .add_track(audio_track)
        .expect("container structural Merge insertion must succeed");
    project
        .add_track(video_track)
        .expect("container structural Merge insertion must succeed");
    project.add_clip(audio_clip);
    project.add_clip(video_clip);
    project.assets.push(audio_asset);
    project.assets.push(video_asset);
    project.add_node(audio_node);
    project.add_node(video_node);
    project
        .add_composition(composition)
        .expect("container structural Merge insertion must succeed");

    for (clip_id, node_id) in [
        (AUDIO_CLIP_ID, AUDIO_NODE_ID),
        (VIDEO_CLIP_ID, VIDEO_NODE_ID),
    ] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
                PortAddress::new(PortOwner::Node(node_id), TIME_PORT),
            )
            .map_err(|error| format!("cannot connect waveform time metadata: {error}"))?;
    }
    let errors = project.validate_connections();
    if !errors.is_empty() {
        return Err(format!(
            "waveform QA fixture has invalid graph connections: {}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    // Both Clip bindings are direct and typed. Composition/Track Audio remains
    // derived from ordered child containers, matching normal Timeline authoring.
    debug_assert_eq!(
        project
            .get_clip(AUDIO_CLIP_ID)
            .and_then(|clip| clip.audio_output_node_id),
        Some(AUDIO_NODE_ID)
    );
    debug_assert_eq!(
        project
            .get_clip(VIDEO_CLIP_ID)
            .and_then(|clip| clip.audio_output_node_id),
        Some(VIDEO_NODE_ID)
    );
    debug_assert_eq!(
        project.find_node_container(AUDIO_NODE_ID),
        Some(NodeContainer::Clip(AUDIO_CLIP_ID))
    );

    Ok(FixtureInfo {
        composition_id: COMPOSITION_ID,
        expanded_tracks: vec![AUDIO_TRACK_ID, VIDEO_TRACK_ID],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::editor::audio_service::routed_audio_media_nodes_for_clip;
    use library::model::NodeContent;
    use library::plugin::PluginManager;
    use std::sync::{Arc, RwLock};

    #[test]
    fn fixture_covers_audio_only_and_video_embedded_audio() {
        let shared = Arc::new(RwLock::new(Project::new("empty")));
        let plugins = Arc::new(PluginManager::default());
        let factory = ProjectService::new(Arc::clone(&shared), plugins);
        let mut project = shared.write().unwrap();
        let info = install(&mut project, &factory).unwrap();

        assert_eq!(info.composition_id, COMPOSITION_ID);
        assert_eq!(
            routed_audio_media_nodes_for_clip(&project, AUDIO_CLIP_ID),
            vec![AUDIO_NODE_ID]
        );
        assert_eq!(
            routed_audio_media_nodes_for_clip(&project, VIDEO_CLIP_ID),
            vec![VIDEO_NODE_ID]
        );
        let NodeContent::Media(video) = project.get_node(VIDEO_NODE_ID).unwrap().content() else {
            panic!("waveform Video fixture node must be Media")
        };
        assert_eq!(video.stream_index, Some(0));
        assert_eq!(video.audio_stream_index, Some(1));
        assert!(project.validate_connections().is_empty());
    }
}
