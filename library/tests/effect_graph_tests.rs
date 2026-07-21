use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use library::animation::EasingFunction;
use library::cache::CacheManager;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::project::{
    Composition, EvalOutput, FPS_PORT, FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT,
    MERGE_IMAGES_PORT, NodeContainer, NodeGraphBundle, PortAddress, PortDataType, PortDefinition,
    PortExposure, PortOwner, PortSide, Project, ProjectConnection, TIME_PORT,
};
use library::model::property::{Keyframe, Property, PropertyValue, Vec2};
use library::model::{Clip, Node, NodeContent};
use library::plugin::{
    EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, EffectPlugin, FrameEvaluationContext,
    OperationDescriptor, OperationDescriptorError, Plugin, PluginManager, ResolvedNodeInputs,
    property_port_key,
};
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use uuid::Uuid;

const WIDTH: u64 = 16;
const HEIGHT: u64 = 8;
const FPS: f64 = 10.0;

fn output_port(key: &str, data_type: PortDataType) -> PortDefinition {
    PortDefinition::output(
        key,
        "Output",
        data_type,
        PortSide::Right,
        PortExposure::Graph,
    )
}

fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    assert!(
        node.set_property(key.to_string(), Property::constant(value))
            .is_ok(),
        "operation descriptor must initialize {key}"
    );
}

fn vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn image_wire(from: Uuid, to: Uuid) -> ProjectConnection {
    ProjectConnection::new(
        PortAddress::new(PortOwner::Node(from), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(to), IMAGE_INPUT_PORT),
        0,
    )
}

fn setup_project() -> (Project, Uuid, Uuid) {
    let mut project = Project::new("effect graph");
    let (mut composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 10.0);
    composition.background_color = Color::black();
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
    (project, composition_id, track_id)
}

fn project_with_graph(
    graph: NodeGraphBundle,
    start_time: f64,
    duration: f64,
) -> AnyResult<(Project, Uuid)> {
    let (mut project, _composition_id, track_id) = setup_project();
    let clip = Clip::new("effect clip", start_time, duration);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project
        .insert_node_graph(NodeContainer::Clip(clip_id), graph)
        .context("insert Effect graph into Clip")?;
    Ok((project, clip_id))
}

fn evaluate(
    project: &Project,
    plugins: &Arc<PluginManager>,
    frame_number: u64,
) -> AnyResult<FrameInfo> {
    get_frame_from_project(
        project,
        0,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )
    .context("evaluate Effect graph frame")
}

fn preview(project: &Project, plugins: &Arc<PluginManager>, frame_number: u64) -> AnyResult<Image> {
    let frame = evaluate(project, plugins, frame_number)?;
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
    match service.render_from_frame_info(&frame)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Texture(_) => bail!("CPU renderer unexpectedly returned a texture"),
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
    fn collect(items: &[FrameItem], ids: &mut Vec<Uuid>) {
        for item in items {
            match item {
                FrameItem::Object(object) => ids.push(object.source_node_id),
                FrameItem::Group(group) => collect(&group.items, ids),
            }
        }
    }
    let mut ids = Vec::new();
    collect(items, &mut ids);
    ids
}

#[test]
fn effect_descriptor_factory_materializes_defaults_and_distinct_image_ports() -> AnyResult<()> {
    let plugins = PluginManager::default();
    for (component_id, _, _) in plugins.get_available_effects() {
        let descriptor = plugins
            .operation_descriptor(EFFECT_CATEGORY, &component_id, EFFECT_APPLY_OPERATION)
            .with_context(|| format!("missing descriptor for Effect component {component_id}"))?;
        let node = plugins.create_effect_operation_node(&component_id)?;
        let NodeContent::PluginOperation(operation) = node.content() else {
            bail!("Effect factory must create a plugin operation");
        };
        assert_eq!(operation.category, EFFECT_CATEGORY);
        assert_eq!(operation.operation, EFFECT_APPLY_OPERATION);
        assert_eq!(operation.component_id, component_id);
        for definition in descriptor.properties() {
            assert_eq!(
                node.properties()
                    .get(definition.name())
                    .and_then(Property::value),
                Some(definition.default_value())
            );
            assert!(
                operation
                    .declared_ports
                    .iter()
                    .any(|port| port.key == property_port_key(definition.name()))
            );
        }
        let image_input = operation
            .declared_ports
            .iter()
            .find(|port| port.key == IMAGE_INPUT_PORT)
            .context("Effect operation has no Image input")?;
        assert_eq!(
            image_input.direction,
            library::model::project::PortDirection::Input
        );
        assert_eq!(image_input.side, PortSide::Left);
        assert_eq!(image_input.data_type, PortDataType::Image);
        let image_output = operation
            .declared_ports
            .iter()
            .find(|port| port.key == IMAGE_OUTPUT_PORT)
            .context("Effect operation has no Image output")?;
        assert_eq!(
            image_output.direction,
            library::model::project::PortDirection::Output
        );
        assert_eq!(image_output.side, PortSide::Right);
        assert_eq!(image_output.data_type, PortDataType::Image);
    }

    let collision = OperationDescriptor::new(
        "test",
        "same-key",
        "same-key.v1",
        "Same Key",
        Vec::new(),
        [
            PortDefinition::input("io", "Input", PortDataType::Image),
            output_port("io", PortDataType::Image),
        ],
    );
    assert!(matches!(
        collision,
        Err(OperationDescriptorError::PortCollision { .. })
    ));

    for (key, data_type) in [
        (FRAME_PORT, PortDataType::Integer),
        (FPS_PORT, PortDataType::Number),
    ] {
        let authored_input = OperationDescriptor::new(
            "test",
            format!("authored-{key}"),
            format!("authored-{key}.v1"),
            format!("Authored {key}"),
            Vec::new(),
            [PortDefinition::input(key, key, data_type)],
        );
        assert_eq!(
            authored_input.err(),
            Some(OperationDescriptorError::ReadOnlyDerivedTimingInput {
                key: key.to_string(),
            })
        );

        let readable_output = OperationDescriptor::new(
            "test",
            format!("readable-{key}"),
            format!("readable-{key}.v1"),
            format!("Readable {key}"),
            Vec::new(),
            [output_port(key, data_type)],
        )?;
        assert_eq!(readable_output.declared_ports()[0].key, key);

        let wrong_output_type = OperationDescriptor::new(
            "test",
            format!("wrong-{key}"),
            format!("wrong-{key}.v1"),
            format!("Wrong {key}"),
            Vec::new(),
            [output_port(key, PortDataType::String)],
        );
        assert_eq!(
            wrong_output_type.err(),
            Some(OperationDescriptorError::InvalidDerivedTimingOutputType {
                key: key.to_string(),
                expected: data_type,
                actual: PortDataType::String,
            })
        );
    }
    Ok(())
}

#[test]
fn effect_chain_uses_wiring_order_and_evaluates_keyframes_and_scalar_overrides() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let source = manager
        .create_solid_node(Color::white(), WIDTH, HEIGHT)
        .context("create Solid source")?;
    let mut blur = plugins.create_effect_operation_node("blur")?;
    let dilate = plugins.create_effect_operation_node("dilate")?;
    blur.set_property(
        "sigma_x".into(),
        Property::keyframe(vec![
            Keyframe::new(0.0, 0.0.into(), EasingFunction::Linear),
            Keyframe::new(1.0, 10.0.into(), EasingFunction::Linear),
        ]),
    )
    .map_err(|error| anyhow!("Blur descriptor must initialize sigma_x: {error}"))?;
    let source_id = source.id;
    let blur_id = blur.id;
    let dilate_id = dilate.id;
    let graph = NodeGraphBundle::new(
        vec![source, blur, dilate],
        vec![
            image_wire(source_id, blur_id),
            image_wire(blur_id, dilate_id),
        ],
        Some(dilate_id),
    );
    let (mut project, clip_id) = project_with_graph(graph, 0.0, 2.0)?;
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(blur_id), property_port_key("sigma_y")),
        )
        .context("connect Clip time to Blur sigma_y")?;

    let rendered = evaluate(&project, &plugins, 5)?;
    let outer = find_group(&rendered.items, dilate_id).context("Dilate group is missing")?;
    assert_eq!(outer.kind, FrameGroupKind::Effect);
    assert_eq!(outer.effects[0].effect_type, "dilate");
    let inner = find_group(&outer.items, blur_id).context("Blur group is missing")?;
    assert_eq!(inner.kind, FrameGroupKind::Effect);
    assert_eq!(inner.effects[0].effect_type, "blur");
    assert_eq!(
        inner.effects[0].properties["sigma_x"],
        PropertyValue::Number(OrderedFloat(5.0))
    );
    assert_eq!(
        inner.effects[0].properties["sigma_y"],
        PropertyValue::Number(OrderedFloat(0.5))
    );
    assert_eq!(
        object_source_ids(&rendered.items),
        vec![source_id],
        "Effect sinks must preserve the actual visual source Node"
    );

    let saved = project.save()?;
    let loaded = Project::load(&saved)?;
    assert_eq!(loaded, project);
    assert!(loaded.validation_issues().is_empty());
    Ok(())
}

#[test]
fn bypassed_image_effect_routes_input_without_descriptor_or_properties() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let source = manager.create_solid_node(Color::white(), WIDTH, HEIGHT)?;
    let source_id = source.id;
    let mut blur = plugins.create_effect_operation_node("blur")?;
    blur.bypassed = true;
    assert!(blur.supports_bypass());
    let blur_id = blur.id;
    let mut persisted = serde_json::to_value(blur)?;
    persisted["content"]["data"]["component_id"] =
        serde_json::Value::String("unavailable-blur".to_string());
    let blur = serde_json::from_value(persisted)?;
    let graph = NodeGraphBundle::new(
        vec![source, blur],
        vec![image_wire(source_id, blur_id)],
        Some(blur_id),
    );
    let (mut project, _) = project_with_graph(graph, 0.0, 2.0)?;

    let rendered = evaluate(&project, &plugins, 0)?;
    assert_eq!(object_source_ids(&rendered.items), [source_id]);
    assert!(find_group(&rendered.items, blur_id).is_none());
    assert_eq!(Project::load(&project.save()?)?, project);

    project.connections.clear();
    assert!(evaluate(&project, &plugins, 0)?.items.is_empty());

    project
        .get_node_mut(blur_id)
        .context("bypassed Effect remains authored")?
        .enabled = false;
    assert!(evaluate(&project, &plugins, 0)?.items.is_empty());
    Ok(())
}

struct DerivedTimingProbe;

impl Plugin for DerivedTimingProbe {
    fn id(&self) -> &str {
        "derived_timing_probe"
    }

    fn name(&self) -> String {
        "Derived Timing Probe".into()
    }

    fn category(&self) -> String {
        "Test".into()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for DerivedTimingProbe {
    fn apply(
        &self,
        _input: &RenderOutput,
        _params: &HashMap<String, PropertyValue>,
        _gpu_context: Option<&mut library::rendering::skia_utils::GpuContext>,
    ) -> Result<RenderOutput, library::LibraryError> {
        Err(library::LibraryError::Render(
            "the derived timing probe has no Image operation".into(),
        ))
    }

    fn properties(&self) -> Vec<library::model::property::PropertyDefinition> {
        Vec::new()
    }

    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::new(
            EFFECT_CATEGORY,
            self.id(),
            EFFECT_APPLY_OPERATION,
            self.name(),
            Vec::new(),
            [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                output_port(FRAME_PORT, PortDataType::Integer),
                output_port(FPS_PORT, PortDataType::Number),
            ],
        )
    }
}

#[test]
fn derived_timing_output_requires_an_available_enabled_operation() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    plugins.register_effect(Arc::new(DerivedTimingProbe));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let timing = plugins.create_effect_operation_node("derived_timing_probe")?;
    let timing_id = timing.id;
    let visual = manager
        .create_sksl_node("half4 main(float2 p) { return half4(1); }", WIDTH, HEIGHT)
        .context("create derived timing consumer")?;
    let visual_id = visual.id;
    let graph = NodeGraphBundle::new(
        vec![timing, visual],
        vec![
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(timing_id), FRAME_PORT),
                PortAddress::new(PortOwner::Node(visual_id), "width"),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(timing_id), FPS_PORT),
                PortAddress::new(PortOwner::Node(visual_id), "height"),
                0,
            ),
        ],
        Some(visual_id),
    );
    let (mut project, _) = project_with_graph(graph, 0.0, 2.0)?;

    assert_eq!(
        object_source_ids(&evaluate(&project, &plugins, 0)?.items),
        vec![visual_id]
    );
    project
        .get_node_mut(timing_id)
        .context("derived timing operation must exist")?
        .enabled = false;
    assert!(evaluate(&project, &plugins, 0)?.items.is_empty());

    let timing = project
        .get_node_mut(timing_id)
        .context("derived timing operation must remain mutable")?;
    timing.enabled = true;
    let original = serde_json::to_value(&*timing)?;
    let mut mismatched = original.clone();
    mismatched["content"]["data"]["declared_ports"]
        .as_array_mut()
        .context("persisted derived timing ports must be an array")?
        .push(serde_json::to_value(output_port(
            "extra",
            PortDataType::Number,
        ))?);
    *timing = serde_json::from_value(mismatched)?;
    assert!(evaluate(&project, &plugins, 0)?.items.is_empty());

    let timing = project
        .get_node_mut(timing_id)
        .context("mismatched derived timing operation must remain mutable")?;
    let mut unknown = original;
    unknown["content"]["data"]["component_id"] =
        serde_json::Value::String("unavailable-derived-timing".into());
    *timing = serde_json::from_value(unknown)?;
    assert!(evaluate(&project, &plugins, 0)?.items.is_empty());
    assert_eq!(Project::load(&project.save()?)?, project);
    Ok(())
}

#[test]
fn unknown_missing_input_and_scalar_no_output_effects_are_safe_no_output() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let missing = plugins.create_effect_operation_node("blur")?;
    let missing_id = missing.id;
    let (project, _) = project_with_graph(
        NodeGraphBundle::new(vec![missing], Vec::new(), Some(missing_id)),
        0.0,
        2.0,
    )?;
    assert!(evaluate(&project, &plugins, 0)?.items.is_empty());

    let source = manager
        .create_solid_node(Color::white(), WIDTH, HEIGHT)
        .context("create Solid source")?;
    let unknown = plugins.create_effect_operation_node("blur")?;
    let source_id = source.id;
    let unknown_id = unknown.id;
    let mut unknown_json = serde_json::to_value(unknown)?;
    unknown_json["content"]["data"]["component_id"] =
        serde_json::Value::String("unavailable-effect".to_string());
    unknown_json["content"]["data"]["declared_ports"]
        .as_array_mut()
        .context("persisted unknown Effect ports must be an array")?
        .push(serde_json::to_value(PortDefinition::input(
            FPS_PORT,
            "Authored legacy FPS",
            PortDataType::Number,
        ))?);
    let unknown: Node = serde_json::from_value(unknown_json)?;
    let (project, _) = project_with_graph(
        NodeGraphBundle::new(
            vec![source, unknown],
            vec![image_wire(source_id, unknown_id)],
            Some(unknown_id),
        ),
        0.0,
        2.0,
    )?;
    assert!(evaluate(&project, &plugins, 0)?.items.is_empty());
    let loaded = Project::load(&project.save()?)?;
    assert_eq!(loaded, project);
    assert!(loaded.validation_issues().is_empty());

    let blur = plugins.create_effect_operation_node("blur")?;
    let descriptor = plugins
        .operation_descriptor(EFFECT_CATEGORY, "blur", EFFECT_APPLY_OPERATION)
        .context("Blur descriptor is missing")?;
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    let project = Project::new("scalar NoOutput");
    let evaluators = plugins.get_property_evaluators();
    let operation_effect = |key: &str, input| {
        let mut inputs = ResolvedNodeInputs::default();
        inputs.properties.insert(key.into(), input);
        let context = FrameEvaluationContext {
            project: &project,
            composition: &composition,
            property_evaluators: &evaluators,
            plugin_manager: &plugins,
            resolved_inputs: Some(&inputs),
        };
        context.build_operation_effect("blur", descriptor.properties(), blur.properties(), 0.0)
    };
    assert!(operation_effect("sigma_x", EvalOutput::NoOutput).is_none());
    assert!(
        operation_effect(
            "sigma_x",
            EvalOutput::Produced(PropertyValue::String("wrong".into()))
        )
        .is_none()
    );
    assert!(operation_effect("sigma_x", EvalOutput::Produced((-1.0).into())).is_none());
    assert!(
        operation_effect(
            "tile_mode",
            EvalOutput::Produced(PropertyValue::String("outside-options".into()))
        )
        .is_none()
    );

    let mut invalid_keyframe = blur.properties().clone();
    invalid_keyframe.set(
        "tile_mode".into(),
        Property::keyframe(vec![Keyframe::new(
            0.0,
            PropertyValue::String("outside-options".into()),
            EasingFunction::Linear,
        )]),
    );
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    assert!(
        context
            .build_operation_effect("blur", descriptor.properties(), &invalid_keyframe, 0.0)
            .is_none()
    );
    Ok(())
}

struct PostCompositeProbe {
    calls: Arc<AtomicUsize>,
}

impl Plugin for PostCompositeProbe {
    fn id(&self) -> &str {
        "post_composite_probe"
    }

    fn name(&self) -> String {
        "Post Composite Probe".into()
    }

    fn category(&self) -> String {
        "Test".into()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for PostCompositeProbe {
    fn apply(
        &self,
        input: &RenderOutput,
        _params: &HashMap<String, PropertyValue>,
        _gpu_context: Option<&mut library::rendering::skia_utils::GpuContext>,
    ) -> Result<RenderOutput, library::LibraryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let RenderOutput::Image(image) = input else {
            return Err(library::LibraryError::Render(
                "probe requires a CPU image".into(),
            ));
        };
        let has_red = image
            .data
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 180 && pixel[2] < 80 && pixel[3] > 0);
        let has_blue = image
            .data
            .chunks_exact(4)
            .any(|pixel| pixel[2] > 180 && pixel[0] < 80 && pixel[3] > 0);
        let replacement = if has_red && has_blue {
            [0, 255, 0]
        } else {
            [255, 0, 255]
        };
        let mut data = image.data.clone();
        for pixel in data.chunks_exact_mut(4) {
            if pixel[3] > 0 {
                pixel[..3].copy_from_slice(&replacement);
            }
        }
        Ok(RenderOutput::Image(Image::new(
            image.width,
            image.height,
            data,
        )))
    }

    fn properties(&self) -> Vec<library::model::property::PropertyDefinition> {
        Vec::new()
    }
}

fn half_solid(
    manager: &ProjectManager,
    plugins: &PluginManager,
    color: Color,
    x: f64,
) -> AnyResult<(Node, Node)> {
    let source = manager.create_solid_node(color, WIDTH, HEIGHT)?;
    let mut transform = plugins.create_image_transform_operation_node()?;
    set_constant(&mut transform, "anchor", vec2(0.0, 0.0));
    set_constant(&mut transform, "scale", vec2(50.0, 100.0));
    set_constant(&mut transform, "position", vec2(x, 0.0));
    Ok((source, transform))
}

#[test]
fn merge_is_composited_before_effect_and_effect_is_applied_exactly_once() -> AnyResult<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_effect(Arc::new(PostCompositeProbe {
        calls: calls.clone(),
    }));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let (red, red_transform) = half_solid(
        &manager,
        &plugins,
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        0.0,
    )?;
    let (blue, blue_transform) = half_solid(
        &manager,
        &plugins,
        Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        },
        WIDTH as f64 / 2.0,
    )?;
    let merge = Node::new_merge("Merge");
    let effect = plugins
        .create_effect_operation_node("post_composite_probe")
        .context("create probe Effect operation")?;
    let red_id = red.id;
    let blue_id = blue.id;
    let red_transform_id = red_transform.id;
    let blue_transform_id = blue_transform.id;
    let merge_id = merge.id;
    let effect_id = effect.id;
    let merge_target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let graph = NodeGraphBundle::new(
        vec![red, red_transform, blue, blue_transform, merge, effect],
        vec![
            image_wire(red_id, red_transform_id),
            image_wire(blue_id, blue_transform_id),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(red_transform_id), IMAGE_OUTPUT_PORT),
                merge_target.clone(),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(blue_transform_id), IMAGE_OUTPUT_PORT),
                merge_target,
                1,
            ),
            image_wire(merge_id, effect_id),
        ],
        Some(effect_id),
    );
    let (project, _) = project_with_graph(graph, 0.0, 2.0)?;
    let frame = evaluate(&project, &plugins, 0)?;
    let effect_group = find_group(&frame.items, effect_id).context("Effect group is missing")?;
    assert_eq!(effect_group.kind, FrameGroupKind::Effect);
    assert_eq!(
        find_group(&effect_group.items, merge_id)
            .context("Merge group is missing")?
            .kind,
        FrameGroupKind::Merge
    );

    let image = preview(&project, &plugins, 0)?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let green_pixels = image
        .data
        .chunks_exact(4)
        .filter(|pixel| pixel[1] > 200 && pixel[0] < 30 && pixel[2] < 30)
        .count();
    assert!(green_pixels >= (WIDTH * HEIGHT) as usize - HEIGHT as usize);
    Ok(())
}

#[test]
fn inactive_effect_operation_never_invokes_plugin() -> AnyResult<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_effect(Arc::new(PostCompositeProbe {
        calls: calls.clone(),
    }));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let source = manager
        .create_solid_node(Color::white(), WIDTH, HEIGHT)
        .context("create Solid source")?;
    let effect = plugins
        .create_effect_operation_node("post_composite_probe")
        .context("create probe Effect operation")?;
    let source_id = source.id;
    let effect_id = effect.id;
    let (project, _) = project_with_graph(
        NodeGraphBundle::new(
            vec![source, effect],
            vec![image_wire(source_id, effect_id)],
            Some(effect_id),
        ),
        5.0,
        2.0,
    )?;

    preview(&project, &plugins, 0)?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    preview(&project, &plugins, 50)?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}
