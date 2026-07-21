use super::*;

#[test]
fn node_enabled_context_command_is_atomic_and_undoable() {
    let (mut project, _, _, _, node_id, _) = fixture();
    let initial = project.clone();
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    let mut state = NodeEditorState::default();
    assert!(apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Atomic(NodeEdit::SetEnabled {
            node_id,
            enabled: false,
        })],
        &mut history,
        &mut state,
    ));
    assert!(!project.get_node(node_id).unwrap().enabled);
    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);
}

#[test]
fn node_bypass_context_command_is_capability_checked_atomic_and_undoable() {
    let (mut project, _, _, clip_id, unsupported_id, _) = fixture();
    let bypassed = Node::new_add("Bypassed Add");
    let bypassed_id = bypassed.id;
    project.add_node(bypassed);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), bypassed_id)
        .unwrap();
    let initial = project.clone();
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    let mut state = NodeEditorState::default();

    assert!(apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Atomic(NodeEdit::SetBypassed {
            node_id: bypassed_id,
            bypassed: true,
        })],
        &mut history,
        &mut state,
    ));
    assert!(project.get_node(bypassed_id).unwrap().bypassed);
    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);

    assert!(!apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Atomic(NodeEdit::SetBypassed {
            node_id: unsupported_id,
            bypassed: true,
        })],
        &mut history,
        &mut state,
    ));
    assert!(!project.get_node(unsupported_id).unwrap().bypassed);
}

#[test]
fn merge_wire_layer_order_and_authored_blend_are_canonical_and_undoable() {
    let (mut project, _, _, clip_id, solid_id, merge_id) = fixture();
    let first_connection_id = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(solid_id)
                && connection.to == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
        })
        .unwrap()
        .id;
    let mut second = generator_node(
        "Second Solid",
        GeneratorNodeRequest::Solid {
            color: Color::black(),
        },
    );
    second.ui_position = [450.0, 560.0];
    let second_id = second.id;
    project.add_node(second);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), second_id)
        .unwrap();
    let second_connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();

    let first = project
        .connections
        .iter()
        .find(|connection| connection.id == first_connection_id)
        .unwrap();
    let second = project
        .connections
        .iter()
        .find(|connection| connection.id == second_connection_id)
        .unwrap();
    assert!(connection_supports_authored_blend(&project, first));
    assert!(connection_supports_authored_blend(&project, second));
    assert_eq!(
        wire_order_menu_state(&project, first),
        Some(WireOrderMenuState {
            back_to_front_index: 0,
            layer_count: 2,
        })
    );
    assert_eq!(
        wire_order_menu_state(&project, second),
        Some(WireOrderMenuState {
            back_to_front_index: 1,
            layer_count: 2,
        })
    );

    // Disabled boundary actions are true no-ops: no Project change and no
    // extra history snapshot even if the QA bridge injects their click.
    let boundary_initial = project.clone();
    let mut boundary_history = HistoryManager::new();
    boundary_history.push_project_state(boundary_initial.clone());
    let mut state = NodeEditorState::default();
    assert!(!apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
            connection_id: first_connection_id,
            new_order: 0,
        })],
        &mut boundary_history,
        &mut state,
    ));
    assert_eq!(project, boundary_initial);
    assert_eq!(boundary_history.undo_depth(), 1);
    assert!(!apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
            connection_id: second_connection_id,
            new_order: 1,
        })],
        &mut boundary_history,
        &mut state,
    ));
    assert_eq!(project, boundary_initial);
    assert_eq!(boundary_history.undo_depth(), 1);

    let blend_initial = project.clone();
    let original_first = project
        .connections
        .iter()
        .find(|connection| connection.id == first_connection_id)
        .unwrap()
        .clone();
    let mut blend_history = HistoryManager::new();
    blend_history.push_project_state(blend_initial.clone());
    assert!(apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
            connection_id: first_connection_id,
            blend_mode: BlendMode::Multiply,
        })],
        &mut blend_history,
        &mut state,
    ));
    let blended_first = project
        .connections
        .iter()
        .find(|connection| connection.id == first_connection_id)
        .unwrap();
    assert_eq!(blended_first.id, original_first.id);
    assert_eq!(blended_first.from, original_first.from);
    assert_eq!(blended_first.to, original_first.to);
    assert_eq!(blended_first.order, original_first.order);
    assert_eq!(blended_first.blend_mode, BlendMode::Multiply);
    let blend_edited = project.clone();
    assert_single_gesture_undo_redo(&mut blend_history, &blend_initial, &blend_edited);

    let reorder_initial = project.clone();
    let original_second_blend = project
        .connections
        .iter()
        .find(|connection| connection.id == second_connection_id)
        .unwrap()
        .blend_mode;
    let mut reorder_history = HistoryManager::new();
    reorder_history.push_project_state(reorder_initial.clone());
    assert!(apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
            connection_id: first_connection_id,
            new_order: 1,
        })],
        &mut reorder_history,
        &mut state,
    ));
    let mut merge_connections = project
        .connections
        .iter()
        .filter(|connection| {
            connection.to == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
        })
        .collect::<Vec<_>>();
    merge_connections.sort_by_key(|connection| connection.order);
    assert_eq!(
        merge_connections
            .iter()
            .map(|connection| (connection.id, connection.order))
            .collect::<Vec<_>>(),
        vec![(second_connection_id, 0), (first_connection_id, 1)]
    );
    assert_eq!(merge_connections[0].blend_mode, original_second_blend);
    assert_eq!(merge_connections[1].blend_mode, BlendMode::Multiply);
    assert_eq!(merge_connections[1].from, original_first.from);
    assert_eq!(merge_connections[1].to, original_first.to);
    let reorder_edited = project.clone();
    assert_single_gesture_undo_redo(&mut reorder_history, &reorder_initial, &reorder_edited);

    let no_op_initial = project.clone();
    let mut no_op_history = HistoryManager::new();
    no_op_history.push_project_state(no_op_initial.clone());
    assert!(!apply_queued_node_edits(
        &mut project,
        vec![
            QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
                connection_id: first_connection_id,
                new_order: 1,
            }),
            QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
                connection_id: first_connection_id,
                blend_mode: BlendMode::Multiply,
            }),
        ],
        &mut no_op_history,
        &mut state,
    ));
    assert_eq!(project, no_op_initial);
    assert_eq!(no_op_history.undo_depth(), 1);

    let time_connection = project
        .connections
        .iter()
        .find(|connection| connection.from.port == TIME_PORT)
        .unwrap()
        .clone();
    assert!(!connection_supports_authored_blend(
        &project,
        &time_connection
    ));
    assert!(!apply_queued_node_edits(
        &mut project,
        vec![QueuedNodeEdit::Atomic(NodeEdit::SetConnectionBlendMode {
            connection_id: time_connection.id,
            blend_mode: BlendMode::LinearDodge,
        })],
        &mut no_op_history,
        &mut state,
    ));
    assert_eq!(project, no_op_initial);
    assert_eq!(no_op_history.undo_depth(), 1);
}

#[test]
fn merge_body_rows_present_front_to_back_and_keep_canonical_wire_identity() {
    let (mut project, composition_id, _, clip_id, solid_id, merge_id) = fixture();
    let single_layer_estimated = estimated_node_size(&project, merge_id);
    let first_connection_id = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(solid_id)
                && connection.to == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
        })
        .expect("fixture Merge connection")
        .id;
    project
        .set_connection_blend_mode(first_connection_id, BlendMode::LinearDodge)
        .expect("first wire Add");

    let mut middle = generator_node(
        "Middle Green",
        GeneratorNodeRequest::Solid {
            color: Color {
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            },
        },
    );
    middle.ui_position = [490.0, 520.0];
    let middle_id = middle.id;
    project.add_node(middle);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), middle_id)
        .expect("attach middle source");
    let middle_connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(middle_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .expect("connect middle source");
    project
        .set_connection_blend_mode(middle_connection_id, BlendMode::Multiply)
        .expect("middle wire Multiply");

    let mut front = generator_node(
        "Front Blue",
        GeneratorNodeRequest::Solid {
            color: Color {
                r: 0,
                g: 0,
                b: 255,
                a: 255,
            },
        },
    );
    front.ui_position = [530.0, 650.0];
    let front_id = front.id;
    project.add_node(front);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), front_id)
        .expect("attach front source");
    let front_connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(front_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .expect("connect front source");
    project
        .set_connection_blend_mode(front_connection_id, BlendMode::Screen)
        .expect("front wire Screen");

    let rows = merge_layer_rows(&project, merge_id);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .map(|row| (
                row.connection_id,
                row.canonical_index,
                row.authored_order,
                row.authored_blend_mode,
                row.source.owner,
                row.source_label.as_str(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                front_connection_id,
                2,
                2,
                BlendMode::Screen,
                PortOwner::Node(front_id),
                "Node · Front Blue",
            ),
            (
                middle_connection_id,
                1,
                1,
                BlendMode::Multiply,
                PortOwner::Node(middle_id),
                "Node · Middle Green",
            ),
            (
                first_connection_id,
                0,
                0,
                BlendMode::LinearDodge,
                PortOwner::Node(solid_id),
                "Node · Solid",
            ),
        ]
    );
    assert!(rows.iter().all(|row| {
        row.merge_id == merge_id && row.layer_count == 3 && row.authored_blend_available
    }));

    let estimated = estimated_node_size(&project, merge_id);
    assert_eq!(estimated.x, 544.0);
    assert_eq!(estimated.x, estimated_merge_node_width());
    assert_eq!(estimated_node_size(&project, solid_id).x, 462.0);
    assert_eq!(estimated_node_width(), 462.0);
    assert!(estimated.y > single_layer_estimated.y);
    let (rects, _, rendered_transform, _) =
        render_test_graph_with_context_menu_exclusions(&project, composition_id);
    let rendered_merge = rects
        .get(&format!("node_editor.node:{merge_id}"))
        .expect("rendered Merge card");
    assert!(
        rendered_merge.width() <= estimated.x * rendered_transform.scaling + 1.0,
        "rendered Merge width escaped its authoritative estimate: rendered={rendered_merge:?}, estimated={estimated:?}, scale={}",
        rendered_transform.scaling,
    );
    assert!(rendered_merge.height() <= estimated.y * rendered_transform.scaling + 1.0);
    let port_rects = [
        qa_port_id(
            &project,
            Some(GraphItem::Node(merge_id)),
            "input",
            MERGE_IMAGES_PORT,
        ),
        qa_port_id(
            &project,
            Some(GraphItem::Node(merge_id)),
            "output",
            IMAGE_OUTPUT_PORT,
        ),
    ]
    .map(|component_id| {
        *rects
            .get(&component_id)
            .unwrap_or_else(|| panic!("missing Merge port {component_id}"))
    });
    for row in &rows {
        for component_id in [
            format!("node_editor.merge_layer:{merge_id}:{}", row.connection_id),
            format!(
                "node_editor.merge_layer.blend_select:{merge_id}:{}",
                row.connection_id
            ),
            format!(
                "node_editor.merge_layer.order_back:{merge_id}:{}",
                row.connection_id
            ),
            format!(
                "node_editor.merge_layer.order_front:{merge_id}:{}",
                row.connection_id
            ),
        ] {
            let control = rects
                .get(&component_id)
                .unwrap_or_else(|| panic!("missing Merge body component {component_id}"));
            assert!(control.is_positive(), "empty Merge control {component_id}");
            assert!(
                port_rects
                    .iter()
                    .all(|port_rect| !control.intersects(*port_rect)),
                "Merge control {component_id} overlaps a left/right port: {control:?}"
            );
        }
    }
}

#[test]
fn empty_merge_body_has_a_stable_empty_state_and_minimum_estimated_height() {
    let (mut project, composition_id, _, _, _, merge_id) = fixture();
    project.connections.retain(|connection| {
        connection.to != PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
    });

    assert!(merge_layer_rows(&project, merge_id).is_empty());
    assert_eq!(estimated_node_size(&project, merge_id).y, 220.0);
    let rects = render_test_graph(&project, composition_id);
    assert!(rects
        .get(&format!("node_editor.merge_layers.empty:{merge_id}"))
        .is_some_and(egui::Rect::is_positive));
}

#[test]
fn real_egui_wire_hit_selects_and_dragging_the_body_queues_disconnect() {
    let (project, _, _, _, solid_id, merge_id) = fixture();
    let connection = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(solid_id)
                && connection.to.owner == PortOwner::Node(merge_id)
        })
        .unwrap();
    let edge = RenderedEdge {
        kind: RenderedEdgeKind::ProjectConnection {
            connection_id: connection.id,
        },
        start: egui::pos2(120.0, 180.0),
        control_a: egui::pos2(200.0, 180.0),
        control_b: egui::pos2(300.0, 180.0),
        end: egui::pos2(380.0, 180.0),
    };
    let midpoint = cubic_bezier_point(edge.start, edge.control_a, edge.control_b, edge.end, 0.5);
    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
    let mut state = NodeEditorState::default();
    let click = vec![
        vec![egui::Event::PointerMoved(midpoint)],
        vec![egui::Event::PointerButton {
            pos: midpoint,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerButton {
            pos: midpoint,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    assert!(
        run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, click,)
            .is_empty()
    );
    assert_eq!(state.selected_connection_id, Some(connection.id));

    let escape = vec![vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]];
    assert!(
        run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, escape,)
            .is_empty()
    );
    assert!(state.selected_connection_id.is_none());

    assert!(run_wire_interaction_frames(
        &project,
        &edge,
        &rendered_ports,
        &mut state,
        vec![
            vec![egui::Event::PointerMoved(midpoint)],
            vec![egui::Event::PointerButton {
                pos: midpoint,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: midpoint,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ],
    )
    .is_empty());
    assert_eq!(state.selected_connection_id, Some(connection.id));
    let blank = egui::pos2(32.0, 32.0);
    assert!(run_wire_interaction_frames(
        &project,
        &edge,
        &rendered_ports,
        &mut state,
        vec![
            vec![egui::Event::PointerMoved(blank)],
            vec![egui::Event::PointerButton {
                pos: blank,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: blank,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ],
    )
    .is_empty());
    assert!(state.selected_connection_id.is_none());

    let dragged = midpoint + egui::vec2(0.0, 48.0);
    let drag = vec![
        vec![egui::Event::PointerMoved(midpoint)],
        vec![egui::Event::PointerButton {
            pos: midpoint,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerMoved(dragged)],
        vec![egui::Event::PointerButton {
            pos: dragged,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    let edits = run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, drag);
    assert!(
        matches!(
            edits.as_slice(),
            [QueuedNodeEdit::Atomic(NodeEdit::DisconnectConnection { connection_id })]
                if *connection_id == connection.id
        ),
        "unexpected wire drag edits: {edits:?}; gesture: {:?}",
        state.wire_gesture
    );
}

#[test]
fn connected_output_invalid_drop_and_escape_leave_project_and_history_untouched() {
    let (project, _, _, _, solid_id, merge_id) = fixture();
    let connection = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(solid_id)
                && connection.to.owner == PortOwner::Node(merge_id)
        })
        .unwrap();
    let source = egui::pos2(120.0, 180.0);
    let invalid_target = egui::pos2(540.0, 340.0);
    let edge = RenderedEdge {
        kind: RenderedEdgeKind::ProjectConnection {
            connection_id: connection.id,
        },
        start: source,
        control_a: egui::pos2(200.0, 180.0),
        control_b: egui::pos2(300.0, 180.0),
        end: egui::pos2(380.0, 180.0),
    };
    let rendered_ports = Arc::new(Mutex::new(HashMap::from([(
        RenderedPortKey {
            address: connection.from.clone(),
            direction: PortDirection::Output,
            connection_id: None,
        },
        egui::Rect::from_center_size(source, egui::vec2(13.0, 13.0)),
    )])));

    let invalid_drop = vec![
        vec![egui::Event::PointerMoved(source)],
        vec![egui::Event::PointerButton {
            pos: source,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerMoved(invalid_target)],
        vec![egui::Event::PointerButton {
            pos: invalid_target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    let mut state = NodeEditorState::default();
    let edits =
        run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, invalid_drop);
    assert!(edits.is_empty());
    assert!(state.normal_connect_gesture.is_none());
    assert!(state.selected_connection_id.is_none());

    let escaped = vec![
        vec![egui::Event::PointerMoved(source)],
        vec![egui::Event::PointerButton {
            pos: source,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerMoved(invalid_target)],
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerButton {
            pos: invalid_target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    let edits = run_wire_interaction_frames(&project, &edge, &rendered_ports, &mut state, escaped);
    assert!(edits.is_empty());
    assert!(state.normal_connect_gesture.is_none());

    let mut untouched = project.clone();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let undo_depth = history.undo_depth();
    assert!(!apply_queued_node_edits(
        &mut untouched,
        edits,
        &mut history,
        &mut state,
    ));
    assert_eq!(untouched, project);
    assert_eq!(history.undo_depth(), undo_depth);
}

#[test]
fn overview_wire_midpoint_remains_a_body_target_when_endpoint_radii_overlap() {
    let edge = RenderedEdge {
        kind: RenderedEdgeKind::ProjectConnection {
            connection_id: Uuid::new_v4(),
        },
        start: egui::pos2(100.0, 100.0),
        control_a: egui::pos2(106.0, 100.0),
        control_b: egui::pos2(114.0, 100.0),
        end: egui::pos2(120.0, 100.0),
    };
    assert_eq!(
        rendered_wire_drag_kind(&edge, egui::pos2(110.0, 100.0)),
        NodeEditorWireDragKind::Disconnect
    );
    assert_eq!(
        rendered_wire_drag_kind(&edge, edge.start),
        NodeEditorWireDragKind::ReconnectSource
    );
    assert_eq!(
        rendered_wire_drag_kind(&edge, edge.end),
        NodeEditorWireDragKind::ReconnectTarget
    );
}

#[test]
fn endpoint_drag_reconnects_through_real_pointer_frames_without_changing_wire_identity() {
    let (mut project, _, _, clip_id, solid_id, merge_id) = fixture();
    let mut alternate = generator_node(
        "Alternate",
        GeneratorNodeRequest::Solid {
            color: Color::black(),
        },
    );
    alternate.ui_position = [250.0, 520.0];
    let alternate_id = alternate.id;
    project.add_node(alternate);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), alternate_id)
        .unwrap();
    let connection = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(solid_id)
                && connection.to.owner == PortOwner::Node(merge_id)
        })
        .unwrap()
        .clone();
    let edge = RenderedEdge {
        kind: RenderedEdgeKind::ProjectConnection {
            connection_id: connection.id,
        },
        start: egui::pos2(120.0, 180.0),
        control_a: egui::pos2(200.0, 180.0),
        control_b: egui::pos2(300.0, 180.0),
        end: egui::pos2(380.0, 180.0),
    };
    let alternate_position = egui::pos2(480.0, 260.0);
    let rendered_ports = Arc::new(Mutex::new(HashMap::from([(
        RenderedPortKey {
            address: PortAddress::new(PortOwner::Node(alternate_id), IMAGE_OUTPUT_PORT),
            direction: PortDirection::Output,
            connection_id: None,
        },
        egui::Rect::from_center_size(alternate_position, egui::vec2(14.0, 14.0)),
    )])));
    let mut state = NodeEditorState::default();
    let edits = run_wire_interaction_frames(
        &project,
        &edge,
        &rendered_ports,
        &mut state,
        vec![
            vec![egui::Event::PointerMoved(edge.start)],
            vec![egui::Event::PointerButton {
                pos: edge.start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(alternate_position)],
            vec![egui::Event::PointerButton {
                pos: alternate_position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ],
    );
    let [QueuedNodeEdit::Atomic(NodeEdit::ReconnectConnection {
        connection_id,
        from,
        to,
    })] = edits.as_slice()
    else {
        panic!("endpoint drag did not queue one reconnect: {edits:?}");
    };
    assert_eq!(*connection_id, connection.id);
    assert_eq!(from.owner, PortOwner::Node(alternate_id));
    assert_eq!(*to, connection.to);
    assert!(apply_edit(
        &mut project,
        NodeEdit::ReconnectConnection {
            connection_id: *connection_id,
            from: from.clone(),
            to: to.clone(),
        },
    ));
    let reconnected = project
        .connections
        .iter()
        .find(|candidate| candidate.id == connection.id)
        .unwrap();
    assert_eq!(reconnected.from.owner, PortOwner::Node(alternate_id));
    assert_eq!(reconnected.to, connection.to);
    assert_eq!(reconnected.order, connection.order);
}

#[test]
fn operation_node_splice_preserves_downstream_uuid_order_and_target() {
    let (mut project, composition_id, _, clip_id, solid_id, merge_id) = fixture();
    let connection = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(solid_id)
                && connection.to.owner == PortOwner::Node(merge_id)
        })
        .unwrap()
        .clone();
    let plugins = PluginManager::default();
    let mut blur = plugins.create_effect_operation_node("blur").unwrap();
    blur.ui_position = [610.0, 500.0];
    let blur_id = blur.id;
    project.add_node(blur);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), blur_id)
        .unwrap();
    assert!(splice_existing_node_on_connection(
        &mut project,
        connection.id,
        blur_id,
    ));
    let downstream = project
        .connections
        .iter()
        .find(|candidate| candidate.id == connection.id)
        .unwrap();
    assert_eq!(downstream.from.owner, PortOwner::Node(blur_id));
    assert_eq!(downstream.to, connection.to);
    assert_eq!(downstream.order, connection.order);

    let second_connection = project
        .connections
        .iter()
        .find(|candidate| candidate.to.owner == PortOwner::Node(blur_id))
        .unwrap()
        .clone();
    let second_blur = plugins.create_effect_operation_node("blur").unwrap();
    let second_blur_id = second_blur.id;
    assert!(insert_node_on_connection(
        &mut project,
        second_connection.id,
        second_blur,
        egui::pos2(560.0, 440.0),
        composition_id,
    ));
    assert_eq!(
        project
            .connections
            .iter()
            .find(|candidate| candidate.id == second_connection.id)
            .unwrap()
            .to,
        second_connection.to
    );
    assert_eq!(
        project.find_node_container(second_blur_id),
        Some(NodeContainer::Clip(clip_id))
    );
}
