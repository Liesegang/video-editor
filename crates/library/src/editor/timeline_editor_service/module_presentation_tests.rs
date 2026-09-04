use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::core::render_plan::RenderPlanCache;
use crate::model::authoring::{
    MediaTime, ModuleDefinition, ModuleDefinitionSharing, ModuleTemplateOrigin, TimelineInterval,
};

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).unwrap()
}

#[test]
fn presentation_edit_preserves_shared_identity_and_reuses_compiled_definition() {
    let service = TimelineEditorService::create_default("Module Presentation").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (definition, output_id) = ModuleDefinition::new_image(
        "Reusable",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    let definition_id = definition.id;
    let topology_revision = definition.topology_revision;
    let node_id = *definition.graph.nodes.keys().next().unwrap();
    let initial_presentation = {
        let node = &definition.graph.nodes[&node_id];
        (node.ui_position, node.ui_size, node.ui_collapsed)
    };
    service.add_module_definition(definition).unwrap();
    let placement = || ModuleItemPlacement {
        track_id,
        name: "Node Clip".to_string(),
        output_id,
        interval: TimelineInterval::new(seconds(0), seconds(2)).unwrap(),
        layer: 0,
        parameter_overrides: HashMap::new(),
        input_bindings: HashMap::new(),
    };
    let (_, first_instance, _) = service
        .place_module_item(definition_id, placement())
        .unwrap();
    let (_, second_instance, _) = service
        .place_module_item(definition_id, placement())
        .unwrap();
    let before = service.snapshot().unwrap();
    let mut cache = RenderPlanCache::default();
    let (first_plan, _) = cache.compile(before.as_ref()).unwrap();

    let (edited_id, changes) = service
        .set_instance_module_node_presentations(
            first_instance,
            vec![ModuleNodePresentationUpdate {
                node_id,
                position: [111.0, 222.0],
                size: [333.0, 144.0],
                collapsed: true,
            }],
        )
        .unwrap();

    assert_eq!(edited_id, definition_id);
    assert!(changes.invalidations.is_empty());
    let changed = service.snapshot().unwrap();
    assert_eq!(
        changed.module_instances[&first_instance].definition_id,
        definition_id
    );
    assert_eq!(
        changed.module_instances[&second_instance].definition_id,
        definition_id
    );
    assert_eq!(
        changed.module_definitions[&definition_id].topology_revision,
        topology_revision
    );
    let node = &changed.module_definitions[&definition_id].graph.nodes[&node_id];
    assert_eq!(
        (node.ui_position, node.ui_size, node.ui_collapsed),
        ([111.0, 222.0], [333.0, 144.0], true)
    );
    let (second_plan, stats) = cache.compile(changed.as_ref()).unwrap();
    assert_eq!(stats.compiled_definitions, 0);
    assert_eq!(stats.reused_definitions, 1);
    assert!(Arc::ptr_eq(
        &first_plan.module_definitions[&definition_id],
        &second_plan.module_definitions[&definition_id]
    ));
    drop(changed);

    service.undo().unwrap().expect("undo presentation");
    let undone = service.snapshot().unwrap();
    let node = &undone.module_definitions[&definition_id].graph.nodes[&node_id];
    assert_eq!(
        (node.ui_position, node.ui_size, node.ui_collapsed),
        initial_presentation
    );
}
