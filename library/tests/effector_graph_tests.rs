#[path = "effector_graph_tests/graph_support.rs"]
mod graph_support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use library::animation::EasingFunction;
use library::cache::CacheManager;
use library::core::ensemble::effectors::OpacityMode;
use library::core::ensemble::target::EffectorTarget;
use library::core::ensemble::types::EffectorConfig;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::frame::runtime_shape::{
    RuntimeShapeGeometry, evaluate_text_element_transforms,
};
use library::model::project::{
    Composition, EvalOutput, MERGE_IMAGES_PORT, NodeContainer, NodeGraphBundle, PortAddress,
    PortDataType, PortDefinition, PortDirection, PortExposure, PortMultiplicity, PortOwner,
    PortSide, Project, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::property::{
    Keyframe, Property, PropertyDefinition, PropertyMap, PropertyValue, Vec2,
};
use library::model::{BlendMode, Clip, Node, NodeContent};
use library::plugin::{
    EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, EffectorPlugin, FrameEvaluationContext,
    OperationDescriptor, OperationDescriptorError, Plugin, PluginManager, ResolvedNodeInputs,
    property_port_key, property_ui_type_to_port_data_type,
};
use library::rendering::renderer::{Affine2D, RenderOutput};
use library::{RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use graph_support::{insert_effector_chain, root_transform_id};

const WIDTH: u64 = 128;
const HEIGHT: u64 = 80;
const FPS: f64 = 10.0;

fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    assert!(
        node.set_property(key.to_string(), Property::constant(value))
            .is_ok(),
        "operation descriptor must initialize {key}"
    );
}

fn setup_project() -> (Project, Uuid, Uuid) {
    let mut project = Project::new("effector graph");
    let (mut composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 10.0);
    composition.background_color = Color::black();
    let composition_id = composition.id;
    let track_id = track.id;
    project
        .add_track(track)
        .expect("container structural Merge insertion must succeed");
    project
        .add_composition(composition)
        .expect("container structural Merge insertion must succeed");
    (project, composition_id, track_id)
}

fn project_with_graph(
    graph: NodeGraphBundle,
    start_time: f64,
    duration: f64,
) -> AnyResult<(Project, Uuid)> {
    let (mut project, _composition_id, track_id) = setup_project();
    let clip = Clip::new("effector clip", start_time, duration);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project
        .insert_node_graph(NodeContainer::Clip(clip_id), graph)
        .context("insert Effector graph into Clip")?;
    Ok((project, clip_id))
}

fn evaluate_result(
    project: &Project,
    plugins: &Arc<PluginManager>,
    frame_number: u64,
) -> Result<FrameInfo, library::LibraryError> {
    get_frame_from_project(
        project,
        0,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )
}

fn evaluate(
    project: &Project,
    plugins: &Arc<PluginManager>,
    frame_number: u64,
) -> AnyResult<FrameInfo> {
    evaluate_result(project, plugins, frame_number).context("evaluate Effector graph frame")
}

fn preview(project: &Project, plugins: &Arc<PluginManager>, frame_number: u64) -> AnyResult<Image> {
    let frame = evaluate(project, plugins, frame_number)?;
    render_frame(&frame, plugins)
}

fn render_frame(frame: &FrameInfo, plugins: &Arc<PluginManager>) -> AnyResult<Image> {
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )
    .context("create CPU renderer")?;
    let mut service = RenderService::new(renderer, plugins.clone(), Arc::new(CacheManager::new()));
    match service.render_from_frame_info(frame)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Texture(_) => bail!("CPU renderer unexpectedly returned a texture"),
    }
}

fn first_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_content(&group.items),
    })
}

fn first_object(items: &[FrameItem]) -> Option<&library::model::frame::entity::FrameObject> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(object),
        FrameItem::Group(group) => first_object(&group.items),
    })
}

fn group_effect_time(items: &[FrameItem], source_id: Uuid) -> Option<f64> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(_) => None,
        FrameItem::Group(group) => (group.source_id == source_id)
            .then(|| group.effect_time.into_inner())
            .or_else(|| group_effect_time(&group.items, source_id)),
    })
}

fn collect_projected_bounds(
    items: &[FrameItem],
    parent: Affine2D,
    bounds: &mut Option<(f64, f64, f64, f64)>,
) -> AnyResult<()> {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                let local = object.content_bounds.with_context(|| {
                    format!(
                        "rendered object {} omitted Preview bounds",
                        object.source_node_id
                    )
                })?;
                let transform = parent.compose(Affine2D::from(object.content.transform()));
                let (x, y, width, height) = local.as_tuple();
                for (local_x, local_y) in [
                    (x, y),
                    (x + width, y),
                    (x + width, y + height),
                    (x, y + height),
                ] {
                    let (mapped_x, mapped_y) =
                        transform.map_point(f64::from(local_x), f64::from(local_y));
                    *bounds = Some(bounds.map_or(
                        (mapped_x, mapped_y, mapped_x, mapped_y),
                        |(left, top, right, bottom)| {
                            (
                                left.min(mapped_x),
                                top.min(mapped_y),
                                right.max(mapped_x),
                                bottom.max(mapped_y),
                            )
                        },
                    ));
                }
            }
            FrameItem::Group(group) => collect_projected_bounds(
                &group.items,
                parent.compose(Affine2D::from(&group.transform)),
                bounds,
            )?,
        }
    }
    Ok(())
}

fn alpha_bounds(image: &Image) -> Option<(f64, f64, f64, f64)> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for (index, pixel) in image.data.chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let x = (index % image.width as usize) as f64;
        let y = (index / image.width as usize) as f64;
        bounds = Some(bounds.map_or((x, y, x + 1.0, y + 1.0), |current| {
            (
                current.0.min(x),
                current.1.min(y),
                current.2.max(x + 1.0),
                current.3.max(y + 1.0),
            )
        }));
    }
    bounds
}

fn assert_alpha_inside_preview_bounds(frame: &FrameInfo, image: &Image) -> AnyResult<()> {
    let mut preview = None;
    collect_projected_bounds(&frame.items, Affine2D::IDENTITY, &mut preview)?;
    let preview = preview.context("frame must expose evaluated Preview bounds")?;
    let alpha = alpha_bounds(image).context("fixture must render non-transparent pixels")?;
    assert!(
        alpha.0 >= preview.0 && alpha.1 >= preview.1,
        "alpha starts outside Preview bounds: alpha={alpha:?}, preview={preview:?}"
    );
    assert!(
        alpha.2 <= preview.2 && alpha.3 <= preview.3,
        "alpha ends outside Preview bounds: alpha={alpha:?}, preview={preview:?}"
    );
    assert!(
        preview.2 - preview.0 < frame.width as f64 && preview.3 - preview.1 < frame.height as f64,
        "regression must not pass through a full-composition fallback: {preview:?}"
    );
    Ok(())
}

fn assert_clean_straight_rgba(image: &Image) {
    let mut visible = 0_usize;
    let mut straight_partial = false;
    for pixel in image.data.chunks_exact(4) {
        if pixel[3] == 0 {
            assert_eq!(pixel, &[0, 0, 0, 0], "transparent RGB carried color");
            continue;
        }
        visible += 1;
        if pixel[3] < 240
            && pixel[..3]
                .iter()
                .any(|channel| u16::from(*channel) > u16::from(pixel[3]) + 24)
        {
            straight_partial = true;
        }
    }
    assert!(visible > 0, "the explicit graph rendered no visible pixels");
    assert!(
        straight_partial,
        "partially transparent colors appear premultiplied instead of straight RGBA"
    );
}

#[test]
fn descriptors_factories_and_text_shape_consumers_have_complete_typed_contracts() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let mut available = plugins.get_available_effectors();
    available.sort();
    assert_eq!(
        available,
        ["opacity", "randomize", "step_delay", "transform"]
    );

    for component_id in available {
        let descriptor = plugins
            .operation_descriptor(EFFECTOR_CATEGORY, &component_id, EFFECTOR_APPLY_OPERATION)
            .with_context(|| format!("missing descriptor for Effector component {component_id}"))?;
        let node = plugins
            .create_effector_operation_node(&component_id)
            .with_context(|| format!("create Effector operation {component_id}"))?;
        let NodeContent::PluginOperation(operation) = node.content() else {
            bail!("Effector factory must create a plugin operation");
        };
        assert_eq!(operation.category, EFFECTOR_CATEGORY);
        assert_eq!(operation.component_id, component_id);
        assert_eq!(operation.operation, EFFECTOR_APPLY_OPERATION);
        assert_eq!(operation.declared_ports, descriptor.declared_ports());
        for definition in descriptor.properties() {
            assert_eq!(
                node.properties()
                    .get(definition.name())
                    .and_then(Property::value),
                Some(definition.default_value())
            );
            let port = operation
                .declared_ports
                .iter()
                .find(|port| port.key == property_port_key(definition.name()))
                .with_context(|| format!("property {} has no port", definition.name()))?;
            assert_eq!(port.direction, PortDirection::Input);
            assert_eq!(
                port.data_type,
                property_ui_type_to_port_data_type(definition.ui_type())
            );
        }
        let input = operation
            .declared_ports
            .iter()
            .find(|port| port.key == SHAPE_INPUT_PORT)
            .context("Effector operation has no Shape input")?;
        assert_eq!(input.direction, PortDirection::Input);
        assert_eq!(input.data_type, PortDataType::Shape);
        assert_eq!(input.multiplicity, PortMultiplicity::Single);
        let output = operation
            .declared_ports
            .iter()
            .find(|port| port.key == SHAPE_OUTPUT_PORT)
            .context("Effector operation has no Shape output")?;
        assert_eq!(output.direction, PortDirection::Output);
        assert_eq!(output.side, PortSide::Right);
        assert_eq!(output.data_type, PortDataType::Shape);
    }

    let transform = plugins.create_effector_operation_node("transform")?;
    for key in ["tx", "ty", "scale_x", "scale_y", "rotation", "target"] {
        assert!(transform.properties().get(key).is_some(), "missing {key}");
    }
    let opacity = plugins.create_effector_operation_node("opacity")?;
    for key in ["opacity", "mode", "target"] {
        assert!(opacity.properties().get(key).is_some(), "missing {key}");
    }

    let manager = ProjectManager::new(Arc::new(RwLock::new(Project::new("factory"))), plugins);
    let text = manager
        .create_text_node("typed", "Arial", WIDTH, HEIGHT)
        .context("create Text source")?;
    let shape = manager
        .create_shape_node("M0 0 L10 0 L10 10 Z", WIDTH, HEIGHT, 10, 10)
        .context("create Shape source")?;
    let (mut project, composition_id, _) = setup_project();
    let text_id = text.id;
    let shape_id = shape.id;
    project.add_node(text);
    project.add_node(shape);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), text_id)
        .context("attach Text source to Composition")?;
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), shape_id)
        .context("attach Shape source to Composition")?;
    for source in [text_id, shape_id] {
        let output = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(source), SHAPE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .context("source has no Shape output port")?;
        assert_eq!(output.data_type, PortDataType::Shape);
        assert_eq!(output.multiplicity, PortMultiplicity::Single);
        assert_eq!(output.side, PortSide::Right);
    }
    Ok(())
}

#[test]
fn graph_order_keyframes_and_scalar_overrides_produce_one_ensemble_and_roundtrip() -> AnyResult<()>
{
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("ORDER", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let mut transform = plugins.create_effector_operation_node("transform")?;
    transform
        .set_property(
            "tx".into(),
            Property::keyframe(vec![
                Keyframe::new(0.0, 0.0.into(), EasingFunction::Linear),
                Keyframe::new(1.0, 20.0.into(), EasingFunction::Linear),
            ]),
        )
        .map_err(|error| anyhow!("Transform descriptor must initialize tx: {error}"))?;
    set_constant(
        &mut transform,
        "target",
        PropertyValue::String("Char".into()),
    );
    let opacity = plugins.create_effector_operation_node("opacity")?;
    let transform_id = transform.id;
    let opacity_id = opacity.id;
    graph.nodes.extend([transform, opacity]);
    insert_effector_chain(&mut graph, &[transform_id, opacity_id])?;
    let (mut project, clip_id) = project_with_graph(graph, 0.0, 2.0)?;
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(opacity_id), property_port_key("opacity")),
        )
        .context("connect Clip time to Opacity")?;

    let rendered = evaluate(&project, &plugins, 5)?;
    let FrameContent::Text {
        ensemble: Some(ensemble),
        ..
    } = first_content(&rendered.items).context("rendered Text content is missing")?
    else {
        bail!("wired Effectors must produce EnsembleData");
    };
    assert_eq!(ensemble.effector_configs.len(), 2);
    assert!(matches!(
        &ensemble.effector_configs[0],
        EffectorConfig::Transform {
            translate,
            target: EffectorTarget::Char,
            ..
        } if (translate.0 - 10.0).abs() < f32::EPSILON
    ));
    assert!(matches!(
        &ensemble.effector_configs[1],
        EffectorConfig::Opacity {
            target_opacity,
            mode: OpacityMode::Set,
            target: EffectorTarget::Block,
        } if (target_opacity - 0.5).abs() < f32::EPSILON
    ));

    let saved = project.save()?;
    assert!(!saved.contains("schema_version"));
    let loaded = Project::load(&saved)?;
    assert_eq!(loaded, project);
    assert!(loaded.validation_issues().is_empty());
    assert_eq!(
        first_content(&evaluate(&loaded, &plugins, 5)?.items),
        first_content(&rendered.items)
    );
    Ok(())
}

#[test]
fn missing_invalid_unknown_and_scalar_no_output_never_restore_embedded_effectors() -> AnyResult<()>
{
    let plugins = Arc::new(PluginManager::default());
    let opacity = plugins.create_effector_operation_node("opacity")?;
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    let project = Project::new("validation");
    let evaluators = plugins.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    assert_eq!(
        plugins.evaluate_effector_operation(
            &context,
            "opacity",
            opacity.id,
            &PropertyMap::new(),
            0.0
        ),
        EvalOutput::NoOutput
    );

    let mut invalid_mode = opacity.properties().clone();
    invalid_mode.set(
        "mode".into(),
        Property::keyframe(vec![Keyframe::new(
            0.0,
            PropertyValue::String("outside-options".into()),
            EasingFunction::Linear,
        )]),
    );
    assert_eq!(
        plugins.evaluate_effector_operation(&context, "opacity", opacity.id, &invalid_mode, 0.0),
        EvalOutput::NoOutput
    );

    let mut scalar = ResolvedNodeInputs::default();
    scalar
        .properties
        .insert("opacity".into(), EvalOutput::NoOutput);
    let scalar_context = FrameEvaluationContext {
        resolved_inputs: Some(&scalar),
        ..context
    };
    assert_eq!(
        plugins.evaluate_effector_operation(
            &scalar_context,
            "opacity",
            opacity.id,
            opacity.properties(),
            0.0
        ),
        EvalOutput::NoOutput
    );

    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("unknown", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let unknown = plugins.create_effector_operation_node("opacity")?;
    let unknown_id = unknown.id;
    let mut persisted = serde_json::to_value(unknown)?;
    persisted["content"]["data"]["component_id"] =
        serde_json::Value::String("unavailable-effector".into());
    let unknown: Node = serde_json::from_value(persisted)?;
    graph.nodes.push(unknown);
    insert_effector_chain(&mut graph, &[unknown_id])?;
    let (project, _) = project_with_graph(graph, 0.0, 2.0)?;
    let rendered = evaluate(&project, &plugins, 0)?;
    assert!(rendered.items.is_empty());
    assert_eq!(Project::load(&project.save()?)?, project);
    Ok(())
}

struct CountingEffectorPlugin {
    evaluations: Arc<AtomicUsize>,
    descriptors: Arc<AtomicUsize>,
}

impl Plugin for CountingEffectorPlugin {
    fn id(&self) -> &str {
        "counting"
    }

    fn name(&self) -> String {
        "Counting".into()
    }

    fn category(&self) -> String {
        "Test".into()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectorPlugin for CountingEffectorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        Vec::new()
    }

    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        self.descriptors.fetch_add(1, Ordering::SeqCst);
        OperationDescriptor::effector(self.id(), self.name(), self.properties())
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        _source_id: Uuid,
        _properties: &PropertyMap,
        _eval_time: f64,
    ) -> Option<EffectorConfig> {
        self.evaluations.fetch_add(1, Ordering::SeqCst);
        Some(EffectorConfig::Opacity {
            target_opacity: 100.0,
            mode: OpacityMode::Set,
            target: EffectorTarget::Block,
        })
    }
}

#[test]
fn disabled_and_inactive_effector_operations_short_circuit_before_plugin_work() -> AnyResult<()> {
    let evaluations = Arc::new(AtomicUsize::new(0));
    let descriptors = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_effector_plugin(Arc::new(CountingEffectorPlugin {
        evaluations: evaluations.clone(),
        descriptors: descriptors.clone(),
    }));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("inactive", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let mut counting = plugins.create_effector_operation_node("counting")?;
    counting.enabled = false;
    let counting_id = counting.id;
    graph.nodes.push(counting);
    insert_effector_chain(&mut graph, &[counting_id])?;
    let descriptor_baseline = descriptors.load(Ordering::SeqCst);
    let (mut project, _) = project_with_graph(graph, 0.0, 2.0)?;

    assert!(evaluate(&project, &plugins, 0)?.items.is_empty());
    assert_eq!(evaluations.load(Ordering::SeqCst), 0);
    assert_eq!(
        descriptors.load(Ordering::SeqCst),
        descriptor_baseline,
        "disabled Shape operations must not look up a plugin descriptor"
    );

    let mut persisted = serde_json::to_value(Node::new_merge("broken time"))?;
    persisted["content"] = serde_json::json!({
        "type": "PluginOperation",
        "data": {
            "category": "test",
            "component_id": "broken-time",
            "operation": "test.broken-time.v1",
            "declared_ports": [PortDefinition::output(
                "broken_time",
                "Broken Time",
                PortDataType::Number,
                PortSide::Right,
                PortExposure::Graph,
            )],
        }
    });
    let mut broken_time: Node = serde_json::from_value(persisted)?;
    broken_time.ui_position = [-400.0, -200.0];
    let broken_time_id = broken_time.id;
    let container = project
        .find_node_container(counting_id)
        .context("Counting Effector has no container")?;
    project.add_node(broken_time);
    project
        .attach_node_to_container(container, broken_time_id)
        .context("attach broken-time Node to container")?;
    let broken_connection = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(broken_time_id), "broken_time"),
            PortAddress::new(PortOwner::Node(counting_id), TIME_PORT),
        )
        .context("connect broken Time output to Counting Effector")?;
    assert!(
        evaluate(&project, &plugins, 0)?.items.is_empty(),
        "a disabled Node must not resolve its Time wire"
    );
    project
        .get_node_mut(counting_id)
        .context("Counting Effector is missing")?
        .enabled = true;
    assert!(
        evaluate(&project, &plugins, 0)?.items.is_empty(),
        "an unavailable scalar operation must propagate NoOutput when its consumer is enabled"
    );
    assert_eq!(evaluations.load(Ordering::SeqCst), 0);
    project.disconnect_connection(broken_connection);

    assert!(first_content(&evaluate(&project, &plugins, 0)?.items).is_some());
    assert_eq!(evaluations.load(Ordering::SeqCst), 1);

    let inactive_graph = {
        let mut graph = manager
            .create_text_graph("inactive", "Arial", WIDTH, HEIGHT)
            .context("create inactive Text graph")?;
        let counting = plugins.create_effector_operation_node("counting")?;
        let counting_id = counting.id;
        graph.nodes.push(counting);
        insert_effector_chain(&mut graph, &[counting_id])?;
        graph
    };
    let (inactive, _) = project_with_graph(inactive_graph, 5.0, 2.0)?;
    assert!(evaluate(&inactive, &plugins, 0)?.items.is_empty());
    assert_eq!(evaluations.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn normal_nonensemble_text_pixels_are_stable_across_project_roundtrip() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let graph = manager
        .create_text_graph("PARITY", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let (project, _) = project_with_graph(graph, 0.0, 2.0)?;
    let frame = evaluate(&project, &plugins, 0)?;
    let FrameContent::Text { ensemble, .. } =
        first_content(&frame.items).context("rendered Text content is missing")?
    else {
        bail!("plain Style graph did not render Text content");
    };
    assert!(
        ensemble.is_none(),
        "a plain Style branch must stay non-Ensemble"
    );
    let expected = preview(&project, &plugins, 0)?;
    assert!(expected.data.iter().any(|channel| *channel != 0));

    let loaded = Project::load(&project.save()?)?;
    assert_eq!(loaded, project);
    assert_eq!(preview(&loaded, &plugins, 0)?.data, expected.data);
    Ok(())
}

#[test]
fn graph_randomize_char_is_deterministic_and_seeded_by_element_identity() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("AA\nAA", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let text_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(library::model::GeneratorContent::Text)
            )
        })
        .context("Text graph has no Text source")?
        .id;
    let mut random = plugins.create_effector_operation_node("randomize")?;
    set_constant(&mut random, "seed", 7.0.into());
    set_constant(&mut random, "translate_range", 8.0.into());
    set_constant(&mut random, "rotate_range", 12.0.into());
    set_constant(&mut random, "scale_range", 0.25.into());
    set_constant(&mut random, "target", PropertyValue::String("Char".into()));
    let random_id = random.id;
    graph.nodes.push(random);
    insert_effector_chain(&mut graph, &[random_id])?;
    let (project, _) = project_with_graph(graph, 0.0, 2.0)?;

    let image_a = preview(&project, &plugins, 0)?;
    let image_b = preview(&project, &plugins, 0)?;
    assert_eq!(image_a.data, image_b.data);

    let frame = evaluate(&project, &plugins, 0)?;
    let FrameContent::Text {
        ensemble: Some(ensemble),
        ..
    } = first_content(&frame.items).context("rendered Text content is missing")?
    else {
        bail!("Randomize graph did not produce Text EnsembleData");
    };

    let evaluators = plugins.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition: project
            .compositions
            .first()
            .context("project has no Composition")?,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    let RuntimeShapeGeometry::Text(runtime_text) = plugins
        .get_entity_converter("text")
        .context("Text entity converter is missing")?
        .convert_shape(
            &context,
            project
                .get_node(text_id)
                .context("Text source is missing")?,
            0.0,
        )
        .context("Text converter produced no Shape")?
        .geometry
    else {
        bail!("Text converter did not produce runtime text geometry");
    };
    assert_eq!(runtime_text.elements.len(), 4);
    assert_ne!(
        runtime_text.elements[0].line_group_id, runtime_text.elements[2].line_group_id,
        "repeated characters on separate lines need distinct line identities"
    );
    let transforms = evaluate_text_element_transforms(&runtime_text, ensemble, 0.0)?;
    assert_eq!(
        transforms,
        evaluate_text_element_transforms(&runtime_text, ensemble, 0.0)?,
        "the same seed and element identities must reproduce exactly"
    );
    assert!(
        transforms
            .iter()
            .skip(1)
            .any(|transform| { transforms.first().is_some_and(|first| transform != first) }),
        "all character identities reused one seeded transform"
    );
    let loaded = Project::load(&project.save()?)?;
    assert_eq!(image_a.data, preview(&loaded, &plugins, 0)?.data);

    let mut changed_seed = project;
    set_constant(
        changed_seed
            .get_node_mut(random_id)
            .context("Randomize operation is missing")?,
        "seed",
        8.0.into(),
    );
    assert_ne!(image_a.data, preview(&changed_seed, &plugins, 0)?.data);
    Ok(())
}

#[test]
fn explicit_shape_effector_decorator_style_merge_keeps_straight_alpha_and_bounds() -> AnyResult<()>
{
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_shape_graph("M 0 0 H 30 V 20 H 0 Z", WIDTH, HEIGHT, 30, 20)
        .context("create Shape graph")?;
    let transform_id = root_transform_id(&graph)?;
    let transform = graph
        .nodes
        .iter_mut()
        .find(|node| node.id == transform_id)
        .context("root Transform is missing")?;
    set_constant(
        transform,
        "position",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(62.0),
            y: OrderedFloat(39.0),
        }),
    );
    set_constant(
        transform,
        "anchor",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(15.0),
            y: OrderedFloat(10.0),
        }),
    );
    set_constant(
        transform,
        "scale",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(125.0),
            y: OrderedFloat(80.0),
        }),
    );
    set_constant(transform, "rotation", 21.0.into());

    for node in &mut graph.nodes {
        let NodeContent::PluginOperation(operation) = node.content() else {
            continue;
        };
        match operation.component_id.as_str() {
            "fill" => {
                set_constant(
                    node,
                    "color",
                    PropertyValue::Color(Color {
                        r: 240,
                        g: 70,
                        b: 20,
                        a: 160,
                    }),
                );
                set_constant(node, "opacity", 0.75.into());
            }
            "stroke" => {
                set_constant(
                    node,
                    "color",
                    PropertyValue::Color(Color {
                        r: 20,
                        g: 80,
                        b: 245,
                        a: 176,
                    }),
                );
                set_constant(node, "opacity", 0.8.into());
            }
            _ => {}
        }
    }

    let mut opacity = plugins.create_effector_operation_node("opacity")?;
    set_constant(&mut opacity, "opacity", 65.0.into());
    set_constant(&mut opacity, "mode", PropertyValue::String("Set".into()));
    let mut backplate = plugins
        .create_decorator_operation_node("backplate")
        .context("create Backplate operation")?;
    set_constant(&mut backplate, "padding", 4.0.into());
    set_constant(
        &mut backplate,
        "color",
        PropertyValue::Color(Color {
            r: 35,
            g: 210,
            b: 85,
            a: 96,
        }),
    );
    let chain = [opacity.id, backplate.id];
    graph.nodes.extend([opacity, backplate]);
    insert_effector_chain(&mut graph, &chain)?;
    let merge_wire = graph
        .connections
        .iter_mut()
        .find(|connection| connection.to.port == MERGE_IMAGES_PORT && connection.order == 1)
        .context("Shape factory must merge its Fill and Stroke branches")?;
    merge_wire.blend_mode = BlendMode::Screen;

    let (mut project, _) = project_with_graph(graph, 0.0, 2.0)?;
    project
        .compositions
        .first_mut()
        .context("project has no Composition")?
        .background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let frame = evaluate(&project, &plugins, 0)?;
    let rendered = render_frame(&frame, &plugins)?;
    assert_clean_straight_rgba(&rendered);
    assert_alpha_inside_preview_bounds(&frame, &rendered)?;

    let loaded = Project::load(&project.save()?)?;
    assert_eq!(rendered.data, preview(&loaded, &plugins, 0)?.data);
    Ok(())
}

#[test]
fn preview_bounds_contain_ensemble_text_and_path_backplate_alpha() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );

    let mut text_graph = manager
        .create_text_graph("AB\nCD", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let text_id = text_graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(library::model::GeneratorContent::Text)
            )
        })
        .context("Text graph has no Text source")?
        .id;
    let text_transform_id = root_transform_id(&text_graph)?;
    set_constant(
        text_graph
            .nodes
            .iter_mut()
            .find(|node| node.id == text_id)
            .context("Text source is missing")?,
        "size",
        18.0.into(),
    );
    let text_transform = text_graph
        .nodes
        .iter_mut()
        .find(|node| node.id == text_transform_id)
        .context("Text root Transform is missing")?;
    set_constant(
        text_transform,
        "position",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(34.0),
            y: OrderedFloat(12.0),
        }),
    );
    set_constant(
        text_transform,
        "anchor",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(0.0),
            y: OrderedFloat(0.0),
        }),
    );

    let mut line_transform = plugins.create_effector_operation_node("transform")?;
    set_constant(&mut line_transform, "tx", 6.0.into());
    set_constant(&mut line_transform, "ty", 3.0.into());
    set_constant(&mut line_transform, "rotation", 12.0.into());
    set_constant(&mut line_transform, "scale_x", 1.1.into());
    set_constant(&mut line_transform, "scale_y", 0.9.into());
    set_constant(
        &mut line_transform,
        "target",
        PropertyValue::String("Line".into()),
    );
    let mut char_random = plugins.create_effector_operation_node("randomize")?;
    set_constant(&mut char_random, "seed", 17.0.into());
    set_constant(&mut char_random, "translate_range", 5.0.into());
    set_constant(&mut char_random, "rotate_range", 8.0.into());
    set_constant(&mut char_random, "scale_range", 0.15.into());
    set_constant(
        &mut char_random,
        "target",
        PropertyValue::String("Char".into()),
    );
    let mut char_backplate = plugins
        .create_decorator_operation_node("backplate")
        .context("create Backplate operation")?;
    set_constant(
        &mut char_backplate,
        "target",
        PropertyValue::String("Char".into()),
    );
    set_constant(&mut char_backplate, "padding", 5.0.into());
    set_constant(
        &mut char_backplate,
        "color",
        PropertyValue::Color(Color::white()),
    );
    let char_backplate_id = char_backplate.id;
    let text_chain = [line_transform.id, char_random.id, char_backplate.id];
    text_graph
        .nodes
        .extend([line_transform, char_random, char_backplate]);
    insert_effector_chain(&mut text_graph, &text_chain)?;
    let (mut text_project, _) = project_with_graph(text_graph, 0.0, 2.0)?;
    text_project
        .compositions
        .first_mut()
        .context("Text project has no Composition")?
        .background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let text_frame = evaluate(&text_project, &plugins, 0)?;
    assert_eq!(
        first_object(&text_frame.items)
            .context("Text frame has no object")?
            .source_node_id,
        text_id
    );
    assert_eq!(
        first_object(&text_frame.items)
            .context("Text frame has no object")?
            .spatial_transform_node_id,
        Some(text_transform_id)
    );
    let text_image = render_frame(&text_frame, &plugins)?;
    assert_alpha_inside_preview_bounds(&text_frame, &text_image)?;
    for target in ["Line", "Block"] {
        set_constant(
            text_project
                .get_node_mut(char_backplate_id)
                .context("Text Backplate operation is missing")?,
            "target",
            PropertyValue::String(target.into()),
        );
        let frame = evaluate(&text_project, &plugins, 0)?;
        let image = render_frame(&frame, &plugins)?;
        assert_alpha_inside_preview_bounds(&frame, &image)?;
    }

    let mut path_graph = manager
        .create_shape_graph("M 0 0 H 30 V 20 H 0 Z", WIDTH, HEIGHT, 30, 20)
        .context("create Shape graph")?;
    let path_transform_id = root_transform_id(&path_graph)?;
    let path_transform = path_graph
        .nodes
        .iter_mut()
        .find(|node| node.id == path_transform_id)
        .context("Shape root Transform is missing")?;
    set_constant(path_transform, "rotation", 23.0.into());
    set_constant(
        path_transform,
        "scale",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(115.0),
            y: OrderedFloat(90.0),
        }),
    );
    let mut path_backplate = plugins
        .create_decorator_operation_node("backplate")
        .context("create Path Backplate operation")?;
    set_constant(&mut path_backplate, "padding", 11.0.into());
    set_constant(
        &mut path_backplate,
        "color",
        PropertyValue::Color(Color::white()),
    );
    let path_backplate_id = path_backplate.id;
    path_graph.nodes.push(path_backplate);
    insert_effector_chain(&mut path_graph, &[path_backplate_id])?;
    let (mut path_project, _) = project_with_graph(path_graph, 0.0, 2.0)?;
    path_project
        .compositions
        .first_mut()
        .context("Path project has no Composition")?
        .background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let path_frame = evaluate(&path_project, &plugins, 0)?;
    let path_object = first_object(&path_frame.items).context("Path frame has no object")?;
    let path_bounds = path_object
        .content_bounds
        .context("Path object has no Preview bounds")?;
    assert!(path_bounds.width.into_inner() >= 52.0);
    assert!(path_bounds.height.into_inner() >= 42.0);
    let path_image = render_frame(&path_frame, &plugins)?;
    assert_alpha_inside_preview_bounds(&path_frame, &path_image)?;
    Ok(())
}

#[test]
fn style_local_scope_time_drives_ensemble_bounds_and_pixels() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("ABCD", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let source_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(library::model::GeneratorContent::Text)
            )
        })
        .context("Text graph has no Text source")?
        .id;
    let transform_id = root_transform_id(&graph)?;
    let style_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation) if operation.category == "style"
            )
        })
        .context("Text graph has no Style operation")?
        .id;
    set_constant(
        graph
            .nodes
            .iter_mut()
            .find(|node| node.id == source_id)
            .context("Text source is missing")?,
        "size",
        18.0.into(),
    );
    let transform = graph
        .nodes
        .iter_mut()
        .find(|node| node.id == transform_id)
        .context("Text root Transform is missing")?;
    set_constant(
        transform,
        "position",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(20.0),
            y: OrderedFloat(12.0),
        }),
    );
    set_constant(
        transform,
        "anchor",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(0.0),
            y: OrderedFloat(0.0),
        }),
    );
    let mut delay = plugins
        .create_effector_operation_node("step_delay")
        .context("create StepDelay operation")?;
    set_constant(&mut delay, "delay", 0.5.into());
    set_constant(&mut delay, "duration", 0.0.into());
    set_constant(&mut delay, "from_opacity", 0.0.into());
    set_constant(&mut delay, "to_opacity", 100.0.into());
    set_constant(&mut delay, "target", PropertyValue::String("Block".into()));
    let delay_id = delay.id;
    graph.nodes.push(delay);
    insert_effector_chain(&mut graph, &[delay_id])?;

    let (mut local_project, _) = project_with_graph(graph.clone(), 2.0, 4.0)?;
    let (mut global_project, _) = project_with_graph(graph, 0.0, 4.0)?;
    for project in [&mut local_project, &mut global_project] {
        project
            .compositions
            .first_mut()
            .context("project has no Composition")?
            .background_color = Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
    }

    let local_frame = evaluate(&local_project, &plugins, 21)?;
    let global_frame = evaluate(&global_project, &plugins, 21)?;
    let local_time = group_effect_time(&local_frame.items, style_id)
        .context("local Style effect time is missing")?;
    let global_time = group_effect_time(&global_frame.items, style_id)
        .context("global Style effect time is missing")?;
    assert!((local_time - 0.1).abs() < 1e-9);
    assert!((global_time - 2.1).abs() < 1e-9);

    let local_bounds = first_object(&local_frame.items)
        .context("local frame has no object")?
        .content_bounds
        .context("local object has no Preview bounds")?;
    let global_bounds = first_object(&global_frame.items)
        .context("global frame has no object")?
        .content_bounds
        .context("global object has no Preview bounds")?;
    assert!(
        local_bounds.width < global_bounds.width,
        "bounds must evaluate StepDelay at Style-local time, not global time"
    );
    let local_image = render_frame(&local_frame, &plugins)?;
    let global_image = render_frame(&global_frame, &plugins)?;
    assert_alpha_inside_preview_bounds(&local_frame, &local_image)?;
    assert_alpha_inside_preview_bounds(&global_frame, &global_image)?;
    assert_ne!(local_image.data, global_image.data);
    Ok(())
}

#[test]
fn shape_variadic_effector_input_applies_single_element_transform() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_shape_graph("M0 0 L20 0 L20 20 L0 20 Z", WIDTH, HEIGHT, 20, 20)
        .context("create Shape graph")?;
    let shape_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(library::model::GeneratorContent::Shape)
            )
        })
        .context("Shape graph has no Shape source")?
        .id;
    let root_transform_id = root_transform_id(&graph)?;
    let mut modulation = plugins.create_effector_operation_node("transform")?;
    set_constant(&mut modulation, "tx", 8.0.into());
    set_constant(&mut modulation, "ty", 3.0.into());
    let modulation_id = modulation.id;
    let mut opacity = plugins.create_effector_operation_node("opacity")?;
    set_constant(&mut opacity, "opacity", 50.0.into());
    let opacity_id = opacity.id;
    graph.nodes.extend([modulation, opacity]);
    insert_effector_chain(&mut graph, &[modulation_id, opacity_id])?;
    let (mut project, _) = project_with_graph(graph, 0.0, 2.0)?;

    let rendered = evaluate(&project, &plugins, 0)?;
    let object = first_object(&rendered.items).context("Shape frame has no object")?;
    let FrameContent::Shape { transform, .. } = &object.content else {
        bail!("Shape graph did not render Shape content");
    };
    assert_eq!(object.source_node_id, shape_id);
    assert_eq!(object.spatial_transform_node_id, Some(root_transform_id));
    assert_eq!(
        (
            object.spatial_transform.position.x,
            object.spatial_transform.position.y
        ),
        (64.0, 40.0),
        "Preview edits the root Transform, not the downstream Transform Modulation"
    );
    assert_eq!((transform.position.x, transform.position.y), (72.0, 43.0));
    assert!((transform.opacity - 0.5).abs() < f64::EPSILON);
    let before = preview(&project, &plugins, 0)?;
    set_constant(
        project
            .get_node_mut(root_transform_id)
            .context("Shape root Transform is missing")?,
        "position",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(70.0),
            y: OrderedFloat(44.0),
        }),
    );
    let moved = evaluate(&project, &plugins, 0)?;
    let moved_object = first_object(&moved.items).context("moved frame has no object")?;
    assert_eq!(
        (
            moved_object.spatial_transform.position.x,
            moved_object.spatial_transform.position.y
        ),
        (70.0, 44.0)
    );
    assert_eq!(
        (
            moved_object.content.transform().position.x,
            moved_object.content.transform().position.y
        ),
        (78.0, 47.0),
        "the unchanged Transform Modulation remains composed after the root Transform edit"
    );
    assert_ne!(
        before.data,
        preview(&project, &plugins, 0)?.data,
        "editing the root Transform must change real rendered pixels"
    );
    Ok(())
}
