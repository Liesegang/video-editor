use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use library::animation::EasingFunction;
use library::core::ensemble::effectors::OpacityMode;
use library::core::ensemble::target::EffectorTarget;
use library::core::ensemble::types::EffectorConfig;
use library::editor::project_service::ProjectManager;
use library::model::Node;
use library::model::frame::entity::FrameContent;
use library::model::project::{
    Composition, EvalOutput, PortAddress, PortDataType, PortDefinition, PortExposure, PortOwner,
    PortSide, Project, TIME_PORT,
};
use library::model::property::{
    Keyframe, Property, PropertyDefinition, PropertyMap, PropertyValue,
};
use library::plugin::{
    EffectorPlugin, FrameEvaluationContext, OperationDescriptor, OperationDescriptorError, Plugin,
    PluginManager, ResolvedNodeInputs, property_port_key,
};
use uuid::Uuid;

use super::support::{
    FPS, HEIGHT, WIDTH, evaluate, first_content, insert_effector_chain, project_with_graph,
    set_constant,
};

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
        _context: &library::plugin::EvaluatedOperation<'_>,
        _source_id: Uuid,
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
