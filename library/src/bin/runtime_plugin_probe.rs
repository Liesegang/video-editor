//! Executable proof for installing a third-party plugin after this host binary
//! has already been built. See `scripts/test-runtime-plugin.sh`.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail};
use library::model::property::{Property, PropertyMap, PropertyValue};
use library::plugin::native_plugin_api::{PROPERTY_CATEGORY, PropertyValueV1};
use library::plugin::{EvaluationContext, PluginManager};

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
        .find(|component| component.category == PROPERTY_CATEGORY)
        .context("bundle descriptor has no integrated property component")?;
    let component_id = component.id.clone();
    if !matches!(
        component.output_default.as_ref(),
        Some(PropertyValueV1::Number { value }) if value.abs() < f64::EPSILON
    ) {
        bail!("property descriptor has no explicit numeric fail-safe default");
    }

    let property = manager.create_property_instance(&component_id)?;
    let descriptor_property_names = component
        .properties
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<BTreeSet<_>>();
    let instance_property_names = property
        .properties
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if descriptor_property_names != instance_property_names {
        bail!("descriptor-backed factory did not materialize every property");
    }

    let evaluators = manager.get_property_evaluators();
    let sibling_properties = PropertyMap::new();
    let context = EvaluationContext {
        property_map: &sibling_properties,
        fps: 30.0,
    };
    let complete_output = evaluators.evaluate(&property, 0.25, &context);
    let sparse = Property {
        evaluator: component_id.clone(),
        properties: HashMap::new(),
    };
    let recovered_output = evaluators.evaluate(&sparse, 0.25, &context);
    if complete_output != recovered_output {
        bail!("sparse property did not evaluate with descriptor defaults");
    }
    if !matches!(
        complete_output,
        PropertyValue::Number(value) if (-1.0..=1.0).contains(&value.into_inner())
    ) {
        bail!("runtime property returned an invalid value");
    }

    verify_unknown_property_is_safe(&manager, &context)?;

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

fn verify_unknown_property_is_safe(
    manager: &PluginManager,
    context: &EvaluationContext,
) -> anyhow::Result<()> {
    let unknown = Property {
        evaluator: "unavailable.vendor.property".to_string(),
        properties: HashMap::from([(
            "vendor_private_value".to_string(),
            PropertyValue::String("keep exactly".to_string()),
        )]),
    };
    let json = serde_json::to_string(&unknown)?;
    let preserved: Property = serde_json::from_str(&json)?;
    if preserved != unknown {
        bail!("serialization changed unavailable property configuration");
    }

    // The registry's pre-existing unknown-evaluator fail-safe must not panic;
    // importantly, the authoritative Property above stays untouched.
    let evaluators = manager.get_property_evaluators();
    let _safe_value = evaluators.evaluate(&preserved, 0.0, context);
    if manager
        .invoke_runtime_plugin(
            PROPERTY_CATEGORY,
            &preserved.evaluator,
            "property.evaluate.v1",
            serde_json::json!({}),
        )
        .is_ok()
    {
        bail!("unknown runtime component unexpectedly invoked a plugin");
    }
    Ok(())
}
