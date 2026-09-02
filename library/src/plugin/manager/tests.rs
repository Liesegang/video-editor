use super::*;
use ordered_float::OrderedFloat;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::model::authoring::AuthoringProject;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::StyleConfig;
use crate::model::project::{EvalOutput, PortDirection, TIME_PORT};
use crate::model::property::{Property, PropertyMap, PropertyUiType, PropertyValue};
use crate::plugin::{
    EffectColorDomain, FrameEvaluationContext, OperationDescriptor, OperationDescriptorError,
    PropertyEvaluator, PropertyPlugin,
};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;
use ruvie_color_management::{
    BuiltinColorTransform, ColorContext, ColorTransformBackend, ColorTransformRequest,
    LINEAR_SRGB_SPACE_ID, ManagedLinearWorkingImage, SRGB_SPACE_ID, WorkingColorIdentity,
};

#[test]
fn every_bundled_property_definition_is_valid_and_operations_are_materialized() {
    fn check_definitions(
        scope: &str,
        definitions: &[PropertyDefinition],
        failures: &mut Vec<String>,
    ) {
        for definition in definitions {
            if let Err(error) = definition.validate_definition() {
                failures.push(format!("{scope} property '{}': {error}", definition.name()));
            }
        }
    }

    let manager = PluginManager::default();
    let sksl_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/plugins/sksl");
    manager
        .load_sksl_plugins_from_directory(&sksl_directory)
        .expect("bundled SkSL directory should be readable");

    let expected_sksl_ids = std::fs::read_dir(&sksl_directory)
        .expect("bundled SkSL directory should exist")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("config.toml"))
        .filter(|path| path.is_file())
        .map(|path| {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            toml::from_str::<crate::plugin::effects::sksl_plugin::SkslPluginConfig>(&source)
                .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()))
                .id
        })
        .collect::<HashSet<_>>();

    let mut failures = Vec::new();
    check_definitions(
        "Clip timing",
        crate::model::Clip::timing_property_definitions(),
        &mut failures,
    );
    for (label, value) in [
        ("native Fmod", crate::model::ValueContent::Fmod),
        ("native Add", crate::model::ValueContent::Add),
        ("native Subtract", crate::model::ValueContent::Subtract),
        ("native Multiply", crate::model::ValueContent::Multiply),
        ("native Divide", crate::model::ValueContent::Divide),
    ] {
        check_definitions(label, value.property_definitions(), &mut failures);
    }
    let transform_definitions = crate::plugin::transforms::property_definitions();
    check_definitions("native Transform", &transform_definitions, &mut failures);
    let mut operation_contracts = Vec::<(&'static str, String, usize)>::new();
    operation_contracts.push((
        TRANSFORM_CATEGORY,
        SHAPE_TRANSFORM_COMPONENT_ID.to_string(),
        transform_definitions.len(),
    ));
    operation_contracts.push((
        TRANSFORM_CATEGORY,
        IMAGE_TRANSFORM_COMPONENT_ID.to_string(),
        transform_definitions.len(),
    ));
    let registered_effect_ids;
    {
        let registry = manager.read_registry();
        registered_effect_ids = registry
            .effect_plugins
            .values()
            .map(|plugin| plugin.id().to_string())
            .collect::<HashSet<_>>();

        for plugin in registry.entity_converter_plugins.values() {
            let definitions = plugin.get_property_definitions(1920, 1080, 640, 360);
            check_definitions(
                &format!("converter {}", plugin.id()),
                &definitions,
                &mut failures,
            );
        }
        for kind in ["video", "image", "text", "shape", "solid", "sksl"] {
            if !registry
                .entity_converter_plugins
                .values()
                .any(|plugin| plugin.supports_kind(kind))
            {
                failures.push(format!("built-in converter kind {kind} is not registered"));
            }
        }
        for plugin in registry.export_plugins.values() {
            let definitions = plugin.properties();
            check_definitions(
                &format!("exporter {}", plugin.id()),
                &definitions,
                &mut failures,
            );
        }

        for plugin in registry.effect_plugins.values() {
            let definitions = plugin.properties();
            check_definitions(
                &format!("effect {}", plugin.id()),
                &definitions,
                &mut failures,
            );
            operation_contracts.push(("effect", plugin.id().to_string(), definitions.len()));
        }
        for plugin in registry.effector_plugins.values() {
            let definitions = plugin.properties();
            check_definitions(
                &format!("effector {}", plugin.id()),
                &definitions,
                &mut failures,
            );
            operation_contracts.push(("effector", plugin.id().to_string(), definitions.len()));
        }
        for plugin in registry.decorator_plugins.values() {
            let definitions = plugin.properties();
            check_definitions(
                &format!("decorator {}", plugin.id()),
                &definitions,
                &mut failures,
            );
            operation_contracts.push(("decorator", plugin.id().to_string(), definitions.len()));
        }
        for plugin in registry.style_plugins.values() {
            match plugin.descriptor() {
                Ok(descriptor) => {
                    check_definitions(
                        &format!("style {}", plugin.id()),
                        descriptor.properties(),
                        &mut failures,
                    );
                    operation_contracts.push((
                        "style",
                        plugin.id().to_string(),
                        descriptor.properties().len(),
                    ));
                }
                Err(error) => failures.push(format!(
                    "style {} descriptor is invalid: {error}",
                    plugin.id()
                )),
            }
        }
        for plugin in registry.path_effect_plugins.values() {
            let definitions = plugin.properties();
            check_definitions(
                &format!("path effect {}", plugin.id()),
                &definitions,
                &mut failures,
            );
            operation_contracts.push((
                PATH_EFFECT_CATEGORY,
                plugin.id().to_string(),
                definitions.len(),
            ));
        }
    }

    for missing in expected_sksl_ids.difference(&registered_effect_ids) {
        failures.push(format!("bundled SkSL effect {missing} was not registered"));
    }

    for (category, component_id, expected_property_count) in operation_contracts {
        let descriptor = match manager.operation_descriptor(
            category,
            &component_id,
            match category {
                "effect" => EFFECT_APPLY_OPERATION,
                "effector" => EFFECTOR_APPLY_OPERATION,
                "decorator" => DECORATOR_APPLY_OPERATION,
                "style" => STYLE_APPLY_OPERATION,
                PATH_EFFECT_CATEGORY => PATH_EFFECT_APPLY_OPERATION,
                TRANSFORM_CATEGORY => TRANSFORM_APPLY_OPERATION,
                unknown => {
                    failures.push(format!(
                        "operation contract uses unknown category {unknown} for {component_id}"
                    ));
                    continue;
                }
            },
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                failures.push(format!(
                    "{category} {component_id} descriptor is unreachable: {error}"
                ));
                continue;
            }
        };
        let first_input = descriptor
            .declared_ports()
            .iter()
            .find(|port| port.direction == PortDirection::Input);
        if first_input.map(|port| port.key.as_str()) != Some(TIME_PORT) {
            failures.push(format!(
                "{category} {component_id} must place Time before every property and payload input"
            ));
        }

        let result = match category {
            "effect" => manager.create_effect_operation_node(&component_id),
            "effector" => manager.create_effector_operation_node(&component_id),
            "decorator" => manager.create_decorator_operation_node(&component_id),
            "style" => manager.create_style_operation_node(&component_id),
            PATH_EFFECT_CATEGORY => manager.create_path_effect_operation_node(&component_id),
            TRANSFORM_CATEGORY if component_id == SHAPE_TRANSFORM_COMPONENT_ID => {
                manager.create_shape_transform_operation_node()
            }
            TRANSFORM_CATEGORY if component_id == IMAGE_TRANSFORM_COMPONENT_ID => {
                manager.create_image_transform_operation_node()
            }
            unknown => {
                failures.push(format!(
                    "operation contract uses unknown category {unknown} for {component_id}"
                ));
                continue;
            }
        };
        match result {
            Ok(node) => {
                let actual_property_count = node.properties().iter().count();
                if actual_property_count != expected_property_count {
                    failures.push(format!(
                            "{category} {component_id} initialized {actual_property_count} of {expected_property_count} properties"
                        ));
                }
                match node.content() {
                    crate::model::NodeContent::PluginOperation(operation)
                        if operation.declared_ports == descriptor.declared_ports() => {}
                    crate::model::NodeContent::PluginOperation(_) => failures.push(format!(
                        "{category} {component_id} factory changed the descriptor port order"
                    )),
                    _ => failures.push(format!(
                        "{category} {component_id} factory did not create a plugin operation"
                    )),
                }
            }
            Err(error) => failures.push(format!(
                "{category} {component_id} cannot create a complete Node: {error}"
            )),
        }
    }

    assert!(
        failures.is_empty(),
        "bundled property definition failures:\n{}",
        failures.join("\n")
    );
}

struct StatefulEvaluator {
    evaluations: Arc<AtomicUsize>,
}

impl PropertyEvaluator for StatefulEvaluator {
    fn evaluate(
        &self,
        _property: &Property,
        _time: f64,
        _context: &crate::plugin::EvaluationContext,
    ) -> Result<PropertyValue, crate::plugin::PropertyEvaluationError> {
        let value = self.evaluations.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(PropertyValue::Number(OrderedFloat(value as f64)))
    }
}

struct StatefulPropertyPlugin {
    evaluations: Arc<AtomicUsize>,
}

impl Plugin for StatefulPropertyPlugin {
    fn id(&self) -> &str {
        "stateful-test"
    }

    fn name(&self) -> String {
        "Stateful Test".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PropertyPlugin for StatefulPropertyPlugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator> {
        Arc::new(StatefulEvaluator {
            evaluations: Arc::clone(&self.evaluations),
        })
    }
}

struct EvaluatedValueStylePlugin;
struct ExternalFillReplacementStylePlugin;

const REENTRANT_EFFECT_ID: &str = "reentrant-drop-effect";

struct ReentrantDropEffect {
    manager: Arc<PluginManager>,
    callback_completed: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for ReentrantDropEffect {
    fn drop(&mut self) {
        let replacement_is_visible = self
            .manager
            .get_effect_plugin(REENTRANT_EFFECT_ID)
            .is_some();
        self.callback_completed
            .store(replacement_is_visible, Ordering::SeqCst);
    }
}

struct ReplacementEffect;
struct ContractDroppingEffect;

struct MetadataProbeLoader {
    id: &'static str,
    failure: Option<&'static str>,
}

impl Plugin for MetadataProbeLoader {
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> String {
        "Metadata Probe Loader".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl LoadPlugin for MetadataProbeLoader {
    fn open(&self, path: &str) -> crate::plugin::loaders::LoadPluginResult<Vec<AssetMetadata>> {
        match self.failure {
            Some(cause) => Err(LoadPluginError::Failed(LibraryError::Plugin(format!(
                "{cause}: {path}"
            )))),
            None => Err(LoadPluginError::Unsupported),
        }
    }

    fn load(
        &self,
        _request: &LoadRequest,
        _cache: &CacheManager,
    ) -> crate::plugin::loaders::LoadPluginResult<LoadResponse> {
        Err(LoadPluginError::Unsupported)
    }
}

macro_rules! impl_reentrant_test_effect {
    ($effect:ty) => {
        impl Plugin for $effect {
            fn id(&self) -> &str {
                REENTRANT_EFFECT_ID
            }

            fn name(&self) -> String {
                "Reentrant Drop Effect".to_string()
            }

            fn category(&self) -> String {
                "Tests".to_string()
            }

            fn version(&self) -> (u32, u32, u32) {
                (0, 1, 0)
            }
        }

        impl EffectPlugin for $effect {
            fn apply(
                &self,
                input: &crate::rendering::renderer::RenderOutput,
                _params: &HashMap<String, PropertyValue>,
                _gpu_context: Option<&mut crate::rendering::skia_utils::GpuContext>,
            ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
                Ok(input.clone())
            }

            fn properties(&self) -> Vec<PropertyDefinition> {
                Vec::new()
            }
        }
    };
}

impl_reentrant_test_effect!(ReentrantDropEffect);
impl_reentrant_test_effect!(ReplacementEffect);

impl Plugin for ContractDroppingEffect {
    fn id(&self) -> &str {
        "contract-dropping-effect"
    }

    fn name(&self) -> String {
        "Contract Dropping Effect".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for ContractDroppingEffect {
    fn apply(
        &self,
        _input: &RenderOutput,
        _params: &HashMap<String, PropertyValue>,
        _gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        Ok(RenderOutput::Image(crate::model::frame::Image::new(
            1,
            1,
            vec![0, 0, 0, 0],
        )))
    }

    fn properties(&self) -> Vec<PropertyDefinition> {
        Vec::new()
    }

    fn color_domain(&self) -> EffectColorDomain {
        EffectColorDomain::ProjectLinearPreserving
    }
}

fn managed_test_output(config: &str) -> RenderOutput {
    let backend = BuiltinColorTransform;
    let context = ColorContext::default();
    let source = backend
        .verify_source_space(SRGB_SPACE_ID, &context)
        .unwrap();
    let working = backend
        .verify_working_space(LINEAR_SRGB_SPACE_ID, &context)
        .unwrap();
    let identity = WorkingColorIdentity::from_verified(config, working).unwrap();
    let processor = backend
        .create_cpu_processor(&ColorTransformRequest::source_to_working(
            SRGB_SPACE_ID,
            LINEAR_SRGB_SPACE_ID,
        ))
        .unwrap();
    RenderOutput::Working(
        ManagedLinearWorkingImage::solid_from_straight_rgba8(
            identity,
            &source,
            1,
            1,
            [64, 128, 255, 255],
            processor.as_ref(),
        )
        .unwrap(),
    )
}

impl Plugin for EvaluatedValueStylePlugin {
    fn id(&self) -> &str {
        "evaluated-value-style"
    }

    fn name(&self) -> String {
        "Evaluated Value Style".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl StylePlugin for EvaluatedValueStylePlugin {
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::style(
            self.id(),
            self.name(),
            vec![PropertyDefinition::new(
                "value",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: String::new(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "Value",
                PropertyValue::from(0.0),
            )],
        )
    }

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        source_id: uuid::Uuid,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> Option<StyleConfig> {
        Some(StyleConfig {
            id: source_id,
            style: DrawStyle::Fill {
                color: Color::white(),
                offset: context.evaluate_number(properties, "value", eval_time, -1.0),
            },
        })
    }
}

impl Plugin for ExternalFillReplacementStylePlugin {
    fn id(&self) -> &str {
        "fill"
    }

    fn name(&self) -> String {
        "External Fill Replacement".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl StylePlugin for ExternalFillReplacementStylePlugin {
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::style(self.id(), self.name(), Vec::new())
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        _source_id: uuid::Uuid,
        _properties: &PropertyMap,
        _eval_time: f64,
    ) -> Option<StyleConfig> {
        None
    }
}

#[test]
fn external_registration_does_not_expand_the_bundled_operation_inventory() {
    let manager = PluginManager::default();
    let before = manager
        .bundled_operation_descriptors()
        .expect("bundled descriptors must resolve")
        .into_iter()
        .map(|descriptor| {
            (
                descriptor.category().to_string(),
                descriptor.component_id().to_string(),
                descriptor.operation().to_string(),
                descriptor.label().to_string(),
                descriptor.declared_ports().to_vec(),
            )
        })
        .collect::<Vec<_>>();

    manager.register_style_plugin(Arc::new(EvaluatedValueStylePlugin));
    manager.register_style_plugin(Arc::new(ExternalFillReplacementStylePlugin));
    assert!(
        manager
            .operation_descriptor(
                STYLE_CATEGORY,
                "evaluated-value-style",
                STYLE_APPLY_OPERATION,
            )
            .is_ok(),
        "external operation must remain runtime reachable"
    );
    assert!(
        manager
            .create_style_operation_node("evaluated-value-style")
            .is_ok(),
        "external operation factory must remain runtime reachable"
    );
    assert_eq!(
        manager
            .operation_descriptor(STYLE_CATEGORY, "fill", STYLE_APPLY_OPERATION)
            .expect("same-ID external replacement must remain runtime reachable")
            .label(),
        "External Fill Replacement"
    );

    let after = manager
        .bundled_operation_descriptors()
        .expect("external registration must not invalidate bundled descriptors")
        .into_iter()
        .map(|descriptor| {
            (
                descriptor.category().to_string(),
                descriptor.component_id().to_string(),
                descriptor.operation().to_string(),
                descriptor.label().to_string(),
                descriptor.declared_ports().to_vec(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(after, before);
    assert!(
        after
            .iter()
            .all(|(_, component_id, _, _, _)| component_id != "evaluated-value-style"),
        "third-party operations belong to the runtime registry, not the static bundled catalog"
    );
}

#[test]
fn operation_validation_materializes_stateful_values_before_plugin_evaluation() {
    let evaluations = Arc::new(AtomicUsize::new(0));
    let manager = PluginManager::default();
    manager.register_property_plugin(Arc::new(StatefulPropertyPlugin {
        evaluations: Arc::clone(&evaluations),
    }));
    manager.register_style_plugin(Arc::new(EvaluatedValueStylePlugin));
    let mut node = manager
        .create_style_operation_node("evaluated-value-style")
        .expect("test Style descriptor creates a Node");
    node.set_property(
        "value".to_string(),
        Property {
            evaluator: "stateful-test".to_string(),
            properties: std::collections::HashMap::new(),
        },
    )
    .expect("descriptor initializes the value property");

    let project = AuthoringProject::new("stateful operation property", 640, 360, 30.0, 1.0)
        .expect("Timeline-first Project");
    let timeline = &project.timelines[&project.root_timeline_id];
    let property_evaluators = manager.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        timeline,
        property_evaluators: &property_evaluators,
        plugin_manager: &manager,
        resolved_inputs: None,
    };

    let output = manager.evaluate_style_operation(
        &context,
        "evaluated-value-style",
        node.id,
        node.properties(),
        0.0,
    );
    let EvalOutput::Produced(StyleConfig {
        style: DrawStyle::Fill { offset, .. },
        ..
    }) = output
    else {
        panic!("valid stateful property should produce a Style config")
    };
    assert_eq!(offset, 1.0);
    assert_eq!(
        evaluations.load(Ordering::SeqCst),
        1,
        "validation must not invoke an authored evaluator again inside plugin code"
    );
}

#[test]
fn replacing_effect_drops_old_plugin_after_manager_write_lock_is_released()
-> Result<(), Box<dyn std::error::Error>> {
    let manager = Arc::new(PluginManager::new());
    let callback_completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    manager.register_effect(Arc::new(ReentrantDropEffect {
        manager: Arc::clone(&manager),
        callback_completed: Arc::clone(&callback_completed),
    }));

    let (completed_tx, completed_rx) = std::sync::mpsc::channel();
    let worker_manager = Arc::clone(&manager);
    let worker = std::thread::spawn(move || {
        worker_manager.register_effect(Arc::new(ReplacementEffect));
        completed_tx.send(()).map_err(|error| error.to_string())
    });

    completed_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let worker_result = worker
        .join()
        .map_err(|_| std::io::Error::other("replacement worker panicked"))?;
    worker_result.map_err(std::io::Error::other)?;
    assert!(
        callback_completed.load(Ordering::SeqCst),
        "the old plugin destructor must be able to read the committed replacement"
    );
    Ok(())
}

#[test]
fn metadata_probe_preserves_claimed_loader_failure() -> Result<(), Box<dyn std::error::Error>> {
    let manager = PluginManager::new();
    manager.register_load_plugin(Arc::new(MetadataProbeLoader {
        id: "declining-metadata-loader",
        failure: None,
    }));
    assert!(
        manager
            .get_available_streams("/fixtures/custom.asset")?
            .is_none(),
        "all Unsupported responses must remain an unclaimed path"
    );

    manager.register_load_plugin(Arc::new(MetadataProbeLoader {
        id: "claiming-metadata-loader",
        failure: Some("fixture header is truncated"),
    }));
    let Err(error) = manager.get_available_streams("/fixtures/custom.asset") else {
        return Err(
            std::io::Error::other("a claimed metadata failure must not become Ok(None)").into(),
        );
    };
    let message = error.to_string();
    assert!(message.contains("fixture header is truncated"));
    assert!(message.contains("/fixtures/custom.asset"));
    assert!(!message.contains("No compatible load plugin"));
    Ok(())
}

#[test]
fn replacing_same_id_plugin_advances_render_revision() {
    let manager = PluginManager::new();
    let initial_revision = manager.render_revision();

    manager.register_load_plugin(Arc::new(MetadataProbeLoader {
        id: "replaceable-loader",
        failure: None,
    }));
    let registered_revision = manager.render_revision();
    assert!(
        registered_revision > initial_revision,
        "registering a render dependency must invalidate Preview authority"
    );

    manager.register_load_plugin(Arc::new(MetadataProbeLoader {
        id: "replaceable-loader",
        failure: Some("replacement implementation"),
    }));
    assert!(
        manager.render_revision() > registered_revision,
        "replacing a plugin under the same stable ID must also invalidate Preview authority"
    );
}

#[test]
fn project_working_effects_fail_closed_when_missing_or_legacy_only() {
    let manager = PluginManager::new();
    let working = managed_test_output("effect-fail-closed");
    let missing = manager
        .apply_effect("missing-effect", &working, &HashMap::new(), None)
        .expect_err("a missing Project effect must not be silently bypassed");
    assert!(missing.to_string().contains("refusing to bypass"));

    manager.register_effect(Arc::new(ReplacementEffect));
    let legacy = manager
        .apply_effect(REENTRANT_EFFECT_ID, &working, &HashMap::new(), None)
        .expect_err("an unmanaged-only effect must fail before touching Project pixels");
    assert!(legacy.to_string().contains("unmanaged encoded-sRGBA8"));
}

#[test]
fn declared_project_effect_cannot_drop_the_working_contract() {
    let manager = PluginManager::new();
    manager.register_effect(Arc::new(ContractDroppingEffect));
    let error = manager
        .apply_effect(
            "contract-dropping-effect",
            &managed_test_output("effect-contract"),
            &HashMap::new(),
            None,
        )
        .expect_err("typed Project effect must return the same managed contract");
    assert!(
        error
            .to_string()
            .contains("dropped the Project working RGBAF32 contract")
    );
}
