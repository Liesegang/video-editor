mod support;

use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use library::cache::CacheManager;
use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
use library::framing::get_frame_from_project;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroup, FrameItem};
use library::model::project::{
    AUDIO_OUTPUT_PORT, Composition, DURATION_PORT, FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT,
    FPS_PORT, FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NUMBER_RESULT_OUTPUT_PORT,
    NodeContainer, PortAddress, PortDataType, PortDirection, PortOwner, Project, ProjectGraphError,
    RESOLUTION_PORT, TIME_PORT,
};
use library::model::property::{Property, PropertyValue};
use library::model::{Clip, CompositionInstanceContent, Node, NodeContent};
use library::plugin::PluginManager;
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use support::generator_node_for_canvas;

fn add_node(project: &mut Project, container: NodeContainer, node: Node) -> Result<Uuid> {
    let node_id = node.id;
    project.add_node(node);
    project.attach_node_to_container(container, node_id)?;
    Ok(node_id)
}

fn address(owner: PortOwner, port: &str) -> PortAddress {
    PortAddress::new(owner, port)
}

fn evaluate_frame(project: &Project) -> Result<library::model::frame::frame::FrameInfo> {
    let plugins = Arc::new(PluginManager::default());
    Ok(get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?)
}

fn render(project: &Project) -> Result<library::model::frame::Image> {
    let plugins = Arc::new(PluginManager::default());
    let frame = get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?;
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )?;
    let mut service = RenderService::new(renderer, plugins, Arc::new(CacheManager::new()));
    match service.render_from_frame_info(&frame)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Texture(_) => anyhow::bail!("CPU renderer returned a Texture"),
    }
}

fn find_group(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Object(_) => None,
    })
}

fn object_source_ids(items: &[FrameItem]) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for item in items {
        match item {
            FrameItem::Object(object) => ids.push(object.source_node_id),
            FrameItem::Group(group) => ids.extend(object_source_ids(&group.items)),
        }
    }
    ids
}

#[test]
fn factory_ports_ownership_and_json_are_canonical() -> Result<()> {
    let mut project = Project::new("composition instance contract");
    let (target, target_track) = Composition::new("target", 640, 360, 24.0, 4.0);
    let target_id = target.id;
    project.add_track(target_track)?;
    project.add_composition(target)?;
    let (parent, parent_track) = Composition::new("parent", 640, 360, 24.0, 4.0);
    let parent_id = parent.id;
    let parent_track_id = parent_track.id;
    project.add_track(parent_track)?;
    project.add_composition(parent)?;

    let manager = ProjectManager::new(
        Arc::new(RwLock::new(project.clone())),
        Arc::new(PluginManager::default()),
    );
    let bundle = manager.create_composition_instance_clip(target_id, 1.0, 2.0)?;
    let node = bundle
        .graph
        .nodes
        .iter()
        .find(|node| matches!(node.content(), NodeContent::CompositionInstance(_)))
        .context("factory must retain the Composition Instance source")?;
    let node_id = node.id;
    let transform = bundle
        .graph
        .nodes
        .iter()
        .find(|node| matches!(node.content(), NodeContent::PluginOperation(_)))
        .context("factory must create one Image Transform")?;
    let transform_id = transform.id;
    assert_eq!(bundle.graph.nodes.len(), 2);
    assert!(bundle.clip.node_ids.is_empty());
    assert_eq!(bundle.graph.output_node_id, Some(transform_id));
    assert_eq!(bundle.clip.output_node_id, None);
    assert_eq!(bundle.clip.audio_output_node_id, Some(node_id));
    assert_eq!(
        node.content(),
        &NodeContent::CompositionInstance(CompositionInstanceContent {
            composition_id: target_id,
        })
    );

    assert!(bundle.graph.connections.iter().any(|connection| {
        connection.from == address(PortOwner::Node(node_id), IMAGE_OUTPUT_PORT)
            && connection.to == address(PortOwner::Node(transform_id), IMAGE_INPUT_PORT)
    }));

    let clip_id = bundle.clip.id;
    project.add_clip(bundle.clip);
    project.attach_clip_to_track(parent_track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), bundle.graph)?;
    project.set_audio_output_node(NodeContainer::Clip(clip_id), Some(node_id))?;

    let ports = project.port_definitions(PortOwner::Node(node_id));
    let inputs = ports
        .iter()
        .filter(|port| port.direction == PortDirection::Input)
        .collect::<Vec<_>>();
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        (inputs[0].key.as_str(), inputs[0].data_type),
        (TIME_PORT, PortDataType::Number)
    );
    assert!(!ports.iter().any(|port| {
        port.direction == PortDirection::Input && port.data_type == PortDataType::Image
    }));
    for (key, data_type) in [
        (TIME_PORT, PortDataType::Number),
        (FRAME_PORT, PortDataType::Integer),
        (FPS_PORT, PortDataType::Number),
        (DURATION_PORT, PortDataType::Number),
        (RESOLUTION_PORT, PortDataType::Vec2),
        (IMAGE_OUTPUT_PORT, PortDataType::Image),
        (AUDIO_OUTPUT_PORT, PortDataType::Audio),
    ] {
        assert!(ports.iter().any(|port| {
            port.key == key
                && port.direction == PortDirection::Output
                && port.data_type == data_type
        }));
    }
    assert_eq!(
        ports
            .iter()
            .filter(|port| port.direction == PortDirection::Output)
            .count(),
        7
    );
    assert!(project.validate_connections().is_empty());

    let json = project.save()?;
    let value: serde_json::Value = serde_json::from_str(&json)?;
    let content = &value["nodes"][node_id.to_string()]["content"];
    assert_eq!(content["type"], "CompositionInstance");
    assert_eq!(
        content["data"],
        serde_json::json!({ "composition_id": target_id })
    );
    assert_eq!(Project::load(&json)?, project);

    let invalid = Node::new_composition_instance(
        "outside Clip",
        CompositionInstanceContent {
            composition_id: target_id,
        },
    );
    let invalid_id = invalid.id;
    project.add_node(invalid);
    project
        .get_composition_mut(parent_id)
        .context("parent Composition must exist")?
        .node_ids
        .push(invalid_id);
    assert!(project.validate_connections().iter().any(|error| matches!(
        error,
        ProjectGraphError::CompositionInstanceOutsideClip {
            node_id,
            container: NodeContainer::Composition(composition_id),
        } if *node_id == invalid_id && *composition_id == parent_id
    )));
    Ok(())
}

#[test]
fn two_instances_render_one_definition_at_independent_explicit_times() -> Result<()> {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut project = Project::new("independent composition instance image time");
    let (mut parent, parent_track) = Composition::new("parent", 4, 2, 4.0, 1.0);
    parent.background_color = transparent.clone();
    let parent_id = parent.id;
    let parent_track_id = parent_track.id;
    project.add_track(parent_track)?;
    project.add_composition(parent)?;

    let (mut source, source_track) = Composition::new("source", 2, 2, 4.0, 2.0);
    source.background_color = transparent;
    let source_id = source.id;
    let source_track_id = source_track.id;
    project.add_track(source_track)?;
    project.add_composition(source)?;

    let mut source_node_ids = Vec::new();
    for (name, start_time, color) in [
        (
            "red",
            0.0,
            Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
        ),
        (
            "green",
            1.0,
            Color {
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            },
        ),
    ] {
        let clip = Clip::new(name, start_time, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(source_track_id, clip_id)?;
        let node =
            generator_node_for_canvas(name, GeneratorNodeRequest::Solid { color }, 2, 2, 2, 2);
        let node_id = add_node(&mut project, NodeContainer::Clip(clip_id), node)?;
        project.set_output_node(NodeContainer::Clip(clip_id), Some(node_id))?;
        source_node_ids.push(node_id);
    }

    let mut instance_ids = Vec::new();
    let mut transform_ids = Vec::new();
    for (name, trim_in, x) in [("left", 0.0, -1.0), ("right", 1.0, 1.0)] {
        let mut clip = Clip::new(name, 0.0, 1.0);
        clip.trim_in = OrderedFloat(trim_in);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(parent_track_id, clip_id)?;
        let instance = Node::new_composition_instance(
            name,
            CompositionInstanceContent {
                composition_id: source_id,
            },
        );
        let instance_id = add_node(&mut project, NodeContainer::Clip(clip_id), instance)?;
        let mut transform = PluginManager::default().create_image_transform_operation_node()?;
        transform
            .set_property(
                "position".to_string(),
                Property::constant(library::plugin::transforms::vec2_value(x, 0.0)),
            )
            .map_err(anyhow::Error::msg)?;
        let transform_id = add_node(&mut project, NodeContainer::Clip(clip_id), transform)?;
        project.connect_ports(
            address(PortOwner::Node(instance_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
        )?;
        project.set_output_node(NodeContainer::Clip(clip_id), Some(transform_id))?;
        transform_ids.push(transform_id);
        if trim_in != 0.0 {
            let mut fmod = Node::new_fmod("two-second source loop");
            fmod.set_property(
                FMOD_DIVISOR_INPUT_PORT.to_string(),
                Property::expression(
                    "value".to_string(),
                    PropertyValue::Number(OrderedFloat(2.0)),
                ),
            )
            .map_err(anyhow::Error::msg)?;
            let fmod_id = add_node(&mut project, NodeContainer::Clip(clip_id), fmod)?;
            project.connect_ports(
                address(PortOwner::Clip(clip_id), TIME_PORT),
                address(PortOwner::Node(fmod_id), FMOD_X_INPUT_PORT),
            )?;
            project.connect_ports(
                address(PortOwner::Node(fmod_id), NUMBER_RESULT_OUTPUT_PORT),
                address(PortOwner::Node(instance_id), TIME_PORT),
            )?;
        }
        instance_ids.push(instance_id);
    }

    assert!(project.validate_connections().is_empty());
    let source_before = project
        .get_composition(source_id)
        .context("source Composition must exist")?
        .clone();
    let frame = evaluate_frame(&project)?;
    assert_eq!(object_source_ids(&frame.items), source_node_ids);
    assert_eq!(
        find_group(&frame.items, instance_ids[0])
            .context("first Composition Instance must render")?
            .effect_time,
        OrderedFloat(0.0)
    );
    assert_eq!(
        find_group(&frame.items, instance_ids[1])
            .context("second Composition Instance must render")?
            .effect_time,
        OrderedFloat(1.0)
    );
    for instance_id in &instance_ids {
        assert_eq!(
            find_group(&frame.items, *instance_id)
                .context("Composition Instance group must render")?
                .transform,
            Default::default(),
            "Composition Instance sources must stay spatially neutral"
        );
    }
    for (transform_id, expected_x) in transform_ids.iter().zip([-1.0, 1.0]) {
        assert_eq!(
            find_group(&frame.items, *transform_id)
                .context("Image Transform group must render")?
                .transform
                .position
                .x,
            expected_x
        );
    }

    let image = render(&project)?;
    assert!(
        image.data[0] > 230 && image.data[1] < 10 && image.data[2] < 10 && image.data[3] > 230,
        "the translated first instance must render the red source at the left edge"
    );
    let right_offset = ((image.width - 2) * 4) as usize;
    assert!(
        image.data[right_offset] < 10
            && image.data[right_offset + 1] > 230
            && image.data[right_offset + 2] < 10
            && image.data[right_offset + 3] > 230,
        "the translated second instance must render the green source at the right edge"
    );
    assert_eq!(project.get_composition(source_id), Some(&source_before));
    assert_eq!(
        project.get_composition(parent_id).map(|item| item.id),
        Some(parent_id)
    );
    Ok(())
}
