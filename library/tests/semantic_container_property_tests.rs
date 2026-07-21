mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use library::editor::project_service::{
    GeneratorNodeRequest, ProjectManager, SemanticAnimationSupport, SemanticPropertyAccess,
    SemanticPropertyGroup, SemanticPropertyOwner,
};
use library::framing::get_frame_from_project;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem};
use library::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT,
    NodeContainer, NodeGraphBundle, PortAddress, PortOwner, ProjectConnection, SHAPE_INPUT_PORT,
    SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue, Vec2};
use library::model::{Clip, Composition, Node, NodeContent, Project};
use library::plugin::{
    IMAGE_OPACITY_STYLE_COMPONENT_ID, IMAGE_TRANSFORM_COMPONENT_ID, PluginManager,
    SHAPE_TRANSFORM_COMPONENT_ID,
};
use ordered_float::OrderedFloat;
use uuid::Uuid;

fn read_project(project: &RwLock<Project>) -> Result<std::sync::RwLockReadGuard<'_, Project>> {
    project
        .read()
        .map_err(|error| anyhow::anyhow!("Project read lock poisoned: {error}"))
}

fn write_project(project: &RwLock<Project>) -> Result<std::sync::RwLockWriteGuard<'_, Project>> {
    project
        .write()
        .map_err(|error| anyhow::anyhow!("Project write lock poisoned: {error}"))
}

fn vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn set_legacy_transform(project: &mut Project, owner: NodeContainer) -> Result<()> {
    let properties = match owner {
        NodeContainer::Composition(id) => {
            &mut project
                .get_composition_mut(id)
                .context("composition exists")?
                .properties
        }
        NodeContainer::Track(id) => {
            &mut project
                .get_track_mut(id)
                .context("track exists")?
                .properties
        }
        NodeContainer::Clip(id) => &mut project.get_clip_mut(id).context("clip exists")?.properties,
    };
    for (key, value) in [
        ("position", vec2(18.0, 12.0)),
        ("rotation", PropertyValue::from(7.0)),
        ("scale", vec2(125.0, 80.0)),
        ("anchor", vec2(4.0, 3.0)),
        ("opacity", PropertyValue::from(50.0)),
    ] {
        properties.set(key.to_string(), Property::constant(value));
    }
    Ok(())
}

fn raster_clip_fixture() -> Result<(Arc<RwLock<Project>>, ProjectManager, Uuid, Uuid)> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("detached factory"))),
        Arc::clone(&plugins),
    );
    let source = factory.create_generator_node(
        GeneratorNodeRequest::Solid {
            color: Color {
                r: 220,
                g: 70,
                b: 30,
                a: 255,
            },
        },
        160,
        90,
        160,
        90,
    )?;
    let source_id = source.id;
    let mut project = Project::new("semantic raster clip");
    let (composition, track) = Composition::new("main", 160, 90, 30.0, 2.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("solid", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(vec![source], Vec::new(), Some(source_id)),
    )?;
    set_legacy_transform(&mut project, NodeContainer::Clip(clip_id))?;
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);
    Ok((shared, manager, clip_id, source_id))
}

fn operation_nodes(project: &Project, container: NodeContainer, component: &str) -> Vec<Uuid> {
    let ids = match container {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.node_ids.as_slice()),
        NodeContainer::Track(id) => project.get_track(id).map(|track| track.node_ids.as_slice()),
        NodeContainer::Clip(id) => project.get_clip(id).map(|clip| clip.node_ids.as_slice()),
    };
    let Some(ids) = ids else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(|node_id| {
            let node = project.get_node(*node_id)?;
            let NodeContent::PluginOperation(operation) = node.content() else {
                return None;
            };
            (operation.component_id == component).then_some(*node_id)
        })
        .collect()
}

fn find_group(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(_) => None,
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
    })
}

#[test]
fn legacy_raster_clip_is_absorbed_once_into_transform_and_image_opacity() -> Result<()> {
    let (shared, manager, clip_id, _) = raster_clip_fixture()?;
    let before_connections = read_project(&shared)?
        .connections
        .iter()
        .map(|connection| {
            (
                connection.id,
                (
                    connection.from.clone(),
                    connection.to.clone(),
                    connection.order,
                    connection.blend_mode,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();

    manager.update_semantic_container_property_or_keyframe(
        NodeContainer::Clip(clip_id),
        "position",
        0.0,
        vec2(30.0, 40.0),
        None,
    )?;
    let first = read_project(&shared)?.clone();
    let transforms = operation_nodes(
        &first,
        NodeContainer::Clip(clip_id),
        IMAGE_TRANSFORM_COMPONENT_ID,
    );
    let opacities = operation_nodes(
        &first,
        NodeContainer::Clip(clip_id),
        IMAGE_OPACITY_STYLE_COMPONENT_ID,
    );
    assert_eq!(transforms.len(), 1);
    assert_eq!(opacities.len(), 1);
    let transform_id = *transforms.first().context("one Image Transform")?;
    let opacity_id = *opacities.first().context("one Image Opacity")?;
    let edited_clip = first.get_clip(clip_id).context("edited Clip exists")?;
    assert_eq!(edited_clip.output_node_id, Some(opacity_id));
    for key in ["position", "rotation", "scale", "anchor", "opacity"] {
        assert!(edited_clip.properties.get(key).is_none());
    }
    for (connection_id, expected) in before_connections {
        let connection = first
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .with_context(|| format!("pre-existing connection {connection_id} was deleted"))?;
        assert_eq!(
            (
                connection.from.clone(),
                connection.to.clone(),
                connection.order,
                connection.blend_mode,
            ),
            expected
        );
    }
    assert_eq!(
        first
            .get_node(transform_id)
            .context("semantic Image Transform exists")?
            .properties()
            .get("rotation")
            .and_then(Property::value),
        Some(&PropertyValue::from(7.0))
    );
    assert_eq!(
        first
            .get_node(opacity_id)
            .context("semantic Image Opacity exists")?
            .properties()
            .get("opacity")
            .and_then(Property::value),
        Some(&PropertyValue::from(0.5))
    );

    manager.update_semantic_container_property_or_keyframe(
        NodeContainer::Clip(clip_id),
        "position",
        0.0,
        vec2(30.0, 40.0),
        None,
    )?;
    let second = read_project(&shared)?.clone();
    assert_eq!(
        first, second,
        "idempotent edit must not synthesize more Nodes"
    );

    let plugins = manager.get_plugin_manager();
    let frame = get_frame_from_project(
        &second,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?;
    let transform = find_group(&frame.items, transform_id).context("Image Transform group")?;
    assert_eq!(transform.kind, FrameGroupKind::ImageTransform);
    assert_eq!(
        (
            transform.transform.position.x,
            transform.transform.position.y
        ),
        (30.0, 40.0)
    );
    let opacity = find_group(&frame.items, opacity_id).context("Image Style group")?;
    assert_eq!(opacity.kind, FrameGroupKind::ImageStyle);
    assert_eq!(opacity.transform.opacity, 0.5);
    Ok(())
}

#[test]
fn shape_fanout_insertion_preserves_downstream_wire_identity_and_layer_metadata() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let source = factory.create_generator_node(
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 30 V 20 Z".to_string(),
        },
        160,
        90,
        30,
        20,
    )?;
    let fill = plugins.create_style_operation_node("fill")?;
    let stroke = plugins.create_style_operation_node("stroke")?;
    let merge = Node::new_merge("result");
    let (source_id, fill_id, stroke_id, merge_id) = (source.id, fill.id, stroke.id, merge.id);
    let fill_wire = ProjectConnection::new(
        PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
        0,
    );
    let stroke_wire = ProjectConnection::new(
        PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(stroke_id), SHAPE_INPUT_PORT),
        0,
    );
    let original_shape_wires = [fill_wire.clone(), stroke_wire.clone()];
    let graph = NodeGraphBundle::new(
        vec![source, fill, stroke, merge],
        vec![
            fill_wire,
            stroke_wire,
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(fill_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(stroke_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
                1,
            ),
        ],
        Some(merge_id),
    );
    let mut project = Project::new("shape fanout");
    let (composition, track) = Composition::new("main", 160, 90, 30.0, 1.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("shape", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);

    manager.update_semantic_container_property_or_keyframe(
        NodeContainer::Clip(clip_id),
        "scale",
        0.0,
        vec2(150.0, 150.0),
        None,
    )?;
    let edited = read_project(&shared)?;
    let transforms = operation_nodes(
        &edited,
        NodeContainer::Clip(clip_id),
        SHAPE_TRANSFORM_COMPONENT_ID,
    );
    assert_eq!(transforms.len(), 1);
    let transform_id = *transforms.first().context("one Shape Transform")?;
    for original in original_shape_wires {
        let current = edited
            .connections
            .iter()
            .find(|connection| connection.id == original.id)
            .context("downstream Shape wire identity must survive")?;
        assert_eq!(current.to, original.to);
        assert_eq!(current.order, original.order);
        assert_eq!(current.blend_mode, original.blend_mode);
        assert_eq!(
            current.from,
            PortAddress::new(PortOwner::Node(transform_id), SHAPE_OUTPUT_PORT)
        );
    }
    assert!(edited.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT)
    }));
    Ok(())
}

#[test]
fn wired_or_ambiguous_semantic_authority_fails_without_partial_project_changes() -> Result<()> {
    let (shared, manager, clip_id, _) = raster_clip_fixture()?;
    manager.update_semantic_container_property_or_keyframe(
        NodeContainer::Clip(clip_id),
        "rotation",
        0.0,
        PropertyValue::from(10.0),
        None,
    )?;
    let transform_id = {
        let project = read_project(&shared)?;
        *operation_nodes(
            &project,
            NodeContainer::Clip(clip_id),
            IMAGE_TRANSFORM_COMPONENT_ID,
        )
        .first()
        .context("semantic Image Transform exists")?
    };
    {
        let mut project = write_project(&shared)?;
        let value = Node::new_add("driven rotation");
        let value_id = value.id;
        project.add_node(value);
        project.attach_node_to_container(NodeContainer::Clip(clip_id), value_id)?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(value_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(
                PortOwner::Node(transform_id),
                library::plugin::property_port_key("rotation"),
            ),
        )?;
    }
    let wired_snapshot = read_project(&shared)?.clone();
    let error = manager
        .update_semantic_container_property_or_keyframe(
            NodeContainer::Clip(clip_id),
            "rotation",
            0.0,
            PropertyValue::from(30.0),
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("wired from"));
    assert_eq!(*read_project(&shared)?, wired_snapshot);

    // A second output-reaching Transform is advanced graph state; the facade
    // must not guess which one a Timeline edit intended.
    let opacity_id = *operation_nodes(
        &wired_snapshot,
        NodeContainer::Clip(clip_id),
        IMAGE_OPACITY_STYLE_COMPONENT_ID,
    )
    .first()
    .context("semantic Image Opacity exists")?;
    {
        let mut project = write_project(&shared)?;
        let opacity_input = PortAddress::new(PortOwner::Node(opacity_id), IMAGE_INPUT_PORT);
        let original = project
            .connections
            .iter()
            .find(|connection| connection.to == opacity_input)
            .context("opacity input wire")?
            .clone();
        let second = manager
            .get_plugin_manager()
            .create_image_transform_operation_node()?;
        let second_id = second.id;
        project.add_node(second);
        project.attach_node_to_container(NodeContainer::Clip(clip_id), second_id)?;
        let downstream = project
            .connections
            .iter_mut()
            .find(|connection| connection.id == original.id)
            .context("original opacity downstream wire survives")?;
        downstream.from = PortAddress::new(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT);
        project.connections.push(ProjectConnection::new(
            original.from,
            PortAddress::new(PortOwner::Node(second_id), IMAGE_INPUT_PORT),
            0,
        ));
        assert!(project.validate_connections().is_empty());
    }
    let ambiguous_snapshot = read_project(&shared)?.clone();
    let error = manager
        .semantic_container_property_projection(NodeContainer::Clip(clip_id))
        .unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
    assert_eq!(*read_project(&shared)?, ambiguous_snapshot);
    Ok(())
}

#[test]
fn track_and_composition_semantic_edits_wrap_their_structural_output() -> Result<()> {
    let mut project = Project::new("container scopes");
    let (composition, track) = Composition::new("main", 160, 90, 30.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    for owner in [
        NodeContainer::Track(track_id),
        NodeContainer::Composition(composition_id),
    ] {
        set_legacy_transform(&mut project, owner)?;
    }
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    for owner in [
        NodeContainer::Track(track_id),
        NodeContainer::Composition(composition_id),
    ] {
        manager.update_semantic_container_property_or_keyframe(
            owner,
            "anchor",
            0.0,
            vec2(9.0, 8.0),
            None,
        )?;
        let project = read_project(&shared)?;
        assert_eq!(
            operation_nodes(&project, owner, IMAGE_TRANSFORM_COMPONENT_ID).len(),
            1
        );
        assert_eq!(
            operation_nodes(&project, owner, IMAGE_OPACITY_STYLE_COMPONENT_ID).len(),
            1
        );
    }
    Ok(())
}

#[test]
fn conflicting_legacy_and_advanced_transform_rolls_back_synthesis() -> Result<()> {
    let (shared, manager, clip_id, source_id) = raster_clip_fixture()?;
    {
        let mut project = write_project(&shared)?;
        let mut transform = manager
            .get_plugin_manager()
            .create_image_transform_operation_node()?;
        transform
            .set_property(
                "rotation".to_string(),
                Property::constant(PropertyValue::from(22.0)),
            )
            .map_err(anyhow::Error::msg)?;
        let transform_id = transform.id;
        project.insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(
                vec![transform],
                vec![ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
                    0,
                )],
                Some(transform_id),
            ),
        )?;
    }
    let before = read_project(&shared)?.clone();
    let error = manager
        .update_semantic_container_property_or_keyframe(
            NodeContainer::Clip(clip_id),
            "position",
            0.0,
            vec2(1.0, 2.0),
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("conflicts"));
    assert_eq!(*read_project(&shared)?, before);
    assert!(
        operation_nodes(
            &before,
            NodeContainer::Clip(clip_id),
            IMAGE_OPACITY_STYLE_COMPONENT_ID
        )
        .is_empty()
    );
    Ok(())
}

#[test]
fn projection_is_derived_without_mutating_or_synthesizing_nodes() -> Result<()> {
    let (shared, manager, clip_id, _) = raster_clip_fixture()?;
    let before = read_project(&shared)?.clone();
    let projection =
        manager.semantic_container_property_projection(NodeContainer::Clip(clip_id))?;
    assert_eq!(projection.owner(), NodeContainer::Clip(clip_id));
    assert_eq!(projection.definitions().len(), 5);
    assert_eq!(
        projection
            .properties()
            .get("opacity")
            .and_then(Property::value),
        Some(&PropertyValue::from(0.5))
    );
    assert!(
        projection
            .binding("position")
            .context("position semantic binding")?
            .node_id
            .is_none()
    );
    assert_eq!(*read_project(&shared)?, before);
    assert_eq!(
        before.nodes.keys().copied().collect::<BTreeSet<_>>(),
        read_project(&shared)?
            .nodes
            .keys()
            .copied()
            .collect::<BTreeSet<_>>()
    );
    Ok(())
}

#[test]
fn property_stack_projects_timing_source_and_unmaterialized_semantics_without_mutation()
-> Result<()> {
    let (shared, manager, clip_id, source_id) = raster_clip_fixture()?;
    let before = read_project(&shared)?.clone();
    let stack = manager.semantic_container_property_stack(NodeContainer::Clip(clip_id))?;

    assert_eq!(stack.owner(), NodeContainer::Clip(clip_id));
    assert!(stack.diagnostics().is_empty());
    assert_eq!(
        stack
            .sections()
            .iter()
            .map(|section| section.group())
            .collect::<Vec<_>>(),
        vec![
            SemanticPropertyGroup::Timing,
            SemanticPropertyGroup::Source,
            SemanticPropertyGroup::Transform,
            SemanticPropertyGroup::Style,
        ]
    );
    let timing = stack.sections().first().context("Timing section")?;
    assert_eq!(timing.stable_id(), "clip:timing");
    assert_eq!(timing.owner(), SemanticPropertyOwner::DirectClip(clip_id));
    assert!(
        timing
            .properties()
            .iter()
            .all(|entry| entry.animation() == SemanticAnimationSupport::ConstantOnly)
    );
    let source = stack
        .sections()
        .iter()
        .find(|section| section.node_id() == Some(source_id))
        .context("output-reaching source section")?;
    assert_eq!(source.group(), SemanticPropertyGroup::Source);
    let transform = stack
        .sections()
        .iter()
        .find(|section| section.stable_id() == "semantic:transform")
        .context("synthetic Transform projection")?;
    assert_eq!(transform.node_id(), None);
    assert_eq!(
        transform
            .properties()
            .iter()
            .find(|entry| entry.key() == "position")
            .context("position")?
            .property()
            .value(),
        Some(&vec2(18.0, 12.0))
    );
    let opacity = stack
        .sections()
        .iter()
        .find(|section| section.stable_id() == "semantic:opacity")
        .context("synthetic Opacity projection")?;
    assert_eq!(
        opacity
            .properties()
            .first()
            .context("opacity property")?
            .property()
            .value(),
        Some(&PropertyValue::from(0.5))
    );
    assert_eq!(*read_project(&shared)?, before);
    Ok(())
}

#[test]
fn property_stack_follows_typed_visual_topology_and_preserves_authored_modes() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        Arc::clone(&plugins),
    );
    let source = factory.create_generator_node(
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 30 V 20 Z".to_string(),
        },
        160,
        90,
        30,
        20,
    )?;
    let decorator = plugins.create_decorator_operation_node("backplate")?;
    let mut fill = plugins.create_style_operation_node("fill")?;
    fill.set_property(
        "opacity".to_string(),
        Property::expression("0.25 + time * 0.0".to_string(), PropertyValue::from(0.25)),
    )
    .map_err(anyhow::Error::msg)?;
    let effect = plugins.create_effect_operation_node("blur")?;
    let (source_id, decorator_id, fill_id, effect_id) =
        (source.id, decorator.id, fill.id, effect.id);
    let graph = NodeGraphBundle::new(
        vec![source, decorator, fill, effect],
        vec![
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(decorator_id), SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(decorator_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(fill_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
                0,
            ),
        ],
        Some(effect_id),
    );
    let mut project = Project::new("semantic stack topology");
    let (composition, track) = Composition::new("main", 160, 90, 30.0, 1.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("shape", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), plugins);
    manager.update_semantic_container_property_or_keyframe(
        NodeContainer::Clip(clip_id),
        "position",
        0.0,
        vec2(5.0, 6.0),
        None,
    )?;
    {
        let mut project = write_project(&shared)?;
        let value = Node::new_add("driven offset");
        let value_id = value.id;
        project.add_node(value);
        project.attach_node_to_container(NodeContainer::Clip(clip_id), value_id)?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(value_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(
                PortOwner::Node(fill_id),
                library::plugin::property_port_key("offset"),
            ),
        )?;
    }
    let before = read_project(&shared)?.clone();
    let stack = manager.semantic_container_property_stack(NodeContainer::Clip(clip_id))?;
    assert_eq!(
        stack
            .sections()
            .iter()
            .map(|section| section.group())
            .collect::<Vec<_>>(),
        vec![
            SemanticPropertyGroup::Timing,
            SemanticPropertyGroup::Source,
            SemanticPropertyGroup::Decorator,
            SemanticPropertyGroup::Transform,
            SemanticPropertyGroup::Style,
            SemanticPropertyGroup::Effect,
            SemanticPropertyGroup::Style,
        ]
    );
    let fill = stack
        .sections()
        .iter()
        .find(|section| section.node_id() == Some(fill_id))
        .context("Fill section")?;
    assert_eq!(fill.owner(), SemanticPropertyOwner::ExactNode(fill_id));
    let opacity = fill
        .properties()
        .iter()
        .find(|entry| entry.key() == "opacity")
        .context("Fill opacity")?;
    assert_eq!(opacity.property().evaluator, "expression");
    assert_eq!(
        opacity.property().expression_text(),
        Some("0.25 + time * 0.0")
    );
    let offset = fill
        .properties()
        .iter()
        .find(|entry| entry.key() == "offset")
        .context("Fill offset")?;
    assert!(matches!(
        offset.access(),
        SemanticPropertyAccess::Wired { .. }
    ));
    assert_eq!(*read_project(&shared)?, before);
    Ok(())
}

#[test]
fn branch_local_opacity_does_not_hide_the_final_semantic_trunk() -> Result<()> {
    let (shared, manager, clip_id, source_id) = raster_clip_fixture()?;
    let owner = NodeContainer::Clip(clip_id);
    manager.update_semantic_container_property_or_keyframe(
        owner,
        "position",
        0.0,
        vec2(10.0, 20.0),
        None,
    )?;
    let (global_transform, global_opacity) = {
        let project = read_project(&shared)?;
        (
            *operation_nodes(&project, owner, IMAGE_TRANSFORM_COMPONENT_ID)
                .first()
                .context("global Image Transform")?,
            *operation_nodes(&project, owner, IMAGE_OPACITY_STYLE_COMPONENT_ID)
                .first()
                .context("global Image Opacity")?,
        )
    };
    let branch_opacity = {
        let mut project = write_project(&shared)?;
        let branch = manager
            .get_plugin_manager()
            .create_image_opacity_style_operation_node()?;
        let merge = Node::new_merge("branch opacity merge");
        let (branch_id, merge_id) = (branch.id, merge.id);
        project.insert_node_graph(
            owner,
            NodeGraphBundle::new(vec![branch, merge], Vec::new(), None),
        )?;
        let transform_input = project
            .connections
            .iter_mut()
            .find(|connection| {
                connection.from == PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(global_transform), IMAGE_INPUT_PORT)
            })
            .context("source -> global Transform")?;
        transform_input.from = PortAddress::new(PortOwner::Node(merge_id), IMAGE_OUTPUT_PORT);
        project.connect_ports(
            PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(branch_id), IMAGE_INPUT_PORT),
        )?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(branch_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )?;
        assert!(project.validate_connections().is_empty());
        branch_id
    };
    let before = read_project(&shared)?.clone();

    let projection = manager.semantic_container_property_projection(owner)?;
    assert_eq!(
        projection
            .binding("opacity")
            .and_then(|binding| binding.node_id),
        Some(global_opacity),
        "only the final output-trunk Opacity owns Clip opacity"
    );
    assert_ne!(global_opacity, branch_opacity);
    assert_eq!(*read_project(&shared)?, before);
    Ok(())
}
