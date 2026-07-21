use anyhow::{Context, Result};
use library::editor::project_service::{MediaNodeRequest, ProjectManager};
use library::model::asset::{Asset, AssetKind};
use library::model::project::{
    AUDIO_OUTPUT_PORT, Composition, ContainerAudioSourceKind, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
    NodeContainer, PortAddress, PortDataType, PortDirection, PortOwner, Project, ProjectGraphError,
};
use library::model::{Clip, Node};
use library::plugin::PluginManager;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

fn project_with_clip() -> Result<(Project, Uuid, Uuid, Uuid)> {
    let mut project = Project::new("typed outputs");
    let (composition, track) = Composition::new("main", 64, 64, 24.0, 2.0);
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
    let clip = Clip::new("media", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    Ok((project, composition_id, track_id, clip_id))
}

fn media_node(manager: &ProjectManager, asset: &Asset) -> Result<Node> {
    let request = match asset.kind {
        AssetKind::Audio => MediaNodeRequest::Audio {
            asset_id: asset.id,
            file_path: asset.path.clone(),
            audio_stream_index: None,
        },
        AssetKind::Video => MediaNodeRequest::Video {
            asset_id: asset.id,
            file_path: asset.path.clone(),
            stream_index: asset.stream_index,
            audio_stream_index: None,
        },
        AssetKind::Image => MediaNodeRequest::Image {
            asset_id: asset.id,
            file_path: asset.path.clone(),
        },
        _ => anyhow::bail!("unsupported test Asset kind"),
    };
    manager
        .create_media_node("media", request, 64, 64, 64, 64)
        .map_err(anyhow::Error::msg)
}

fn typed_media_project() -> Result<(Project, Uuid, Uuid, Uuid, Uuid, Uuid, Uuid)> {
    let (mut project, composition_id, track_id, clip_id) = project_with_clip()?;
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::new(PluginManager::default()),
    );
    let assets = [
        Asset::new("audio", "/fixture/audio.wav", AssetKind::Audio),
        Asset::new("image", "/fixture/image.png", AssetKind::Image),
        Asset::new("video", "/fixture/video.mp4", AssetKind::Video),
    ];
    let mut node_ids = Vec::new();
    for asset in assets {
        let node = media_node(&manager, &asset)?;
        let node_id = node.id;
        project.assets.push(asset);
        project.add_node(node);
        project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id)?;
        node_ids.push(node_id);
    }
    Ok((
        project,
        composition_id,
        track_id,
        clip_id,
        node_ids[0],
        node_ids[1],
        node_ids[2],
    ))
}

fn output_type(project: &Project, owner: PortOwner, key: &str) -> Option<PortDataType> {
    project
        .port_definitions(owner)
        .into_iter()
        .find(|port| port.direction == PortDirection::Output && port.key == key)
        .map(|port| port.data_type)
}

#[test]
fn containers_are_stably_dual_typed_and_media_outputs_follow_asset_kind() -> Result<()> {
    let (project, composition_id, track_id, clip_id, audio_id, image_id, video_id) =
        typed_media_project()?;

    for owner in [
        PortOwner::Composition(composition_id),
        PortOwner::Track(track_id),
        PortOwner::Clip(clip_id),
    ] {
        assert_eq!(
            output_type(&project, owner, IMAGE_OUTPUT_PORT),
            Some(PortDataType::Image)
        );
        assert_eq!(
            output_type(&project, owner, AUDIO_OUTPUT_PORT),
            Some(PortDataType::Audio)
        );
    }

    assert_eq!(
        output_type(&project, PortOwner::Node(audio_id), IMAGE_OUTPUT_PORT),
        None
    );
    assert_eq!(
        output_type(&project, PortOwner::Node(audio_id), AUDIO_OUTPUT_PORT),
        Some(PortDataType::Audio)
    );
    assert_eq!(
        output_type(&project, PortOwner::Node(image_id), IMAGE_OUTPUT_PORT),
        Some(PortDataType::Image)
    );
    assert_eq!(
        output_type(&project, PortOwner::Node(image_id), AUDIO_OUTPUT_PORT),
        None
    );
    assert_eq!(
        output_type(&project, PortOwner::Node(video_id), IMAGE_OUTPUT_PORT),
        Some(PortDataType::Image)
    );
    assert_eq!(
        output_type(&project, PortOwner::Node(video_id), AUDIO_OUTPUT_PORT),
        Some(PortDataType::Audio)
    );
    Ok(())
}

#[test]
fn bindings_reject_cross_typed_media_and_video_can_bind_both() -> Result<()> {
    let (mut project, _, _, clip_id, audio_id, image_id, video_id) = typed_media_project()?;
    let container = NodeContainer::Clip(clip_id);

    assert!(matches!(
        project.set_output_node(container, Some(audio_id)),
        Err(ProjectGraphError::OutputNodeHasNoImagePort { node_id, .. }) if node_id == audio_id
    ));
    assert!(matches!(
        project.set_audio_output_node(container, Some(image_id)),
        Err(ProjectGraphError::OutputNodeHasNoAudioPort { node_id, .. }) if node_id == image_id
    ));

    project.set_output_node(container, Some(video_id))?;
    project.set_audio_output_node(container, Some(video_id))?;
    let clip = project.get_clip(clip_id).context("Clip disappeared")?;
    assert_eq!(clip.output_node_id, Some(video_id));
    assert_eq!(clip.audio_output_node_id, Some(video_id));

    assert_eq!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .len(),
        1
    );
    let audio_sources = project.container_audio_sources(PortOwner::Clip(clip_id));
    assert_eq!(audio_sources.len(), 1);
    assert_eq!(audio_sources[0].source, PortOwner::Node(video_id));
    assert_eq!(
        audio_sources[0].kind,
        ContainerAudioSourceKind::OutputBinding
    );
    Ok(())
}

#[test]
fn missing_asset_media_has_no_typed_output_to_masquerade_as() -> Result<()> {
    let (mut project, _, _, clip_id) = project_with_clip()?;
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::new(PluginManager::default()),
    );
    let missing = Asset::new("missing", "/fixture/missing.wav", AssetKind::Audio);
    let node = media_node(&manager, &missing)?;
    let node_id = node.id;
    project.add_node(node);
    project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id)?;

    assert_eq!(
        output_type(&project, PortOwner::Node(node_id), IMAGE_OUTPUT_PORT),
        None
    );
    assert_eq!(
        output_type(&project, PortOwner::Node(node_id), AUDIO_OUTPUT_PORT),
        None
    );
    assert!(matches!(
        project.set_audio_output_node(NodeContainer::Clip(clip_id), Some(node_id)),
        Err(ProjectGraphError::OutputNodeHasNoAudioPort { .. })
    ));
    Ok(())
}

#[test]
fn detach_and_reparent_clear_source_bindings_and_preserve_destination_bindings() -> Result<()> {
    let (mut project, _, track_id, clip_id, _, _, first_video) = typed_media_project()?;
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::new(PluginManager::default()),
    );
    let second_asset = Asset::new("video 2", "/fixture/video-2.mp4", AssetKind::Video);
    let second_video = media_node(&manager, &second_asset)?;
    let second_video_id = second_video.id;
    project.assets.push(second_asset);
    project.add_node(second_video);
    project.attach_node_to_container(NodeContainer::Track(track_id), second_video_id)?;

    project.set_output_node(NodeContainer::Clip(clip_id), Some(first_video))?;
    project.set_audio_output_node(NodeContainer::Clip(clip_id), Some(first_video))?;
    let track_merge_id = project
        .get_track(track_id)
        .context("Track disappeared")?
        .structural_merge_node_id;
    project.connect_ports(
        PortAddress::new(PortOwner::Node(second_video_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(track_merge_id), MERGE_IMAGES_PORT),
    )?;
    project.set_audio_output_node(NodeContainer::Track(track_id), Some(second_video_id))?;

    project.attach_node_to_container(NodeContainer::Track(track_id), first_video)?;
    let clip = project.get_clip(clip_id).context("Clip disappeared")?;
    assert_eq!(clip.output_node_id, None);
    assert_eq!(clip.audio_output_node_id, None);
    let track = project.get_track(track_id).context("Track disappeared")?;
    assert_eq!(track.output_node_id, Some(track_merge_id));
    assert_eq!(track.audio_output_node_id, Some(second_video_id));

    assert!(project.detach_node(second_video_id));
    let track = project.get_track(track_id).context("Track disappeared")?;
    assert_eq!(track.output_node_id, Some(track_merge_id));
    assert_eq!(track.audio_output_node_id, None);
    Ok(())
}

#[test]
fn audio_bindings_are_required_pre_v1_serialized_state() -> Result<()> {
    let (project, composition_id, track_id, clip_id) = project_with_clip()?;
    let encoded = project.save()?;
    let decoded = Project::load(&encoded)?;
    assert_eq!(decoded, project);

    let value: serde_json::Value = serde_json::from_str(&encoded)?;
    let cases = [
        ("compositions", composition_id.to_string()),
        ("tracks", track_id.to_string()),
        ("clips", clip_id.to_string()),
    ];
    for (collection, id) in cases {
        let mut missing = value.clone();
        let object = if collection == "compositions" {
            missing[collection][0]
                .as_object_mut()
                .context("Composition JSON is not an object")?
        } else {
            missing[collection][&id]
                .as_object_mut()
                .context("container JSON is not an object")?
        };
        assert!(object.remove("audio_output_node_id").is_some());
        assert!(
            Project::load(&serde_json::to_string(&missing)?).is_err(),
            "{collection} silently defaulted missing audio_output_node_id"
        );
    }
    Ok(())
}
