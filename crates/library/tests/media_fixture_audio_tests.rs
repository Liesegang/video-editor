mod support;

use anyhow::{Context, Result, anyhow, bail};
use library::cache::CacheManager;
use library::core::audio::cache::{AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
use library::core::audio::loader::AudioLoader;
use library::core::audio::mixer::{mix_samples, render_samples};
use library::editor::EditorService;
use library::editor::project_service::MediaNodeRequest;
use library::model::{Asset, AssetKind, Clip, Composition, NodeContainer, Project};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

use support::{
    channel_energy, media_node_for_canvas, media_project_with_asset, positive_zero_crossings,
};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data/e2e_media")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn dedicated_audio_loader_decodes_the_tiny_mp3_as_interleaved_stereo() -> Result<()> {
    let path = fixture("tone.mp3");
    assert!(AudioLoader::has_audio(&path));
    let format =
        AudioDecodeFormat::new(48_000, 2).context("48 kHz stereo decode format must be valid")?;
    let source = AudioSourceKey::read(&path, None, format)?;
    let chunk = AudioLoader::decode_chunk(&AudioChunkKey {
        source,
        chunk_index: 0,
    })?;
    let samples = chunk.samples();
    assert!(samples.len() > 90_000);
    assert_eq!(samples.len() % 2, 0);
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
    assert!(
        samples
            .chunks_exact(2)
            .take(1_000)
            .all(|frame| (frame[0] - frame[1]).abs() < f32::EPSILON)
    );
    Ok(())
}

#[test]
fn explicit_global_audio_stream_ordinals_decode_distinct_signals() -> Result<()> {
    let path = fixture("multi_audio.mkv");
    let format =
        AudioDecodeFormat::new(8_000, 2).context("8 kHz stereo decode format must be valid")?;
    let decode = |stream_index| -> Result<library::audio::cache::AudioChunk> {
        let source = AudioSourceKey::read(&path, stream_index, format)?;
        AudioLoader::decode_chunk(&AudioChunkKey {
            source,
            chunk_index: 0,
        })
    };

    assert!(
        AudioLoader::decode_chunk(&AudioChunkKey {
            source: AudioSourceKey::read(&path, Some(0), format)?,
            chunk_index: 0,
        })
        .is_err()
    );
    let default_audio = decode(None)?;
    let stream_one = decode(Some(1))?;
    let stream_two = decode(Some(2))?;

    assert_eq!(default_audio.samples(), stream_one.samples());
    assert!(channel_energy(stream_one.samples(), 0) > 0.001);
    assert!(channel_energy(stream_one.samples(), 1) < 0.000_001);
    assert!(channel_energy(stream_two.samples(), 1) > 0.001);
    assert!(channel_energy(stream_two.samples(), 0) < 0.000_001);
    let crossings_one = positive_zero_crossings(stream_one.samples(), 0);
    let crossings_two = positive_zero_crossings(stream_two.samples(), 1);
    assert!(
        (420..=460).contains(&crossings_one),
        "unexpected stream 1 frequency proxy: {crossings_one} crossings"
    );
    assert!(
        (850..=910).contains(&crossings_two),
        "unexpected stream 2 frequency proxy: {crossings_two} crossings"
    );
    Ok(())
}

#[test]
fn cold_render_survives_high_stretch_with_a_two_chunk_cache() -> Result<()> {
    let mut project = Project::new("bounded cold audio render");
    let (composition, track) = Composition::new("main", 8, 6, 12.0, 1.25);
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

    let mut asset = Asset::new(
        "multi audio video",
        &fixture("multi_audio.mkv"),
        AssetKind::Video,
    );
    asset.stream_index = Some(0);
    let asset_id = asset.id;
    project.assets.push(asset);
    let mut clip = Clip::new("retimed audio", 0.0, 1.25);
    clip.time_stretch = OrderedFloat(2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    let node = media_node_for_canvas(
        "explicit second audio stream",
        MediaNodeRequest::Video {
            asset_id,
            file_path: fixture("multi_audio.mkv"),
            stream_index: Some(0),
            audio_stream_index: Some(2),
        },
        8,
        6,
        8,
        6,
    );
    let node_id = node.id;
    project.add_node(node);
    support::attach_audio_output(&mut project, NodeContainer::Clip(clip_id), node_id)
        .map_err(|error| anyhow!(error))?;

    let cache = CacheManager::with_audio_chunk_capacity(2);
    let plugin_manager = PluginManager::default();
    let rendered = render_samples(
        &project.assets,
        &project,
        project
            .get_composition(composition_id)
            .context("audio composition must exist")?,
        &cache,
        0,
        10_000,
        8_000,
        2,
        &plugin_manager,
    );

    assert_eq!(rendered.len(), 20_000);
    assert!(cache.audio_chunk_cache_len() <= 2);
    assert!(cache.cached_audio_sample_count() <= 2 * 8_000 * 2);
    assert!(channel_energy(&rendered[..2_000], 1) > 0.001);
    assert!(channel_energy(&rendered[18_000..], 1) > 0.001);
    assert!(channel_energy(&rendered, 0) < 0.000_001);
    Ok(())
}

fn wait_for_audio(
    service: &EditorService,
    cache: &CacheManager,
    project: &Project,
    asset_id: Uuid,
    sample_rate: u32,
) -> Result<Arc<library::audio::cache::AudioChunk>> {
    let composition_id = project
        .compositions
        .first()
        .context("audio project must contain a composition")?
        .id;
    service.set_active_composition(Some(composition_id), 0.0);
    service.reset_audio_pump(0.0);
    let asset = project
        .get_asset(asset_id)
        .context("embedded-audio Asset must exist")?;
    let format = AudioDecodeFormat::new(sample_rate, 2)
        .context("editor sample rate must form a valid stereo decode format")?;
    let source = AudioSourceKey::read(&asset.path, None, format)?;
    let key = AudioChunkKey {
        source,
        chunk_index: 0,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        service.pump_audio();
        if let Some(audio) = cache.get_audio_chunk(&key) {
            return Ok(audio);
        }
        if Instant::now() >= deadline {
            bail!("timed out hydrating embedded audio for Asset {asset_id}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_nonzero_mix(
    service: &EditorService,
    project: &Project,
    cache: &CacheManager,
    sample_rate: u32,
    asset_id: Uuid,
) -> Result<()> {
    let cached = wait_for_audio(service, cache, project, asset_id, sample_rate)?;
    assert!(cached.samples().iter().any(|sample| sample.abs() > 0.001));
    let composition = project
        .compositions
        .first()
        .context("audio project must contain a composition")?;
    let mixed = mix_samples(
        &project.assets,
        project,
        composition,
        cache,
        0,
        (sample_rate / 4) as usize,
        sample_rate,
        2,
        service.get_plugin_manager().as_ref(),
    );
    assert!(
        mixed.iter().any(|sample| sample.abs() > 0.001),
        "hydrated Video Asset must contribute embedded audio under its own ID"
    );
    Ok(())
}

#[test]
fn import_and_load_hydrate_embedded_audio_under_the_video_asset_id() -> Result<()> {
    let path = fixture("av_duration_mismatch.mp4");
    assert!(AudioLoader::has_audio(&path));
    assert!(
        !AudioLoader::has_audio(&fixture("h264_24.mp4")),
        "a video codec must not be mistaken for an audio track"
    );

    let plugins = Arc::new(PluginManager::default());
    let shared = Arc::new(RwLock::new(Project::new("import target")));
    let cache = Arc::new(CacheManager::new());
    let service = EditorService::new(
        Arc::clone(&shared),
        Arc::clone(&plugins),
        Arc::clone(&cache),
    )?;
    let sample_rate = service.get_audio_engine().get_sample_rate();

    let imported_ids = service.import_file(&path)?;
    let imported_video = shared
        .read()
        .map_err(|error| anyhow!("project lock poisoned: {error}"))?
        .assets
        .iter()
        .find(|asset| imported_ids.contains(&asset.id) && asset.kind == AssetKind::Video)
        .cloned()
        .context("import must produce a Video Asset")?;
    assert_eq!(imported_video.frame_count, Some(12));
    assert_eq!(imported_video.stream_index, Some(0));

    let (imported_project, imported_video_id) = media_project_with_asset(imported_video)?;
    service.set_project(imported_project.clone())?;
    assert_nonzero_mix(
        &service,
        &imported_project,
        &cache,
        sample_rate,
        imported_video_id,
    )?;

    let mut loaded_asset = Asset::new("loaded AV", &path, AssetKind::Video);
    loaded_asset.duration = Some(2.0);
    loaded_asset.fps = Some(12.0);
    loaded_asset.frame_count = Some(12);
    loaded_asset.width = Some(12);
    loaded_asset.height = Some(8);
    loaded_asset.stream_index = Some(0);
    let (loaded_project, loaded_video_id) = media_project_with_asset(loaded_asset)?;
    service.load_project(&loaded_project.save()?)?;
    let loaded_snapshot = shared
        .read()
        .map_err(|error| anyhow!("project lock poisoned: {error}"))?
        .clone();
    assert_eq!(
        loaded_snapshot
            .get_asset(loaded_video_id)
            .context("loaded Video Asset must exist")?
            .frame_count,
        Some(12)
    );
    assert_nonzero_mix(
        &service,
        &loaded_snapshot,
        &cache,
        sample_rate,
        loaded_video_id,
    )?;
    Ok(())
}
