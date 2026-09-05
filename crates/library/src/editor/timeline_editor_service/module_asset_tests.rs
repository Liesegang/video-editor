use std::collections::HashMap;

use super::*;
use crate::model::NodeContent;
use crate::model::asset::{Asset, AssetKind};
use crate::model::authoring::{
    MediaTime, ModuleDefinitionSharing, ModuleNodePortContract, ModuleTemplateOrigin,
    TimelineInterval,
};
use crate::model::project::{PortDataType, PortDirection};

fn placed_reusable_instance(
    service: &TimelineEditorService,
) -> (ModuleInstanceId, ModuleDefinitionId) {
    let project = service.snapshot().expect("Project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (definition, output_id) = ModuleDefinition::new_image(
        "Asset drop",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    let definition_id = definition.id;
    service
        .add_module_definition(definition)
        .expect("Module definition");
    let (_, instance_id, _) = service
        .place_module_item(
            definition_id,
            ModuleItemPlacement {
                track_id,
                name: "Node Clip".to_string(),
                output_id,
                interval: TimelineInterval::new(
                    MediaTime::zero(),
                    MediaTime::new(2, 1).expect("duration"),
                )
                .expect("interval"),
                layer: 0,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .expect("Node Clip");
    (instance_id, definition_id)
}

#[test]
fn asset_media_insert_resolves_identity_and_makes_reusable_instance_private() {
    let service = TimelineEditorService::create_default("Asset Module").expect("service");
    let (instance_id, shared_definition_id) = placed_reusable_instance(&service);
    let mut asset = Asset::new("Still", "still.png", AssetKind::Image);
    asset.width = Some(640);
    asset.height = Some(360);
    let asset_id = asset.id;
    service.add_asset(asset).expect("Asset");

    let (node_id, private_definition_id, _) = service
        .add_asset_to_instance_module(
            instance_id,
            asset_id,
            [123.0, 456.0],
            &PluginManager::default(),
            1920,
            1080,
        )
        .expect("Media Node");

    assert_ne!(private_definition_id, shared_definition_id);
    let project = service.snapshot().expect("Project");
    assert_eq!(
        project.module_instances[&instance_id].definition_id,
        private_definition_id
    );
    let node = &project.module_definitions[&private_definition_id]
        .graph
        .nodes[&node_id];
    assert_eq!(node.ui_position, [123.0, 456.0]);
    assert!(matches!(
        node.content(),
        NodeContent::Media(media) if media.asset_id == asset_id
    ));
    assert!(
        !project.module_definitions[&shared_definition_id]
            .graph
            .nodes
            .contains_key(&node_id)
    );
}

#[test]
fn unsupported_asset_insert_does_not_clone_or_mutate_the_module() {
    let service = TimelineEditorService::create_default("Unsupported Asset").expect("service");
    let (instance_id, definition_id) = placed_reusable_instance(&service);
    let asset = Asset::new("Model", "model.fbx", AssetKind::Model3D);
    let asset_id = asset.id;
    service.add_asset(asset).expect("Asset");
    let before = service.snapshot().expect("before");
    let node_count = before.module_definitions[&definition_id].graph.nodes.len();
    drop(before);

    let error = service
        .add_asset_to_instance_module(
            instance_id,
            asset_id,
            [10.0, 20.0],
            &PluginManager::default(),
            1920,
            1080,
        )
        .expect_err("3D Asset has no 2D Media Node");
    assert!(
        error
            .to_string()
            .contains("cannot be used as a 2D Media Node")
    );

    let project = service.snapshot().expect("after");
    assert_eq!(
        project.module_instances[&instance_id].definition_id,
        definition_id
    );
    assert_eq!(
        project.module_definitions[&definition_id].graph.nodes.len(),
        node_count
    );
}

fn output_types(node: &crate::model::Node) -> Vec<PortDataType> {
    ModuleNodePortContract::resolve(node)
        .expect("Media port contract")
        .ports
        .into_iter()
        .filter_map(|port| (port.direction == PortDirection::Output).then_some(port.data_type))
        .collect()
}

#[test]
fn media_ports_follow_explicit_selection_not_optional_stream_indices() {
    let asset_id = uuid::Uuid::new_v4();
    let node = |output_selection| {
        crate::model::Node::from_media_converter(
            "Media",
            crate::model::MediaContent {
                asset_id,
                output_selection,
                stream_index: None,
                audio_stream_index: None,
            },
            &[],
            "source.mkv".to_string(),
        )
        .expect("Media Node")
    };

    assert_eq!(
        output_types(&node(crate::model::MediaOutputSelection::Image)),
        vec![PortDataType::Image]
    );
    assert_eq!(
        output_types(&node(crate::model::MediaOutputSelection::Audio)),
        vec![PortDataType::Audio]
    );
    assert_eq!(
        output_types(&node(crate::model::MediaOutputSelection::ImageAndAudio)),
        vec![PortDataType::Image, PortDataType::Audio]
    );
}

#[test]
fn media_asset_nodes_keep_stream_identity_and_expose_only_selected_media() {
    let service = TimelineEditorService::create_default("Media streams").expect("service");
    let (instance_id, _) = placed_reusable_instance(&service);
    let image = Asset::new("Still", "still.png", AssetKind::Image);
    let image_id = image.id;
    service.add_asset(image).expect("Image Asset");
    let mut audio = Asset::new("Audio stream", "source.mkv", AssetKind::Audio);
    audio.stream_index = Some(7);
    let audio_id = audio.id;
    service.add_asset(audio).expect("Audio Asset");
    let mut video = Asset::new("Video stream", "source.mkv", AssetKind::Video);
    video.stream_index = Some(3);
    video.width = Some(1920);
    video.height = Some(1080);
    let video_id = video.id;
    service.add_asset(video).expect("Video Asset");
    let plugins = PluginManager::default();

    let (image_node_id, _, _) = service
        .add_asset_to_instance_module(instance_id, image_id, [10.0, 15.0], &plugins, 1920, 1080)
        .expect("Image Node");
    let (audio_node_id, _, _) = service
        .add_asset_to_instance_module(instance_id, audio_id, [20.0, 30.0], &plugins, 1920, 1080)
        .expect("Audio Node");
    let (video_node_id, _, _) = service
        .add_asset_to_instance_module(instance_id, video_id, [40.0, 50.0], &plugins, 1920, 1080)
        .expect("Video Node");
    let combined = crate::editor::AuthoringNodeFactory::create_media(
        &plugins,
        "Combined streams",
        crate::editor::project_service::MediaNodeRequest::Video {
            asset_id: video_id,
            file_path: "source.mkv".to_string(),
            stream_index: Some(3),
            audio_stream_index: Some(7),
            outputs: crate::model::MediaOutputSelection::ImageAndAudio,
        },
        1920,
        1080,
        1920,
        1080,
    )
    .expect("Combined Media Node");
    let (combined_node_id, definition_id, _) = service
        .add_instance_module_node(instance_id, combined)
        .expect("Combined Node");

    let project = service.snapshot().expect("Project");
    let definition = &project.module_definitions[&definition_id];
    let image = &definition.graph.nodes[&image_node_id];
    assert_eq!(output_types(image), vec![PortDataType::Image]);
    let NodeContent::Media(audio) = definition.graph.nodes[&audio_node_id].content() else {
        panic!("Audio Media Node");
    };
    assert_eq!(audio.asset_id, audio_id);
    assert_eq!(audio.stream_index, None);
    assert_eq!(audio.audio_stream_index, Some(7));
    assert_eq!(
        output_types(&definition.graph.nodes[&audio_node_id]),
        vec![PortDataType::Audio]
    );
    let NodeContent::Media(video) = definition.graph.nodes[&video_node_id].content() else {
        panic!("Video Media Node");
    };
    assert_eq!(video.asset_id, video_id);
    assert_eq!(video.stream_index, Some(3));
    assert_eq!(video.audio_stream_index, None);
    assert_eq!(
        output_types(&definition.graph.nodes[&video_node_id]),
        vec![PortDataType::Image]
    );
    assert_eq!(
        output_types(&definition.graph.nodes[&combined_node_id]),
        vec![PortDataType::Image, PortDataType::Audio]
    );
}

#[test]
fn sksl_generator_clip_is_one_private_module_with_a_connected_output() {
    let service = TimelineEditorService::create_default("Shader Clip").expect("service");
    let project = service.snapshot().expect("Project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);

    let (item_id, instance_id, definition_id, _) = service
        .create_generator_node_clip(
            &PluginManager::default(),
            crate::editor::ModuleNodeRequest::SkSL {
                shader: crate::editor::project_service::DEFAULT_SKSL_SHADER.to_string(),
            },
            GeneratorNodeClipPlacement {
                track_id,
                name: "SkSL Shader".to_string(),
                interval: TimelineInterval::new(
                    MediaTime::zero(),
                    MediaTime::new(5, 1).expect("duration"),
                )
                .expect("interval"),
                layer: 0,
            },
            1920,
            1080,
        )
        .expect("SkSL Node Clip");

    let project = service.snapshot().expect("Project");
    let definition = &project.module_definitions[&definition_id];
    assert_eq!(definition.sharing, ModuleDefinitionSharing::Private);
    assert_eq!(definition.graph.nodes.len(), 2);
    assert_eq!(definition.graph.connections.len(), 1);
    assert_eq!(
        project.module_instances[&instance_id].definition_id,
        definition_id
    );
    assert!(matches!(
        project.items[&item_id].source,
        SourceRef::Module(_)
    ));
    assert!(definition.graph.nodes.values().any(|node| matches!(
        node.content(),
        NodeContent::Generator(crate::model::GeneratorContent::SkSL)
    )));
}
