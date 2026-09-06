use std::collections::HashMap;

use super::*;
use crate::model::authoring::{ModuleDefinitionSharing, ModuleTemplateOrigin};
use crate::model::node::native_node_descriptor;

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

#[test]
fn unsupported_style_bypass_rolls_back_copy_on_write_and_history_atomically() {
    let plugins = PluginManager::default();
    let setup = TimelineEditorService::create_default("Style bypass guard").expect("service");
    let setup_project = setup.snapshot().expect("setup Project");
    let track_id = setup_project.timelines[&setup_project.root_timeline_id].track_order[0];
    drop(setup_project);

    let (mut definition, output_id) = ModuleDefinition::new_image(
        "Reusable Style",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    let style = plugins
        .create_style_operation_node("fill")
        .expect("Fill Style Node");
    assert!(!style.supports_bypass());
    let style_id = style.id;
    let style_name = style.name.clone();
    definition.graph.nodes.insert(style_id, style);
    definition.topology_revision += 1;
    definition.validate().expect("valid reusable Module");
    let definition_id = definition.id;
    setup
        .add_module_definition(definition)
        .expect("add reusable Module");
    let (_, instance_id, _) = setup
        .place_module_item(
            definition_id,
            ModuleItemPlacement {
                track_id,
                name: "Style Node Clip".to_string(),
                output_id,
                interval: TimelineInterval::new(seconds(0), seconds(2)).expect("interval"),
                layer: 0,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .expect("place reusable Module");

    // Start from a clean AuthoringSession so an erroneous failed-edit history
    // entry cannot hide behind fixture construction commands.
    let service = TimelineEditorService::new(
        setup
            .snapshot()
            .expect("configured Project")
            .as_ref()
            .clone(),
    )
    .expect("clean service");
    let before = service.snapshot().expect("before rejected edit");
    let revision = service.revision().expect("revision");
    assert!(!service.can_undo().expect("history"));

    let error = service
        .set_instance_module_node_state(instance_id, style_id, style_name, true, true)
        .expect_err("Fill cannot bypass a Style value");

    assert!(error.to_string().contains("cannot be bypassed"), "{error}");
    assert_eq!(
        service.snapshot().expect("rolled back").as_ref(),
        before.as_ref()
    );
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert!(!service.can_undo().expect("unchanged history"));
    let after = service.snapshot().expect("COW rollback");
    assert_eq!(
        after.module_definitions.len(),
        before.module_definitions.len()
    );
    assert_eq!(
        after.module_instances[&instance_id].definition_id, definition_id,
        "rejected edit must not leave an orphan private COW definition"
    );
}

#[test]
fn point_store_creation_chooses_unique_attribute_names() {
    let setup = TimelineEditorService::create_default("Point store names").expect("service");
    let project = setup.snapshot().expect("Project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (definition, output_id) =
        ModuleDefinition::new_image("Point Stores", ModuleDefinitionSharing::Private);
    let definition_id = definition.id;
    let (_, instance_id, _) = setup
        .create_private_module_item(
            definition,
            ModuleItemPlacement {
                track_id,
                name: "Point Stores".to_string(),
                output_id,
                interval: TimelineInterval::new(seconds(0), seconds(2)).expect("interval"),
                layer: 0,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .expect("placement");
    let descriptor = native_node_descriptor("native.point.store-number-attribute")
        .expect("Store Number Attribute descriptor");
    let first = descriptor.create_detached_node().expect("first Store Node");
    let second = descriptor
        .create_detached_node()
        .expect("second Store Node");
    let (first_id, _, _) = setup
        .add_instance_module_node(instance_id, first)
        .expect("first Store Node");
    let (second_id, _, _) = setup
        .add_instance_module_node(instance_id, second)
        .expect("second Store Node");

    let project = setup.snapshot().expect("renamed Point Stores");
    let definition = &project.module_definitions[&definition_id];
    assert_eq!(
        definition.graph.nodes[&first_id].name,
        "Store Number Attribute"
    );
    assert_eq!(
        definition.graph.nodes[&second_id].name,
        "Store Number Attribute 2"
    );
}
