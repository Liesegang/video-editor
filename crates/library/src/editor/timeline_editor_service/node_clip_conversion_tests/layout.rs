use super::*;

#[test]
fn converted_property_nodes_use_persisted_widths_and_non_overlapping_columns() {
    let plugins = PluginManager::default();
    let operations = [
        "fill",
        "drop_shadow",
        "fill",
        "gradient_overlay",
        "pattern_overlay",
    ]
    .into_iter()
    .map(|component| {
        AppearanceOperationFactory::create(&plugins, component)
            .unwrap_or_else(|error| panic!("create {component}: {error}"))
    })
    .collect();
    let (service, track_id) = small_service("Appearance layout");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Appearance layout".to_string(),
            SourceRef::Text {
                text: "Layout".to_string(),
                appearance_operations: operations,
                ensemble_operations: Vec::new(),
            },
            interval(2),
            0,
        )
        .expect("Text fixture");
    let conversion = service
        .convert_source_to_node_clip(&plugins, item_id)
        .expect("convert Text");
    let project = service.snapshot().expect("converted Project");
    let definition = project
        .module_definitions
        .get(&conversion.definition_id)
        .expect("converted definition");
    let mut nodes = definition.graph.nodes.values().collect::<Vec<_>>();
    nodes.sort_by(|left, right| left.ui_position[0].total_cmp(&right.ui_position[0]));

    assert_eq!(
        nodes.len(),
        8,
        "Text, five operations, one Merge, and Output"
    );
    for node in nodes
        .iter()
        .filter(|node| node.properties().iter().next().is_some())
    {
        assert!(node.ui_size[0] >= crate::model::node::PROPERTY_NODE_UI_WIDTH);
    }
    for pair in nodes.windows(2) {
        let left = pair[0];
        let right = pair[1];
        assert!(
            left.ui_position[0] + left.ui_size[0] + crate::model::node::NODE_LAYOUT_COLUMN_GAP
                <= right.ui_position[0],
            "{} at {:?} overlaps {} at {:?}",
            left.name,
            left.ui_position,
            right.name,
            right.ui_position
        );
    }
}
