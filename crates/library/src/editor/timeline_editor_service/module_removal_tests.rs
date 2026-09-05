use std::collections::HashMap;
use std::path::PathBuf;

use super::*;
use crate::editor::build_authoring_e2e_fixture;
use crate::model::authoring::{ModuleDefinitionSharing, ModuleTemplateOrigin};

fn fixture() -> crate::editor::AuthoringE2eFixture {
    let media = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data/e2e_media");
    build_authoring_e2e_fixture(&media, &PluginManager::default()).expect("authoring fixture")
}

fn published_target_node(
    project: &AuthoringProject,
    definition_id: ModuleDefinitionId,
    parameter_id: PublishedParameterId,
) -> uuid::Uuid {
    project.module_definitions[&definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == parameter_id)
        .expect("published parameter")
        .target
        .node_id
}

fn reusable_fixture() -> (
    TimelineEditorService,
    ModuleDefinitionId,
    ModuleInstanceId,
    ModuleInstanceId,
    TimelineItemId,
    PublishedParameterId,
) {
    let base = fixture();
    let definition_id = base.info.module_definition_id;
    let first_instance_id = base.info.module_instance_id;
    let first_item_id = base.info.node_clip_item_id;
    let parameter_id = base.info.module_parameter_id;
    let mut project = (*base.service.snapshot().expect("fixture snapshot")).clone();
    project
        .module_definitions
        .get_mut(&definition_id)
        .expect("Module definition")
        .sharing = ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project);

    let first_item = project.items[&first_item_id].clone();
    let SourceRef::Module(first_invocation) = first_item.source else {
        panic!("fixture Node Clip");
    };
    let service = TimelineEditorService::new(project).expect("reusable fixture");
    let (_, second_instance_id, _) = service
        .place_module_item(
            definition_id,
            ModuleItemPlacement {
                track_id: first_item.track_id,
                name: "Second QA Node Clip".to_string(),
                output_id: first_invocation.output_id,
                interval: first_item.interval,
                layer: first_item.layer + 1,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .expect("second placement");
    let first_value = service
        .snapshot()
        .expect("placed snapshot")
        .module_instances[&first_instance_id]
        .parameter_overrides[&parameter_id]
        .clone();
    service
        .set_module_parameter(second_instance_id, parameter_id, first_value)
        .expect("second override");

    let clean_project = (*service.snapshot().expect("clean snapshot")).clone();
    (
        TimelineEditorService::new(clean_project).expect("clean reusable service"),
        definition_id,
        first_instance_id,
        second_instance_id,
        first_item_id,
        parameter_id,
    )
}

#[test]
fn private_node_deletion_cleans_published_dependents_and_undo_restores_exact_project() {
    let fixture = fixture();
    let service = fixture.service;
    let before = service.snapshot().expect("before");
    let node_id = published_target_node(
        before.as_ref(),
        fixture.info.module_definition_id,
        fixture.info.module_parameter_id,
    );
    drop(before);
    let before = service.snapshot().expect("before deletion");

    let (impact, definition_id, _) = service
        .remove_instance_module_nodes(fixture.info.module_instance_id, vec![node_id])
        .expect("delete published Node");

    assert_eq!(definition_id, fixture.info.module_definition_id);
    assert_eq!(impact.removed_parameter_overrides, 1);
    assert_eq!(impact.removed_automation_tracks, 1);
    assert_eq!(impact.removed_media_input_bindings, 0);
    let after = service.snapshot().expect("after deletion");
    assert!(
        !after.module_definitions[&definition_id]
            .graph
            .nodes
            .contains_key(&node_id)
    );
    assert!(
        after.module_definitions[&definition_id]
            .interface
            .parameters
            .is_empty()
    );
    assert!(
        after.module_instances[&fixture.info.module_instance_id]
            .parameter_overrides
            .is_empty()
    );
    let SourceRef::Module(invocation) = &after.items[&fixture.info.node_clip_item_id].source else {
        panic!("fixture Node Clip");
    };
    assert!(invocation.automation_tracks.is_empty());
    after.validate().expect("valid deletion");
    drop(after);

    service.undo().expect("undo").expect("deletion change");
    assert_eq!(
        service.snapshot().expect("restored").as_ref(),
        before.as_ref()
    );
}

#[test]
fn reusable_instance_deletion_is_copy_on_write_and_cleans_only_that_instance() {
    let (service, reusable_id, first_instance, second_instance, first_item, parameter_id) =
        reusable_fixture();
    let before = service.snapshot().expect("before");
    let node_id = published_target_node(before.as_ref(), reusable_id, parameter_id);

    let (impact, private_id, _) = service
        .remove_instance_module_nodes(first_instance, vec![node_id])
        .expect("copy-on-write deletion");

    assert_ne!(private_id, reusable_id);
    assert_eq!(impact.removed_parameter_overrides, 1);
    assert_eq!(impact.removed_automation_tracks, 1);
    let after = service.snapshot().expect("after deletion");
    assert_eq!(
        after.module_instances[&first_instance].definition_id,
        private_id
    );
    assert_eq!(
        after.module_instances[&second_instance].definition_id,
        reusable_id
    );
    assert!(
        !after.module_definitions[&private_id]
            .graph
            .nodes
            .contains_key(&node_id)
    );
    assert!(
        after.module_definitions[&reusable_id]
            .graph
            .nodes
            .contains_key(&node_id)
    );
    assert!(
        after.module_instances[&first_instance]
            .parameter_overrides
            .is_empty()
    );
    assert!(
        after.module_instances[&second_instance]
            .parameter_overrides
            .contains_key(&parameter_id)
    );
    let SourceRef::Module(invocation) = &after.items[&first_item].source else {
        panic!("first Node Clip");
    };
    assert!(invocation.automation_tracks.is_empty());
    after.validate().expect("valid copy-on-write deletion");
    drop(after);

    service.undo().expect("undo").expect("deletion change");
    assert_eq!(
        service.snapshot().expect("restored").as_ref(),
        before.as_ref()
    );
}

#[test]
fn shared_batch_deletion_cleans_every_instance_in_one_undoable_transaction() {
    let (service, reusable_id, first_instance, second_instance, _, parameter_id) =
        reusable_fixture();
    let before = service.snapshot().expect("before");
    let node_id = published_target_node(before.as_ref(), reusable_id, parameter_id);

    let edit = service
        .remove_shared_module_nodes(reusable_id, vec![node_id])
        .expect("shared deletion");

    assert_eq!(edit.affected_instance_count, 2);
    assert_eq!(edit.value.removed_parameter_overrides, 2);
    assert_eq!(edit.value.removed_automation_tracks, 1);
    assert_eq!(edit.value.removed_media_input_bindings, 0);
    let after = service.snapshot().expect("after deletion");
    assert!(
        !after.module_definitions[&reusable_id]
            .graph
            .nodes
            .contains_key(&node_id)
    );
    assert!(
        after.module_instances[&first_instance]
            .parameter_overrides
            .is_empty()
    );
    assert!(
        after.module_instances[&second_instance]
            .parameter_overrides
            .is_empty()
    );
    assert!(after.items.values().all(|item| match &item.source {
        SourceRef::Module(invocation) => !invocation.automation_tracks.contains_key(&parameter_id),
        _ => true,
    }));
    after.validate().expect("valid shared deletion");
    drop(after);

    service.undo().expect("undo").expect("deletion change");
    assert_eq!(
        service.snapshot().expect("restored").as_ref(),
        before.as_ref()
    );
}

#[test]
fn invalid_batch_is_rejected_before_mutating_the_definition() {
    let fixture = fixture();
    let service = fixture.service;
    let before = service.snapshot().expect("before");
    let node_id = published_target_node(
        before.as_ref(),
        fixture.info.module_definition_id,
        fixture.info.module_parameter_id,
    );

    let error = service
        .remove_instance_module_nodes(fixture.info.module_instance_id, vec![node_id, node_id])
        .expect_err("duplicate selection");

    assert!(error.to_string().contains("duplicate IDs"));
    assert_eq!(
        service.snapshot().expect("unchanged").as_ref(),
        before.as_ref()
    );
}

#[test]
fn multi_node_selection_is_one_edit_and_one_undo_restores_every_node() {
    let fixture = fixture();
    let service = fixture.service;
    let instance_id = fixture.info.module_instance_id;
    let definition_id = fixture.info.module_definition_id;
    let initial = service.snapshot().expect("fixture");
    let first = published_target_node(&initial, definition_id, fixture.info.module_parameter_id);
    let mut extra = initial.module_definitions[&definition_id].graph.nodes[&first].clone();
    extra.id = uuid::Uuid::new_v4();
    let second = extra.id;
    service
        .add_instance_module_node(instance_id, extra)
        .expect("second Node");
    let before = service.snapshot().expect("before batch");
    let revision = service.revision().expect("revision");

    service
        .remove_instance_module_nodes(instance_id, vec![first, second])
        .expect("delete selection");
    let after = service.snapshot().expect("after batch");
    assert_eq!(
        service.revision().expect("revision").get(),
        revision.get() + 1
    );
    assert!(![first, second].iter().any(|id| {
        after.module_definitions[&definition_id]
            .graph
            .nodes
            .contains_key(id)
    }));
    after.validate().expect("valid batch");
    service.undo().expect("Undo").expect("one batch edit");
    assert_eq!(
        service.snapshot().expect("restored").as_ref(),
        before.as_ref()
    );
    service.redo().expect("Redo").expect("one batch edit");
    assert_eq!(
        service.snapshot().expect("removed again").as_ref(),
        after.as_ref()
    );
}

#[test]
fn batch_containing_required_output_rolls_back_all_processing_node_deletions() {
    let fixture = fixture();
    let service = fixture.service;
    let before = service.snapshot().expect("before");
    let definition_id = fixture.info.module_definition_id;
    let processing =
        published_target_node(&before, definition_id, fixture.info.module_parameter_id);
    let output = before.module_definitions[&definition_id]
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                crate::model::node::NodeContent::ModuleOutput(_)
            )
        })
        .expect("required Output")
        .id;
    let revision = service.revision().expect("revision");
    let error = service
        .remove_instance_module_nodes(fixture.info.module_instance_id, vec![processing, output])
        .expect_err("required Output cannot be deleted");
    assert!(error.to_string().contains("required render terminal"));
    assert_eq!(service.revision().expect("revision"), revision);
    assert_eq!(
        service.snapshot().expect("unchanged").as_ref(),
        before.as_ref()
    );
}
