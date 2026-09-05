use super::*;

use crate::editor::timeline_editor_service::node_clip_conversion_tests::{small_service, time};
use crate::model::authoring::ModuleDefinitionSharing;

fn converted_text_stack() -> (
    TimelineEditorService,
    PluginManager,
    TimelineItemId,
    ModuleInstanceId,
) {
    let plugins = PluginManager::default();
    let (service, track_id) = small_service("Structured Text Ensemble");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Title".to_string(),
            SourceRef::Text {
                text: "Structured".to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(MediaTime::zero(), time(3)).unwrap(),
            0,
        )
        .unwrap();
    service
        .add_appearance_operation(&plugins, item_id, "fill", 0)
        .unwrap();
    service
        .add_appearance_operation(&plugins, item_id, "stroke", 1)
        .unwrap();
    service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "opacity",
        )
        .unwrap();
    service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Decorator,
            "backplate",
        )
        .unwrap();
    let conversion = service
        .convert_source_to_node_clip(&plugins, item_id)
        .unwrap();
    (service, plugins, item_id, conversion.instance_id)
}

#[test]
fn recognizes_converted_text_and_adds_in_phase_order_with_one_undo() {
    let (service, plugins, item_id, _) = converted_text_stack();
    let before = service.snapshot().unwrap();
    let before_stack = service
        .node_clip_text_ensemble_stack(item_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        before_stack
            .operations
            .iter()
            .map(|entry| entry.component_id.as_str())
            .collect::<Vec<_>>(),
        vec!["opacity", "backplate"]
    );

    let (step_delay_id, _) = service
        .add_node_clip_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "step_delay",
        )
        .unwrap();
    let after_stack = service
        .node_clip_text_ensemble_stack(item_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        after_stack
            .operations
            .iter()
            .map(|entry| entry.component_id.as_str())
            .collect::<Vec<_>>(),
        vec!["opacity", "step_delay", "backplate"]
    );
    let added = after_stack
        .operations
        .iter()
        .find(|entry| entry.node_id == step_delay_id)
        .unwrap();
    assert!(!added.parameter_ids.is_empty());
    let project = service.snapshot().unwrap();
    let definition = &project.module_definitions[&after_stack.definition_id];
    let fill_parameter = definition
        .interface
        .parameters
        .iter()
        .position(|parameter| parameter.target.node_id == after_stack.appearance_anchor_node_id)
        .unwrap();
    assert!(added.parameter_ids.iter().all(|parameter_id| {
        definition
            .interface
            .parameters
            .iter()
            .position(|parameter| parameter.id == *parameter_id)
            .is_some_and(|position| position < fill_parameter)
    }));

    service.undo().unwrap().expect("one structured add undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn remove_cleans_instance_values_and_timeline_automation_atomically() {
    let (service, plugins, item_id, instance_id) = converted_text_stack();
    let (step_delay_id, _) = service
        .add_node_clip_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "step_delay",
        )
        .unwrap();
    let stack = service
        .node_clip_text_ensemble_stack(item_id)
        .unwrap()
        .unwrap();
    let added = stack
        .operations
        .iter()
        .find(|entry| entry.node_id == step_delay_id)
        .unwrap();
    let parameter_id = added.parameter_ids[0];
    let definition = service.snapshot().unwrap().module_definitions[&stack.definition_id].clone();
    let default = definition
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == parameter_id)
        .unwrap()
        .default_value
        .clone();
    service
        .set_module_parameter(instance_id, parameter_id, default.clone())
        .unwrap();
    service
        .upsert_module_parameter_keyframe(item_id, parameter_id, time(1), default, None)
        .unwrap();
    let before_remove = service.snapshot().unwrap();

    service
        .remove_node_clip_text_ensemble_operation(item_id, step_delay_id)
        .unwrap();
    let after = service.snapshot().unwrap();
    assert!(
        !after.module_definitions[&stack.definition_id]
            .graph
            .nodes
            .contains_key(&step_delay_id)
    );
    assert!(
        !after.module_definitions[&stack.definition_id]
            .interface
            .parameters
            .iter()
            .any(|parameter| parameter.id == parameter_id)
    );
    assert!(
        !after.module_instances[&instance_id]
            .parameter_overrides
            .contains_key(&parameter_id)
    );
    let SourceRef::Module(invocation) = &after.items[&item_id].source else {
        panic!("converted Text must remain a Node Clip");
    };
    assert!(!invocation.automation_tracks.contains_key(&parameter_id));

    service.undo().unwrap().expect("one structured remove undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), before_remove.as_ref());
}

#[test]
fn reorder_updates_topology_and_interface_order_with_one_undo() {
    let (service, plugins, item_id, _) = converted_text_stack();
    let (step_delay_id, _) = service
        .add_node_clip_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "step_delay",
        )
        .unwrap();
    let before = service.snapshot().unwrap();
    let before_stack = service
        .node_clip_text_ensemble_stack(item_id)
        .unwrap()
        .unwrap();
    let first_position = before.module_definitions[&before_stack.definition_id]
        .graph
        .nodes[&before_stack.operations[0].node_id]
        .ui_position;

    service
        .reorder_node_clip_text_ensemble_operation(item_id, step_delay_id, 0)
        .unwrap();
    let stack = service
        .node_clip_text_ensemble_stack(item_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        stack
            .operations
            .iter()
            .map(|entry| entry.component_id.as_str())
            .collect::<Vec<_>>(),
        vec!["step_delay", "opacity", "backplate"]
    );
    let after_reorder = service.snapshot().unwrap();
    let definition = &after_reorder.module_definitions[&stack.definition_id];
    assert_eq!(
        definition.graph.nodes[&step_delay_id].ui_position,
        first_position
    );
    let interface_nodes = definition
        .interface
        .parameters
        .iter()
        .filter_map(|parameter| {
            stack
                .operations
                .iter()
                .any(|entry| entry.node_id == parameter.target.node_id)
                .then_some(parameter.target.node_id)
        })
        .fold(Vec::new(), |mut nodes, node_id| {
            if nodes.last() != Some(&node_id) {
                nodes.push(node_id);
            }
            nodes
        });
    assert_eq!(
        interface_nodes,
        stack
            .operations
            .iter()
            .map(|entry| entry.node_id)
            .collect::<Vec<_>>()
    );

    service
        .undo()
        .unwrap()
        .expect("one structured reorder undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn arbitrary_graph_edit_hides_structured_facade_without_projecting_fake_state() {
    let (service, _, item_id, instance_id) = converted_text_stack();
    let stack = service
        .node_clip_text_ensemble_stack(item_id)
        .unwrap()
        .unwrap();
    let project = service.snapshot().unwrap();
    let definition = &project.module_definitions[&stack.definition_id];
    let first_link = definition
        .graph
        .connections
        .iter()
        .find(|connection| {
            connection.from.node_id == stack.text_node_id
                && connection.to.node_id == stack.operations[0].node_id
        })
        .unwrap()
        .id;
    drop(project);

    service
        .disconnect_instance_module_connection(instance_id, first_link)
        .unwrap();
    assert!(
        service
            .node_clip_text_ensemble_stack(item_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn instance_edit_copy_on_write_leaves_the_sibling_stack_unchanged() {
    let (service, plugins, item_id, _) = converted_text_stack();
    let (sibling_id, _) = service.duplicate_item(item_id, time(4), 1).unwrap();
    let before = service.snapshot().unwrap();
    let before_item_stack = service
        .node_clip_text_ensemble_stack(item_id)
        .unwrap()
        .unwrap();
    let before_sibling_stack = service
        .node_clip_text_ensemble_stack(sibling_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        before_item_stack.definition_id,
        before_sibling_stack.definition_id
    );

    service
        .add_node_clip_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "step_delay",
        )
        .unwrap();
    let item_stack = service
        .node_clip_text_ensemble_stack(item_id)
        .unwrap()
        .unwrap();
    let sibling_stack = service
        .node_clip_text_ensemble_stack(sibling_id)
        .unwrap()
        .unwrap();
    assert_ne!(item_stack.definition_id, sibling_stack.definition_id);
    assert_eq!(
        item_stack
            .operations
            .iter()
            .map(|entry| entry.component_id.as_str())
            .collect::<Vec<_>>(),
        vec!["opacity", "step_delay", "backplate"]
    );
    assert_eq!(
        sibling_stack
            .operations
            .iter()
            .map(|entry| entry.component_id.as_str())
            .collect::<Vec<_>>(),
        vec!["opacity", "backplate"]
    );
    let after = service.snapshot().unwrap();
    assert!(matches!(
        after.module_definitions[&item_stack.definition_id].sharing,
        ModuleDefinitionSharing::Private
    ));
    assert!(matches!(
        after.module_definitions[&sibling_stack.definition_id].sharing,
        ModuleDefinitionSharing::Private
    ));

    service.undo().unwrap().expect("one copy-on-write add undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn failed_structured_mutation_rolls_back_graph_interface_and_instance_identity() {
    let (service, plugins, item_id, _) = converted_text_stack();
    let stack = service
        .node_clip_text_ensemble_stack(item_id)
        .unwrap()
        .unwrap();
    let mut project = service.snapshot().unwrap().as_ref().clone();
    project
        .module_definitions
        .get_mut(&stack.definition_id)
        .unwrap()
        .interface_version = u64::MAX;
    let service = TimelineEditorService::new(project).unwrap();
    let before = service.snapshot().unwrap();

    let error = service
        .add_node_clip_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "step_delay",
        )
        .unwrap_err();
    assert!(error.to_string().contains("interface version overflow"));
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
    assert!(service.undo().unwrap().is_none());
}
