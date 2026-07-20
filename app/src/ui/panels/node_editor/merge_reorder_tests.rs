use super::*;
use crate::test_support::generator_node;
use library::editor::project_service::GeneratorNodeRequest;
use library::model::frame::color::Color;
use library::model::project::{
    PortDataType, PortDefinition, PortExposure, PortSide, ProjectConnection, IMAGE_OUTPUT_PORT,
    MERGE_IMAGES_PORT,
};
use library::model::{BlendMode, Composition, PluginOperationContent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn three_layer_fixture() -> (Project, Uuid, Uuid, [Uuid; 3], [Uuid; 3]) {
    let mut project = Project::new("physical Merge reorder test");
    let (mut composition, mut track) = Composition::new("Main", 640, 360, 30.0, 4.0);
    composition.ui_position = [10.0, 20.0];
    composition.ui_size = [1500.0, 1000.0];
    track.ui_position = [90.0, 120.0];
    track.ui_size = [1250.0, 760.0];
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track);
    project.add_composition(composition);

    let mut clip = Clip::new("Layers", 0.0, 4.0);
    clip.ui_position = [180.0, 220.0];
    clip.ui_size = [980.0, 560.0];
    let clip_id = clip.id;
    project.add_clip(clip);
    assert!(project.attach_clip_to_track(track_id, clip_id).is_ok());

    let colors = [
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        Color {
            r: 0,
            g: 255,
            b: 0,
            a: 255,
        },
        Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        },
    ];
    let names = ["Back Red", "Middle Green", "Front Blue"];
    let mut source_ids = [Uuid::nil(); 3];
    for (index, (name, color)) in names.into_iter().zip(colors).enumerate() {
        let mut node = generator_node(name, GeneratorNodeRequest::Solid { color });
        node.ui_position = [310.0, 300.0 + index as f32 * 120.0];
        source_ids[index] = node.id;
        project.add_node(node);
        assert!(project
            .attach_node_to_container(NodeContainer::Clip(clip_id), source_ids[index])
            .is_ok());
    }

    let mut merge = Node::new_merge("Merge");
    merge.ui_position = [720.0, 350.0];
    let merge_id = merge.id;
    project.add_node(merge);
    assert!(project
        .attach_node_to_container(NodeContainer::Clip(clip_id), merge_id)
        .is_ok());
    assert!(project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .is_ok());

    let target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let mut connection_ids = [Uuid::nil(); 3];
    for (index, source_id) in source_ids.iter().copied().enumerate() {
        let result = project.connect_ports(
            PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            target.clone(),
        );
        assert!(result.is_ok());
        if let Ok(connection_id) = result {
            connection_ids[index] = connection_id;
        }
    }
    assert!(project
        .set_connection_blend_mode(connection_ids[0], BlendMode::Add)
        .is_ok());
    assert!(project
        .set_connection_blend_mode(connection_ids[1], BlendMode::Multiply)
        .is_ok());
    assert!(project
        .set_connection_blend_mode(connection_ids[2], BlendMode::Screen)
        .is_ok());
    (
        project,
        composition_id,
        merge_id,
        source_ids,
        connection_ids,
    )
}

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

#[derive(Default)]
struct RenderedMergeFrame {
    edits: Vec<QueuedNodeEdit>,
    layout_edit_count: usize,
    transform: egui::emath::TSTransform,
    edges: Vec<RenderedEdge>,
}

fn render_merge_frame(
    context: &egui::Context,
    project: &Project,
    composition_id: Uuid,
    state: &mut NodeEditorState,
    frame: usize,
    events: Vec<egui::Event>,
) -> RenderedMergeFrame {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
    let modifiers = events
        .iter()
        .find_map(|event| match event {
            egui::Event::MouseWheel { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    let mut result = RenderedMergeFrame::default();
    reset_test_rects();
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(frame as f64 / 60.0),
            events,
            modifiers,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let (mut snarl, containers) = build_snarl(project, composition_id);
                let mut navigation = None;
                let mut selection = None;
                let mut wire_context_request = None;
                let mut exclusions = Vec::new();
                let mut to_global = egui::emath::TSTransform::IDENTITY;
                let mut canvas_clip = ui.clip_rect();
                let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
                let locked_canvas_transform = state
                    .merge_layer_reorder
                    .as_ref()
                    .map(|gesture| gesture.canvas_transform);
                let mut viewer = ProjectNodeViewer {
                    project,
                    plugin_manager: None,
                    containers: &containers,
                    edits: &mut result.edits,
                    pending_navigation: &mut navigation,
                    pending_selection: &mut selection,
                    current_time: 0.0,
                    context_menu_exclusion_rects: &mut exclusions,
                    wire_context_request: &mut wire_context_request,
                    suppress_wire_connect: state.merge_layer_reorder.is_some(),
                    locked_canvas_transform,
                    to_global: &mut to_global,
                    canvas_clip: &mut canvas_clip,
                    rendered_ports: Arc::clone(&rendered_ports),
                    merge_layer_reorder: &mut state.merge_layer_reorder,
                    rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                };
                snarl.show(
                    &mut viewer,
                    &node_editor_snarl_style(),
                    egui::Id::new(("physical-merge-reorder", composition_id)),
                    ui,
                );
                drop(viewer);
                result.transform = to_global;
                result.layout_edit_count = collect_layout_edits(project, &snarl).len();
                result.edges = register_rendered_edges(project, &rendered_ports, canvas_clip, None);
            });
        },
    ));
    result
}

fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn merge_connections_project_to_distinct_pins_and_disconnect_by_identity() {
    let (mut project, composition_id, merge_id, source_ids, connection_ids) = three_layer_fixture();
    let slots = merge_input_slots(&project, merge_id);
    let connected = slots
        .iter()
        .filter_map(|slot| match &slot.role {
            MergeInputSlotRole::Connected(row) => Some(row.connection_id),
            MergeInputSlotRole::Canonical | MergeInputSlotRole::VacantImages => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(connected.len(), 3);
    assert!(matches!(
        slots.last().map(|slot| &slot.role),
        Some(MergeInputSlotRole::VacantImages)
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
    assert!(!project
        .connections
        .iter()
        .any(|connection| connection.id == connection_ids[1]));
    assert!(project
        .connections
        .iter()
        .any(|connection| connection.id == connection_ids[0]));
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
    assert!(project
        .attach_node_to_container(container, plugin_id)
        .is_ok());
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
    assert!(project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .all(|connection| !connection_supports_authored_blend(&project, connection)));

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
    assert!(project
        .connections
        .iter()
        .any(|connection| connection.from == first_from && connection.to == target));
    assert!(!project
        .connections
        .iter()
        .any(|connection| connection.from == second_from && connection.to == target));
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
    assert!(project
        .attach_node_to_container(container, plugin_id)
        .is_ok());

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

#[test]
fn dropping_an_existing_source_on_the_vacant_merge_slot_is_a_history_no_op() {
    let (mut project, composition_id, merge_id, source_ids, _) = three_layer_fixture();
    let initial = project.clone();
    let (snarl, _) = build_snarl(&project, composition_id);
    let merge_snarl_id = snarl
        .nodes_ids_data()
        .find_map(|(id, node)| (node.value == GraphItem::Node(merge_id)).then_some(id));
    let source_snarl_id = snarl
        .nodes_ids_data()
        .find_map(|(id, node)| (node.value == GraphItem::Node(source_ids[0])).then_some(id));
    let vacant_index = merge_input_slots(&project, merge_id).len().checked_sub(1);
    assert!(merge_snarl_id.is_some() && source_snarl_id.is_some() && vacant_index.is_some());
    let (Some(merge_snarl_id), Some(source_snarl_id), Some(vacant_index)) =
        (merge_snarl_id, source_snarl_id, vacant_index)
    else {
        return;
    };
    let edit = edit_for_wire(
        &project,
        &snarl,
        source_snarl_id,
        0,
        merge_snarl_id,
        vacant_index,
        true,
    );
    assert!(matches!(edit, Some(NodeEdit::Connect { .. })));
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    let mut state = NodeEditorState::default();
    let changed = apply_queued_node_edits(
        &mut project,
        edit.into_iter().map(QueuedNodeEdit::Atomic).collect(),
        &mut history,
        &mut state,
    );
    assert!(!changed);
    assert_eq!(project, initial);
    assert_eq!(history.undo_depth(), 1);
    assert_eq!(history.redo_depth(), 0);
}

#[test]
fn real_pointer_drag_reorders_first_to_last_once_and_preserves_wire_metadata() {
    let (mut project, composition_id, merge_id, _, connection_ids) = three_layer_fixture();
    let initial = project.clone();
    let initial_positions = project
        .nodes
        .values()
        .map(|node| (node.id, node.ui_position))
        .collect::<Vec<_>>();
    let original = connection_ids.map(|connection_id| {
        project
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .cloned()
    });
    assert!(original.iter().all(Option::is_some));

    let context = egui::Context::default();
    let mut state = NodeEditorState::default();
    let mut frame = 0;
    let mut settled_transform = egui::emath::TSTransform::IDENTITY;
    for _ in 0..6 {
        settled_transform = render_merge_frame(
            &context,
            &project,
            composition_id,
            &mut state,
            frame,
            Vec::new(),
        )
        .transform;
        frame += 1;
    }

    let first_handle_id = format!(
        "node_editor.merge_layer.drag_handle:{merge_id}:{}",
        connection_ids[0]
    );
    let zoom_anchor = test_rect(&first_handle_id).map(|rect| rect.center());
    assert!(zoom_anchor.is_some());
    let Some(zoom_anchor) = zoom_anchor else {
        return;
    };
    let command_modifiers = egui::Modifiers {
        command: true,
        mac_cmd: cfg!(target_os = "macos"),
        ..egui::Modifiers::NONE
    };
    let zoomed = render_merge_frame(
        &context,
        &project,
        composition_id,
        &mut state,
        frame,
        vec![
            egui::Event::PointerMoved(zoom_anchor),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -40.0),
                modifiers: command_modifiers,
            },
        ],
    )
    .transform;
    frame += 1;
    assert_ne!(zoomed.scaling, settled_transform.scaling);
    let mut settled_zoom = zoomed;
    for _ in 0..20 {
        settled_zoom = render_merge_frame(
            &context,
            &project,
            composition_id,
            &mut state,
            frame,
            Vec::new(),
        )
        .transform;
        frame += 1;
    }

    let pan_start = egui::pos2(1740.0, 1120.0);
    let pan_end = pan_start + egui::vec2(-48.0, -32.0);
    let mut panned = settled_zoom;
    for events in [
        vec![egui::Event::PointerMoved(pan_start)],
        vec![pointer_button(pan_start, true)],
        vec![egui::Event::PointerMoved(pan_end)],
        vec![pointer_button(pan_end, false)],
    ] {
        let rendered = render_merge_frame(
            &context,
            &project,
            composition_id,
            &mut state,
            frame,
            events,
        );
        assert_eq!(rendered.layout_edit_count, 0);
        assert!(rendered.edits.is_empty());
        panned = rendered.transform;
        frame += 1;
    }
    assert_ne!(panned.translation, settled_zoom.translation);

    let last_row_id = format!("node_editor.merge_layer:{merge_id}:{}", connection_ids[2]);
    let start = test_rect(&first_handle_id).map(|rect| rect.center());
    let last = test_rect(&last_row_id).map(|rect| rect.center());
    assert!(start.is_some() && last.is_some());
    let (Some(start), Some(last)) = (start, last) else {
        return;
    };

    let mut drag_transforms = Vec::new();
    for events in [
        vec![egui::Event::PointerMoved(start)],
        vec![pointer_button(start, true)],
        vec![egui::Event::PointerMoved(last)],
        vec![pointer_button(last, false)],
    ] {
        let rendered = render_merge_frame(
            &context,
            &project,
            composition_id,
            &mut state,
            frame,
            events,
        );
        drag_transforms.push(rendered.transform);
        assert_eq!(rendered.layout_edit_count, 0);
        if !rendered.edits.is_empty() {
            assert_eq!(rendered.edits.len(), 1);
            let mut history = HistoryManager::new();
            history.push_project_state(initial.clone());
            assert!(apply_queued_node_edits(
                &mut project,
                rendered.edits,
                &mut history,
                &mut state,
            ));
            assert_eq!(history.undo_depth(), 2);
            let edited = project.clone();
            assert_eq!(history.undo(&edited), Some(initial.clone()));
            assert_eq!(history.redo(&initial), Some(edited));
        }
        frame += 1;
    }
    assert!(
        state
            .merge_layer_reorder
            .as_ref()
            .is_some_and(|gesture| gesture.finished && gesture.target_index == Some(2)),
        "gesture after release: {:?}",
        state.merge_layer_reorder,
    );
    state.merge_layer_reorder = None;
    assert!(
        drag_transforms.iter().all(|transform| *transform == panned),
        "panned: {panned:?}; drag transforms: {drag_transforms:?}",
    );
    assert_eq!(
        project
            .nodes
            .values()
            .map(|node| (node.id, node.ui_position))
            .collect::<Vec<_>>(),
        initial_positions
    );

    let rows = merge_layer_rows(&project, merge_id);
    assert_eq!(
        rows.iter().map(|row| row.connection_id).collect::<Vec<_>>(),
        vec![connection_ids[1], connection_ids[2], connection_ids[0]]
    );
    for original in original.into_iter().flatten() {
        let current = project
            .connections
            .iter()
            .find(|connection| connection.id == original.id);
        assert!(current.is_some());
        if let Some(current) = current {
            assert_eq!(current.id, original.id);
            assert_eq!(current.from, original.from);
            assert_eq!(current.to, original.to);
            assert_eq!(current.blend_mode, original.blend_mode);
        }
    }

    let final_frame = render_merge_frame(
        &context,
        &project,
        composition_id,
        &mut state,
        frame,
        Vec::new(),
    );
    let mut merge_ends = final_frame
        .edges
        .iter()
        .filter_map(|edge| {
            edge.kind
                .connection_id()
                .filter(|connection_id| connection_ids.contains(connection_id))
                .map(|connection_id| (connection_id, edge.end.y))
        })
        .collect::<Vec<_>>();
    merge_ends.sort_by(|left, right| left.1.total_cmp(&right.1));
    assert_eq!(merge_ends.len(), 3);
    assert!(merge_ends.windows(2).all(|pair| pair[0].1 != pair[1].1));
    assert_eq!(
        merge_ends
            .iter()
            .map(|(connection_id, _)| *connection_id)
            .collect::<Vec<_>>(),
        vec![connection_ids[1], connection_ids[2], connection_ids[0]]
    );
}

#[test]
fn real_pointer_drag_outside_rows_cancels_without_project_or_layout_change() {
    let (project, composition_id, merge_id, _, connection_ids) = three_layer_fixture();
    let context = egui::Context::default();
    let mut state = NodeEditorState::default();
    let mut frame = 0;
    for _ in 0..5 {
        let _ = render_merge_frame(
            &context,
            &project,
            composition_id,
            &mut state,
            frame,
            Vec::new(),
        );
        frame += 1;
    }
    let handle = test_rect(&format!(
        "node_editor.merge_layer.drag_handle:{merge_id}:{}",
        connection_ids[1]
    ))
    .map(|rect| rect.center());
    assert!(handle.is_some());
    let Some(handle) = handle else {
        return;
    };
    let invalid = handle + egui::vec2(700.0, 420.0);
    let mut edits = Vec::new();
    for events in [
        vec![egui::Event::PointerMoved(handle)],
        vec![pointer_button(handle, true)],
        vec![egui::Event::PointerMoved(invalid)],
        vec![pointer_button(invalid, false)],
    ] {
        let rendered = render_merge_frame(
            &context,
            &project,
            composition_id,
            &mut state,
            frame,
            events,
        );
        assert_eq!(rendered.layout_edit_count, 0);
        edits.extend(rendered.edits);
        frame += 1;
    }
    assert!(edits.is_empty());
    assert!(state
        .merge_layer_reorder
        .as_ref()
        .is_some_and(|gesture| gesture.finished && gesture.target_index.is_none()));
    assert_eq!(
        merge_layer_rows(&project, merge_id)
            .iter()
            .map(|row| row.connection_id)
            .collect::<Vec<_>>(),
        connection_ids
    );
}

#[test]
fn single_layer_drag_handle_is_disabled_and_cannot_claim_pointer() {
    let (mut project, composition_id, merge_id, _, connection_ids) = three_layer_fixture();
    project
        .connections
        .retain(|connection| ![connection_ids[1], connection_ids[2]].contains(&connection.id));
    let context = egui::Context::default();
    let mut state = NodeEditorState::default();
    let mut frame = 0;
    for _ in 0..5 {
        let _ = render_merge_frame(
            &context,
            &project,
            composition_id,
            &mut state,
            frame,
            Vec::new(),
        );
        frame += 1;
    }
    let handle = test_rect(&format!(
        "node_editor.merge_layer.drag_handle:{merge_id}:{}",
        connection_ids[0]
    ))
    .map(|rect| rect.center());
    assert!(handle.is_some());
    let Some(handle) = handle else {
        return;
    };
    let destination = handle + egui::vec2(0.0, 120.0);
    let mut edits = Vec::new();
    for events in [
        vec![egui::Event::PointerMoved(handle)],
        vec![pointer_button(handle, true)],
        vec![egui::Event::PointerMoved(destination)],
        vec![pointer_button(destination, false)],
    ] {
        edits.extend(
            render_merge_frame(
                &context,
                &project,
                composition_id,
                &mut state,
                frame,
                events,
            )
            .edits,
        );
        frame += 1;
    }
    assert!(edits.is_empty());
    assert!(state.merge_layer_reorder.is_none());
    assert_eq!(merge_layer_rows(&project, merge_id).len(), 1);
}
