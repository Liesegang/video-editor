use super::merge_reorder_tests::three_layer_fixture;
use super::*;
use library::model::PluginOperationContent;
use library::model::project::{
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortDefinition, PortExposure, PortSide, ProjectConnection,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn variadic_images_plugin_node() -> Option<Node> {
    let content = PluginOperationContent {
        category: "test".to_string(),
        component_id: "variadic-images".to_string(),
        operation: "test.variadic-images.v1".to_string(),
        declared_ports: vec![
            PortDefinition::input(MERGE_IMAGES_PORT, "Images", PortDataType::Image).variadic(),
            PortDefinition::output(
                IMAGE_OUTPUT_PORT,
                "Image",
                PortDataType::Image,
                PortSide::Right,
                PortExposure::Graph,
            ),
            PortDefinition::output(
                "value",
                "Value",
                PortDataType::Any,
                PortSide::Right,
                PortExposure::Graph,
            ),
        ],
    };
    let mut serialized = serde_json::to_value(Node::new_merge("Variadic Images Plugin")).ok()?;
    *serialized.get_mut("content")? = serde_json::json!({
        "type": "PluginOperation",
        "data": content,
    });
    serde_json::from_value(serialized).ok()
}

#[test]
fn merge_connections_project_to_distinct_pins_and_disconnect_by_identity() {
    let (mut project, composition_id, merge_id, source_ids, connection_ids) = three_layer_fixture();
    let slots = merge_input_slots(&project, merge_id);
    let connected = slots
        .iter()
        .filter_map(|slot| match &slot.role {
            MergeInputSlotRole::Connected(row) => Some(row.connection_id),
            MergeInputSlotRole::Canonical | MergeInputSlotRole::Vacant(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(connected.len(), 3);
    assert_eq!(
        connected,
        connection_ids.iter().rev().copied().collect::<Vec<_>>()
    );
    assert!(matches!(
        slots.last().map(|slot| &slot.role),
        Some(MergeInputSlotRole::Vacant(NativeVariadicMergeKind::Image))
    ));

    let (snarl, _) = build_snarl(&project, composition_id);
    let merge_snarl_id = snarl
        .nodes_ids_data()
        .find_map(|(id, node)| (node.value == GraphItem::Node(merge_id)).then_some(id));
    assert!(merge_snarl_id.is_some());
    let Some(merge_snarl_id) = merge_snarl_id else {
        return;
    };
    let mut physical_indices = snarl
        .wires()
        .filter_map(|(_, input)| (input.node == merge_snarl_id).then_some(input.input))
        .collect::<Vec<_>>();
    physical_indices.sort_unstable();
    physical_indices.dedup();
    assert_eq!(physical_indices.len(), 3);

    let selected_index = merge_input_index_for_connection(&project, merge_id, connection_ids[1]);
    assert!(selected_index.is_some());
    let source_snarl_id = snarl
        .nodes_ids_data()
        .find_map(|(id, node)| (node.value == GraphItem::Node(source_ids[1])).then_some(id));
    assert!(source_snarl_id.is_some());
    let (Some(source_snarl_id), Some(selected_index)) = (source_snarl_id, selected_index) else {
        return;
    };
    let edit = edit_for_wire(
        &project,
        &snarl,
        source_snarl_id,
        0,
        merge_snarl_id,
        selected_index,
        false,
    );
    assert!(matches!(
        edit,
        Some(NodeEdit::DisconnectConnection { connection_id }) if connection_id == connection_ids[1]
    ));
    assert!(apply_edit(
        &mut project,
        NodeEdit::DisconnectConnection {
            connection_id: connection_ids[1],
        },
    ));
    assert!(
        !project
            .connections
            .iter()
            .any(|connection| connection.id == connection_ids[1])
    );
    assert!(
        project
            .connections
            .iter()
            .any(|connection| connection.id == connection_ids[0])
    );
}

#[test]
fn non_merge_variadic_images_keeps_one_generic_pin_and_disconnects_by_address() {
    let (mut project, composition_id, merge_id, source_ids, _) = three_layer_fixture();
    let plugin = variadic_images_plugin_node();
    assert!(plugin.is_some());
    let Some(plugin) = plugin else {
        return;
    };
    let plugin_id = plugin.id;
    let container = project.find_node_container(merge_id);
    assert!(container.is_some());
    let Some(container) = container else {
        return;
    };
    project.add_node(plugin);
    assert!(
        project
            .attach_node_to_container(container, plugin_id)
            .is_ok()
    );
    let target = PortAddress::new(PortOwner::Node(plugin_id), MERGE_IMAGES_PORT);
    let first_from = PortAddress::new(PortOwner::Node(source_ids[0]), IMAGE_OUTPUT_PORT);
    let second_from = PortAddress::new(PortOwner::Node(source_ids[1]), IMAGE_OUTPUT_PORT);
    let first_connection = project.connect_ports(first_from.clone(), target.clone());
    let second_connection = project.connect_ports(second_from.clone(), target.clone());
    assert!(first_connection.is_ok() && second_connection.is_ok());
    assert_eq!(merge_images_target_node_id(&project, &target), None);
    let plugin_slots = merge_input_slots(&project, plugin_id);
    assert_eq!(plugin_slots.len(), 1);
    assert!(matches!(
        plugin_slots.first().map(|slot| &slot.role),
        Some(MergeInputSlotRole::Canonical)
    ));
    assert!(
        project
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .all(|connection| !connection_supports_authored_blend(&project, connection))
    );

    let (snarl, _) = build_snarl(&project, composition_id);
    let plugin_snarl_id = snarl
        .nodes_ids_data()
        .find_map(|(id, node)| (node.value == GraphItem::Node(plugin_id)).then_some(id));
    let second_source_snarl_id = snarl
        .nodes_ids_data()
        .find_map(|(id, node)| (node.value == GraphItem::Node(source_ids[1])).then_some(id));
    assert!(plugin_snarl_id.is_some() && second_source_snarl_id.is_some());
    let (Some(plugin_snarl_id), Some(second_source_snarl_id)) =
        (plugin_snarl_id, second_source_snarl_id)
    else {
        return;
    };
    let mut target_indices = snarl
        .wires()
        .filter_map(|(_, input)| (input.node == plugin_snarl_id).then_some(input.input))
        .collect::<Vec<_>>();
    target_indices.sort_unstable();
    assert_eq!(target_indices, vec![0, 0]);

    let edit = edit_for_wire(
        &project,
        &snarl,
        second_source_snarl_id,
        0,
        plugin_snarl_id,
        0,
        false,
    );
    assert!(matches!(
        &edit,
        Some(NodeEdit::Disconnect { from, to }) if from == &second_from && to == &target
    ));
    assert!(edit.is_some_and(|edit| apply_edit(&mut project, edit)));
    assert!(
        project
            .connections
            .iter()
            .any(|connection| connection.from == first_from && connection.to == target)
    );
    assert!(
        !project
            .connections
            .iter()
            .any(|connection| connection.from == second_from && connection.to == target)
    );
}

#[test]
fn physical_merge_endpoint_identity_is_independent_of_authored_blend_support() {
    let (mut project, _, merge_id, _, _) = three_layer_fixture();
    let plugin = variadic_images_plugin_node();
    assert!(plugin.is_some());
    let Some(plugin) = plugin else {
        return;
    };
    let plugin_id = plugin.id;
    let container = project.find_node_container(merge_id);
    assert!(container.is_some());
    let Some(container) = container else {
        return;
    };
    project.add_node(plugin);
    assert!(
        project
            .attach_node_to_container(container, plugin_id)
            .is_ok()
    );

    let any_from = PortAddress::new(PortOwner::Node(plugin_id), "value");
    let merge_target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    // `Any` graph outputs are not newly authorable, but directly loaded or
    // forward-version Projects can still carry one. Endpoint projection must
    // be target-owned and must not silently fall back to the vacant Merge pin.
    let any_connection = ProjectConnection::new(any_from.clone(), merge_target.clone(), 3);
    let any_connection_id = any_connection.id;
    project.connections.push(any_connection);
    let any_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == any_connection_id);
    assert!(any_connection.is_some_and(|connection| {
        merge_images_target_node_id(&project, &connection.to) == Some(merge_id)
            && !connection_supports_authored_blend(&project, connection)
    }));

    let source_rect = egui::Rect::from_center_size(egui::pos2(40.0, 60.0), egui::vec2(8.0, 8.0));
    let exact_rect = egui::Rect::from_center_size(egui::pos2(240.0, 100.0), egui::vec2(8.0, 8.0));
    let vacant_rect = egui::Rect::from_center_size(egui::pos2(240.0, 180.0), egui::vec2(8.0, 8.0));
    let rendered_ports = Arc::new(Mutex::new(HashMap::from([
        (
            RenderedPortKey {
                address: any_from,
                direction: PortDirection::Output,
                connection_id: None,
            },
            source_rect,
        ),
        (
            RenderedPortKey {
                address: merge_target.clone(),
                direction: PortDirection::Input,
                connection_id: Some(any_connection_id),
            },
            exact_rect,
        ),
        (
            RenderedPortKey {
                address: merge_target,
                direction: PortDirection::Input,
                connection_id: None,
            },
            vacant_rect,
        ),
    ])));
    let rendered = register_rendered_edges(&project, &rendered_ports, egui::Rect::EVERYTHING, None);
    let any_edge = rendered.iter().find(|edge| {
        edge.kind
            == (RenderedEdgeKind::ProjectConnection {
                connection_id: any_connection_id,
            })
    });
    assert_eq!(any_edge.map(|edge| edge.end), Some(exact_rect.center()));
}
