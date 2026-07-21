use super::*;
use ordered_float::OrderedFloat;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::StyleConfig;
use crate::model::project::{Composition, EvalOutput, Project};
use crate::model::property::{Property, PropertyMap, PropertyUiType, PropertyValue};
use crate::plugin::{
    FrameEvaluationContext, OperationDescriptor, OperationDescriptorError, PropertyEvaluator,
    PropertyPlugin,
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

    let (composition, track) = Composition::new("Main", 640, 360, 30.0, 1.0);
    let composition_id = composition.id;
    let mut project = Project::new("stateful operation property");
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let composition = project
        .get_composition(composition_id)
        .expect("test composition exists");
    let property_evaluators = manager.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition,
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
