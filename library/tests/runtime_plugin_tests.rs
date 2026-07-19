use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};

use library::model::property::{Property, PropertyMap, PropertyValue};
use library::model::{Composition, NodeContainer, Project};
use library::plugin::native_plugin_api::{
    DECORATOR_CATEGORY, EFFECT_CATEGORY, LOADER_CATEGORY, PROPERTY_CATEGORY, PropertyValueV1,
    STYLE_CATEGORY,
};
use library::plugin::{EvaluationContext, PluginManager};

const COMPONENT_ID: &str = "random_property";
const FILL_COMPONENT_ID: &str = "runtime_fill_style";
const STROKE_COMPONENT_ID: &str = "runtime_stroke_style";
const BACKPLATE_COMPONENT_ID: &str = "runtime_backplate_decorator";
const EFFECT_COMPONENT_ID: &str = "runtime_solid_tint_effect";
const LOADER_COMPONENT_ID: &str = "runtime_rgba_fixture_loader";
const DESCRIPTOR_CALLS_OPERATION: &str = "random_property.descriptor_calls.v1";

#[test]
fn common_effector_operation_factory_materializes_all_known_defaults() {
    let manager = PluginManager::default();
    let opacity = manager
        .create_effector_operation_node("opacity")
        .expect("built-in descriptor creates an explicit operation Node");
    assert!(opacity.properties().get("opacity").is_some());
    assert!(opacity.properties().get("mode").is_some());
    assert!(opacity.properties().get("target").is_some());
    let (composition, track) = Composition::new("Main", 640, 360, 30.0, 1.0);
    let composition_id = composition.id;
    let mut encoded_node = serde_json::to_value(opacity).unwrap();
    encoded_node["content"]["data"]["component_id"] =
        serde_json::Value::String("not.installed".to_string());
    let mut node: library::model::Node = serde_json::from_value(encoded_node).unwrap();
    let node_id = node.id;
    node.set_property(
        "private".to_string(),
        Property::constant(PropertyValue::String("preserve".to_string())),
    );

    let mut project = Project::new("Service boundary");
    project.add_track(track);
    project.add_composition(composition);
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
        .expect("test containment is valid");
    let saved = project.save().unwrap();
    let loaded = Project::load(&saved).unwrap();
    assert_eq!(loaded, project);
    assert_eq!(
        loaded
            .get_node(node_id)
            .and_then(|node| node.properties().get("private"))
            .and_then(Property::value),
        Some(&PropertyValue::String("preserve".to_string()))
    );
}

fn bundle_from_environment() -> PathBuf {
    std::env::var_os("RUVIE_TEST_PLUGIN_BUNDLE")
        .map(PathBuf::from)
        .expect("RUVIE_TEST_PLUGIN_BUNDLE must name the independently built test bundle")
}

#[test]
#[ignore = "requires the independently built bundle from scripts/test-runtime-plugin.sh"]
fn standalone_runtime_bundle_loads_builds_nodes_and_invokes() {
    let bundle = bundle_from_environment();
    let manager = Arc::new(PluginManager::default());
    let workers = 12;
    let barrier = Arc::new(Barrier::new(workers));
    let scans = (0..workers)
        .map(|_| {
            let manager = Arc::clone(&manager);
            let barrier = Arc::clone(&barrier);
            let bundle = bundle.clone();
            std::thread::spawn(move || {
                barrier.wait();
                manager.rescan_runtime_plugin_path(bundle)
            })
        })
        .collect::<Vec<_>>();
    let reports = scans
        .into_iter()
        .map(|worker| worker.join().expect("runtime scan worker panicked"))
        .collect::<Vec<_>>();
    assert!(
        reports.iter().all(|report| report.failures.is_empty()),
        "concurrent scan failures: {reports:?}"
    );
    assert_eq!(
        reports
            .iter()
            .map(|report| report.loaded_bundles.len())
            .sum::<usize>(),
        1,
        "exactly one concurrent scan must load the library"
    );
    assert_eq!(
        reports
            .iter()
            .map(|report| {
                report.loaded_bundles.len()
                    + report.already_loaded_bundles.len()
                    + report.in_flight_bundles.len()
            })
            .sum::<usize>(),
        workers,
        "every scan must report loaded, already-loaded, or in-flight"
    );
    let descriptor_calls = manager
        .invoke_runtime_plugin(
            PROPERTY_CATEGORY,
            COMPONENT_ID,
            DESCRIPTOR_CALLS_OPERATION,
            serde_json::json!({}),
        )
        .expect("test plugin reports descriptor callback count");
    assert_eq!(
        descriptor_calls
            .get("calls")
            .and_then(serde_json::Value::as_u64),
        Some(1),
        "identity must be claimed before dlopen/descriptor callbacks"
    );

    let report = reports
        .iter()
        .find(|report| !report.loaded_bundles.is_empty())
        .expect("one scan loaded the runtime bundle");
    assert_eq!(
        report.registered_components,
        vec![
            (PROPERTY_CATEGORY.to_string(), COMPONENT_ID.to_string()),
            (STYLE_CATEGORY.to_string(), FILL_COMPONENT_ID.to_string()),
            (STYLE_CATEGORY.to_string(), STROKE_COMPONENT_ID.to_string()),
            (
                DECORATOR_CATEGORY.to_string(),
                BACKPLATE_COMPONENT_ID.to_string()
            ),
            (EFFECT_CATEGORY.to_string(), EFFECT_COMPONENT_ID.to_string()),
            (LOADER_CATEGORY.to_string(), LOADER_COMPONENT_ID.to_string()),
        ]
    );

    let descriptors = manager.get_runtime_plugin_descriptors();
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].descriptor.components.len(), 6);
    let component = descriptors[0]
        .descriptor
        .components
        .iter()
        .find(|component| component.category == PROPERTY_CATEGORY)
        .expect("bundle has its property component");
    assert_eq!(component.id, COMPONENT_ID);
    assert_eq!(component.category, PROPERTY_CATEGORY);
    assert!(matches!(
        component.output_default.as_ref(),
        Some(PropertyValueV1::Number { value }) if value.abs() < f64::EPSILON
    ));
    let names = component
        .properties
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["amplitude", "seed"]);
    for (category, id, node) in [
        (
            STYLE_CATEGORY,
            FILL_COMPONENT_ID,
            manager
                .create_style_operation_node(FILL_COMPONENT_ID)
                .expect("runtime Fill creates a graph Node"),
        ),
        (
            STYLE_CATEGORY,
            STROKE_COMPONENT_ID,
            manager
                .create_style_operation_node(STROKE_COMPONENT_ID)
                .expect("runtime Stroke creates a graph Node"),
        ),
        (
            DECORATOR_CATEGORY,
            BACKPLATE_COMPONENT_ID,
            manager
                .create_decorator_operation_node(BACKPLATE_COMPONENT_ID)
                .expect("runtime Backplate creates a graph Node"),
        ),
        (
            EFFECT_CATEGORY,
            EFFECT_COMPONENT_ID,
            manager
                .create_effect_operation_node(EFFECT_COMPONENT_ID)
                .expect("runtime Effect creates a graph Node"),
        ),
    ] {
        let component = descriptors[0]
            .descriptor
            .components
            .iter()
            .find(|component| component.category == category && component.id == id)
            .expect("runtime config component stays in the accepted descriptor");
        let descriptor_names = component
            .properties
            .iter()
            .map(|definition| definition.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let node_names = node
            .properties()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            node_names, descriptor_names,
            "descriptor-backed runtime factory must materialize every property"
        );
    }

    let instance = manager
        .create_property_instance(COMPONENT_ID)
        .expect("descriptor-backed factory creates the runtime property");
    assert_eq!(
        instance.properties.get("amplitude"),
        Some(&PropertyValue::from(1.0))
    );
    assert_eq!(
        instance.properties.get("seed"),
        Some(&PropertyValue::Integer(0))
    );

    let evaluators = manager.get_property_evaluators();
    let sibling_properties = PropertyMap::new();
    let context = EvaluationContext {
        property_map: &sibling_properties,
        fps: 30.0,
    };
    let output = evaluators.evaluate(&instance, 0.25, &context);
    assert!(matches!(
        &output,
        PropertyValue::Number(value) if (-1.0..=1.0).contains(&value.into_inner())
    ));
    let sparse = Property {
        evaluator: COMPONENT_ID.to_string(),
        properties: HashMap::new(),
    };
    assert_eq!(evaluators.evaluate(&sparse, 0.25, &context), output);

    let unavailable = Property {
        evaluator: "not.installed.property".to_string(),
        properties: HashMap::from([(
            "private".to_string(),
            PropertyValue::String("preserve".to_string()),
        )]),
    };
    let unavailable_json =
        serde_json::to_string(&unavailable).expect("unknown property serializes");
    let preserved: Property =
        serde_json::from_str(&unavailable_json).expect("unknown property stays loadable");
    assert_eq!(preserved, unavailable);
    let _safe_unknown_output = evaluators.evaluate(&preserved, 0.25, &context);
    assert!(
        manager
            .invoke_runtime_plugin(
                PROPERTY_CATEGORY,
                "not.installed.property",
                "property.evaluate.v1",
                serde_json::json!({}),
            )
            .is_err(),
        "unknown runtime component must not be routed to another plugin"
    );

    // On Unix a loaded native image remains mapped after its file is renamed.
    // Temporarily removing the binary proves the already-loaded manifest check
    // happens before manifest parsing, library resolution, dlopen, or callbacks.
    #[cfg(unix)]
    let moved_library = {
        let library_path = descriptors[0].library_path.clone();
        let backup_path = library_path.with_extension("loaded-test-backup");
        std::fs::rename(&library_path, &backup_path)
            .expect("temporarily move the already-loaded plugin binary");
        Some((library_path, backup_path))
    };
    #[cfg(not(unix))]
    let moved_library: Option<(PathBuf, PathBuf)> = None;

    let second = manager.rescan_runtime_plugin_path(&bundle);
    if let Some((library_path, backup_path)) = moved_library {
        std::fs::rename(backup_path, library_path)
            .expect("restore the independently built plugin binary");
    }
    assert!(second.failures.is_empty());
    assert_eq!(second.already_loaded_bundles.len(), 1);

    let serialized = serde_json::to_string(&instance).expect("property instance serializes");
    drop(manager);
    let preserved: Property =
        serde_json::from_str(&serialized).expect("unavailable plugin config stays loadable");
    assert_eq!(preserved, instance);
}
