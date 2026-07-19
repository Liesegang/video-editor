//! Executable proof for installing a third-party plugin after this host binary
//! has already been built. See `scripts/test-runtime-plugin.sh`.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{Context, bail};
use library::core::ensemble::types::EffectorConfig;
use library::editor::ProjectService;
use library::model::property::{Property, PropertyMap, PropertyValue};
use library::model::{Composition, EffectorInstance, Node, NodeContainer, NodeContent, Project};
use library::plugin::native_plugin_api::EFFECTOR_CATEGORY;
use library::plugin::{FrameEvaluationContext, PluginManager};

fn main() -> anyhow::Result<()> {
    let bundle_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: runtime_plugin_probe <bundle-directory>")?;
    let manager = Arc::new(PluginManager::default());

    let report = manager.rescan_runtime_plugin_path(&bundle_path);
    if !report.failures.is_empty() {
        bail!("runtime scan failed: {:?}", report.failures);
    }
    if report.loaded_bundles.len() != 1 || report.registered_components.len() != 1 {
        bail!("unexpected first scan report: {report:?}");
    }

    let descriptors = manager.get_runtime_plugin_descriptors();
    let component = descriptors
        .iter()
        .flat_map(|bundle| &bundle.descriptor.components)
        .find(|component| component.category == EFFECTOR_CATEGORY)
        .context("bundle descriptor has no integrated Effector component")?;
    let component_id = component.id.clone();
    let instance = manager.create_effector_instance(&component_id)?;
    let descriptor_property_names = component
        .properties
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let instance_property_names = instance
        .properties
        .iter()
        .map(|(name, _property)| name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if descriptor_property_names != instance_property_names {
        bail!("descriptor-backed factory did not materialize every property");
    }

    let project = Project::new("Runtime plugin invocation");
    let (composition, _root_track) = Composition::new("Main", 640, 360, 30.0, 1.0);
    let evaluators = manager.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &manager,
        resolved_inputs: None,
    };
    let plugin = manager
        .get_effector_plugin(&component_id)
        .context("runtime Effector was not registered in the normal repository")?;
    let complete_output = plugin
        .convert(&context, &instance, 0.25)
        .context("runtime Effector produced no config")?;
    let missing_properties = EffectorInstance::new(&component_id, PropertyMap::new());
    let recovered_output = plugin
        .convert(&context, &missing_properties, 0.25)
        .context("descriptor defaults did not recover a sparse instance")?;
    if !equivalent_output(&complete_output, &recovered_output) {
        bail!("sparse instance did not evaluate with descriptor defaults");
    }

    verify_unknown_project_config_is_preserved(Arc::clone(&manager))?;

    let second = manager.rescan_runtime_plugin_path(&bundle_path);
    if !second.failures.is_empty() || second.already_loaded_bundles.len() != 1 {
        bail!("runtime rescan was not idempotent: {second:?}");
    }

    println!(
        "runtime plugin proof passed: component={}, properties={}, prebuilt_host=true",
        component_id,
        descriptor_property_names.len()
    );
    Ok(())
}

fn equivalent_output(left: &EffectorConfig, right: &EffectorConfig) -> bool {
    match (left, right) {
        (
            EffectorConfig::Opacity {
                target_opacity: left_opacity,
                mode: left_mode,
                target: left_target,
            },
            EffectorConfig::Opacity {
                target_opacity: right_opacity,
                mode: right_mode,
                target: right_target,
            },
        ) => {
            (left_opacity - right_opacity).abs() < f32::EPSILON
                && left_mode == right_mode
                && left_target == right_target
        }
        (
            EffectorConfig::Transform {
                translate: left_translate,
                rotate: left_rotate,
                scale: left_scale,
                target: left_target,
            },
            EffectorConfig::Transform {
                translate: right_translate,
                rotate: right_rotate,
                scale: right_scale,
                target: right_target,
            },
        ) => {
            left_translate == right_translate
                && left_rotate == right_rotate
                && left_scale == right_scale
                && left_target == right_target
        }
        _ => false,
    }
}

fn verify_unknown_project_config_is_preserved(manager: Arc<PluginManager>) -> anyhow::Result<()> {
    let (composition, track) = Composition::new("Unknown config", 640, 360, 30.0, 1.0);
    let composition_id = composition.id;
    let mut properties = PropertyMap::new();
    properties.set(
        "vendor_private_value".to_string(),
        Property::constant(PropertyValue::String("keep exactly".to_string())),
    );
    let unknown = EffectorInstance::new("unavailable.vendor.effector", properties);
    let mut node = Node::new("Unknown plugin holder", NodeContent::Merge);
    let node_id = node.id;
    node.effectors.push(unknown.clone());

    let mut project = Project::new("Unknown plugin preservation");
    project.add_track(track);
    project.add_composition(composition);
    project.add_node(node);
    project.attach_node_to_container(NodeContainer::Composition(composition_id), node_id)?;
    let json = project.save()?;

    let shared = Arc::new(RwLock::new(Project::new("Before load")));
    let service = ProjectService::new(Arc::clone(&shared), manager);
    let loaded = service.load_project(&json)?;
    let loaded_effector = loaded
        .get_node(node_id)
        .and_then(|node| node.effectors.first())
        .context("ProjectService load dropped the unavailable plugin config")?;
    if loaded_effector != &unknown {
        bail!("ProjectService load changed unavailable plugin configuration");
    }
    Ok(())
}
