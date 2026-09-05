use super::*;

use crate::editor::timeline_editor_service::node_clip_conversion_tests::{small_service, time};
use crate::model::authoring::{ModuleDefinitionSharing, ShapeKind, ShapeSource};

fn converted_shape() -> (
    TimelineEditorService,
    PluginManager,
    TimelineItemId,
    ModuleInstanceId,
) {
    let plugins = PluginManager::default();
    let (service, track_id) = small_service("Structured Appearance");
    let fill = AppearanceOperationFactory::create(&plugins, "fill").expect("Fill");
    let stroke = AppearanceOperationFactory::create(&plugins, "stroke").expect("Stroke");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Rectangle".to_string(),
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: std::collections::HashMap::from([
                        ("width".to_string(), PropertyValue::from(40.0)),
                        ("height".to_string(), PropertyValue::from(24.0)),
                    ]),
                    appearance_operations: vec![fill, stroke],
                },
            },
            TimelineInterval::new(MediaTime::zero(), time(3)).expect("interval"),
            0,
        )
        .expect("Shape item");
    let conversion = service
        .convert_source_to_node_clip(&plugins, item_id)
        .expect("Node Clip conversion");
    (service, plugins, item_id, conversion.instance_id)
}

#[test]
fn recognizes_adds_reorders_and_removes_real_style_nodes_atomically() {
    let (service, plugins, item_id, instance_id) = converted_shape();
    let initial = service
        .node_clip_appearance_stack(item_id)
        .expect("stack")
        .expect("structured Appearance");
    assert_eq!(
        initial
            .operations
            .iter()
            .map(|entry| entry.component_id.as_str())
            .collect::<Vec<_>>(),
        ["fill", "stroke"]
    );

    let before_revision = service.revision().expect("revision");
    let (shadow_id, _) = service
        .add_node_clip_appearance_operation(&plugins, item_id, "drop_shadow", 1)
        .expect("add Drop Shadow");
    assert_eq!(
        service.revision().expect("revision").get(),
        before_revision.get() + 1
    );
    let added = service
        .node_clip_appearance_stack(item_id)
        .expect("stack")
        .expect("structured Appearance");
    assert_eq!(
        added
            .operations
            .iter()
            .map(|entry| entry.component_id.as_str())
            .collect::<Vec<_>>(),
        ["fill", "drop_shadow", "stroke"]
    );
    let project = service.snapshot().expect("after add");
    assert!(
        project.module_definitions[&added.definition_id]
            .graph
            .nodes
            .contains_key(&shadow_id)
    );
    assert!(!added.operations[1].parameter_ids.is_empty());

    service
        .reorder_node_clip_appearance_operation(item_id, shadow_id, 0)
        .expect("reorder Drop Shadow");
    let reordered = service
        .node_clip_appearance_stack(item_id)
        .expect("stack")
        .expect("structured Appearance");
    assert_eq!(
        reordered
            .operations
            .iter()
            .map(|entry| entry.component_id.as_str())
            .collect::<Vec<_>>(),
        ["drop_shadow", "fill", "stroke"]
    );

    let parameter_id = reordered.operations[0].parameter_ids[0];
    let default = service.snapshot().expect("project").module_definitions[&reordered.definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == parameter_id)
        .expect("published Drop Shadow parameter")
        .default_value
        .clone();
    service
        .set_module_parameter(instance_id, parameter_id, default.clone())
        .expect("override");
    service
        .upsert_module_parameter_keyframe(item_id, parameter_id, time(1), default, None)
        .expect("automation");
    let before_remove = service.snapshot().expect("before remove");

    service
        .remove_node_clip_appearance_operation(item_id, shadow_id)
        .expect("remove Drop Shadow");
    let after = service.snapshot().expect("after remove");
    let SourceRef::Module(invocation) = &after.items[&item_id].source else {
        panic!("converted Shape must remain a Node Clip");
    };
    assert!(
        !after.module_instances[&instance_id]
            .parameter_overrides
            .contains_key(&parameter_id)
    );
    assert!(!invocation.automation_tracks.contains_key(&parameter_id));
    assert!(
        !after.module_definitions[&reordered.definition_id]
            .graph
            .nodes
            .contains_key(&shadow_id)
    );

    service.undo().expect("Undo").expect("remove Undo");
    assert_eq!(
        service.snapshot().expect("restored").as_ref(),
        before_remove.as_ref()
    );
}

#[test]
fn instance_edit_copy_on_write_preserves_the_sibling_appearance() {
    let (service, plugins, item_id, _) = converted_shape();
    let (sibling_id, _) = service
        .duplicate_item(item_id, time(4), 1)
        .expect("duplicate");
    let before = service.snapshot().expect("before");
    let shared = service
        .node_clip_appearance_stack(item_id)
        .expect("stack")
        .expect("structured");
    assert_eq!(
        shared.definition_id,
        service
            .node_clip_appearance_stack(sibling_id)
            .expect("sibling stack")
            .expect("structured sibling")
            .definition_id
    );

    service
        .add_node_clip_appearance_operation(&plugins, item_id, "outer_glow", 1)
        .expect("add Outer Glow");
    let edited = service
        .node_clip_appearance_stack(item_id)
        .expect("stack")
        .expect("structured");
    let sibling = service
        .node_clip_appearance_stack(sibling_id)
        .expect("stack")
        .expect("structured");
    assert_ne!(edited.definition_id, sibling.definition_id);
    assert_eq!(edited.operations.len(), 3);
    assert_eq!(sibling.operations.len(), 2);
    let project = service.snapshot().expect("after");
    assert!(matches!(
        project.module_definitions[&edited.definition_id].sharing,
        ModuleDefinitionSharing::Private
    ));

    service.undo().expect("Undo").expect("one add Undo");
    assert_eq!(
        service.snapshot().expect("restored").as_ref(),
        before.as_ref()
    );
}

#[test]
fn arbitrary_style_topology_hides_the_facade_without_projecting_fake_state() {
    let (service, _, item_id, instance_id) = converted_shape();
    let stack = service
        .node_clip_appearance_stack(item_id)
        .expect("stack")
        .expect("structured");
    let project = service.snapshot().expect("project");
    let definition = &project.module_definitions[&stack.definition_id];
    let stack_node_id = definition
        .graph
        .nodes
        .values()
        .find_map(|node| match node.content() {
            NodeContent::NativeOperation(operation)
                if operation.catalog_id == crate::model::node::APPEARANCE_STACK_CATALOG_ID =>
            {
                Some(node.id)
            }
            _ => None,
        })
        .expect("Appearance Stack Node");
    let connection_id = definition
        .graph
        .connections
        .iter()
        .find(|connection| {
            connection.to.node_id == stack_node_id && connection.to.port == SHAPE_INPUT_PORT
        })
        .expect("Shape input")
        .id;
    drop(project);

    service
        .disconnect_instance_module_connection(instance_id, connection_id)
        .expect("customize topology");
    assert!(
        service
            .node_clip_appearance_stack(item_id)
            .expect("facade query")
            .is_none()
    );
}

#[test]
fn shared_style_consumer_hides_facade_before_a_structured_remove_can_delete_it() {
    let (service, _, item_id, instance_id) = converted_shape();
    let stack = service
        .node_clip_appearance_stack(item_id)
        .expect("stack")
        .expect("structured");
    let style_id = stack.operations[0].node_id;
    let extra_consumer = Node::new_merge("Independent Image branch");
    let extra_consumer_id = extra_consumer.id;
    service
        .add_instance_module_node(instance_id, extra_consumer)
        .expect("add independent consumer");
    service
        .connect_instance_module_ports(
            instance_id,
            ModulePortAddress {
                node_id: style_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: extra_consumer_id,
                port: crate::model::project::MERGE_IMAGES_PORT.to_string(),
            },
            0,
        )
        .expect("connect independent consumer");

    assert!(
        service
            .node_clip_appearance_stack(item_id)
            .expect("facade query")
            .is_none(),
        "a Style shared with arbitrary Image topology must not expose destructive structured controls"
    );
    assert!(
        service
            .remove_node_clip_appearance_operation(item_id, style_id)
            .is_err(),
        "structured removal must refuse a shared Style Node"
    );
    let project = service.snapshot().expect("project");
    let definition_id = project.module_instances[&instance_id].definition_id;
    assert!(
        project.module_definitions[&definition_id]
            .graph
            .nodes
            .contains_key(&style_id)
    );
}
