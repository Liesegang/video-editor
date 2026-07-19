use std::path::PathBuf;
use std::sync::{Arc, Barrier, RwLock};

use library::core::ensemble::effectors::OpacityMode;
use library::core::ensemble::target::EffectorTarget;
use library::core::ensemble::types::EffectorConfig;
use library::editor::ProjectService;
use library::model::property::{Property, PropertyMap, PropertyValue};
use library::model::{Composition, EffectorInstance, Node, NodeContainer, NodeContent, Project};
use library::plugin::{FrameEvaluationContext, PluginManager};

const COMPONENT_ID: &str = "example.third_party_opacity";
const DESCRIPTOR_CALLS_OPERATION: &str = "example.descriptor_calls.v1";

#[test]
fn common_effector_factory_and_update_boundary_materialize_all_known_defaults() {
    let manager = Arc::new(PluginManager::default());
    let opacity = manager
        .create_effector_instance("opacity")
        .expect("built-in definitions use the common factory");
    assert!(opacity.properties.get("opacity").is_some());
    assert!(opacity.properties.get("mode").is_some());
    assert!(opacity.properties.get("target").is_some());

    let evaluation_project = Project::new("Sparse evaluation");
    let (evaluation_composition, _evaluation_track) =
        Composition::new("Sparse", 640, 360, 30.0, 1.0);
    let evaluators = manager.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &evaluation_project,
        composition: &evaluation_composition,
        property_evaluators: &evaluators,
        plugin_manager: &manager,
        resolved_inputs: None,
    };
    let sparse_output = manager
        .convert_effector_instance(
            &context,
            &EffectorInstance::new("opacity", PropertyMap::new()),
            0.0,
        )
        .expect("known sparse instance resolves definitions in-memory");
    assert!(matches!(
        sparse_output,
        EffectorConfig::Opacity {
            target_opacity,
            mode: OpacityMode::Set,
            target: EffectorTarget::Block,
        } if target_opacity.abs() < f32::EPSILON
    ));

    let (composition, track) = Composition::new("Main", 640, 360, 30.0, 1.0);
    let composition_id = composition.id;
    let mut node = Node::new("Text holder", NodeContent::Merge);
    let node_id = node.id;
    let mut sparse_properties = PropertyMap::new();
    sparse_properties.set(
        "opacity".to_string(),
        Property::constant(PropertyValue::from(25.0)),
    );
    let sparse = EffectorInstance::new("opacity", sparse_properties);
    let mut unavailable_properties = PropertyMap::new();
    unavailable_properties.set(
        "private".to_string(),
        Property::constant(PropertyValue::String("preserve".to_string())),
    );
    let unavailable = EffectorInstance::new("not.installed", unavailable_properties);
    node.effectors = vec![sparse, unavailable.clone()];

    let mut project = Project::new("Service boundary");
    project.add_track(track);
    project.add_composition(composition);
    project.add_node(node.clone());
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
        .expect("test containment is valid");
    let shared = Arc::new(RwLock::new(project));
    let service = ProjectService::new(Arc::clone(&shared), manager);
    service
        .update_node_effectors(node_id, node.effectors)
        .expect("service update accepts recoverable and unavailable configs");
    let project = shared.read().expect("test project lock");
    let updated = &project.get_node(node_id).expect("test node").effectors;
    assert!(updated[0].properties.get("mode").is_some());
    assert!(updated[0].properties.get("target").is_some());
    assert_eq!(updated[1], unavailable);
}

fn bundle_from_environment() -> PathBuf {
    std::env::var_os("RUVIE_TEST_PLUGIN_BUNDLE")
        .map(PathBuf::from)
        .expect("RUVIE_TEST_PLUGIN_BUNDLE must name the independently built test bundle")
}

#[test]
#[ignore = "requires the independently built bundle from scripts/test-runtime-plugin.sh"]
fn standalone_runtime_effector_loads_describes_builds_and_invokes() {
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
            "effector",
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
        vec![("effector".to_string(), COMPONENT_ID.to_string())]
    );

    let descriptors = manager.get_runtime_plugin_descriptors();
    assert_eq!(descriptors.len(), 1);
    let component = &descriptors[0].descriptor.components[0];
    assert_eq!(component.id, COMPONENT_ID);
    let names = component
        .properties
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["opacity", "mode", "target"]);

    let instance = manager
        .create_effector_instance(COMPONENT_ID)
        .expect("descriptor-backed factory creates the runtime effector");
    assert!(instance.properties.get("opacity").is_some());
    assert!(instance.properties.get("mode").is_some());
    assert!(instance.properties.get("target").is_some());

    let project = Project::new("Runtime plugin test");
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
        .get_effector_plugin(COMPONENT_ID)
        .expect("runtime effector is registered in the normal repository");
    let output = plugin
        .convert(&context, &instance, 0.25)
        .expect("runtime effector produces an integrated config");
    assert!(matches!(
        output,
        EffectorConfig::Opacity {
            target_opacity,
            mode: OpacityMode::Multiply,
            target: EffectorTarget::Char,
        } if (target_opacity - 37.5).abs() < f32::EPSILON
    ));

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

    let serialized = serde_json::to_string(&instance).expect("instance serializes");
    drop(manager);
    let preserved: EffectorInstance =
        serde_json::from_str(&serialized).expect("unavailable plugin config stays loadable");
    assert_eq!(preserved, instance);
}
