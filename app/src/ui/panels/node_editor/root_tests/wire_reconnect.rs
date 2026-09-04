use super::*;

#[test]
fn selected_source_handle_wins_over_socket_fanout_and_reconnects_atomically() {
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
    let source_rect = egui::Rect::from_center_size(edge.start, egui::vec2(14.0, 14.0));
    let rendered_ports = Arc::new(Mutex::new(HashMap::from([
        (
            RenderedPortKey {
                address: connection.from.clone(),
                direction: PortDirection::Output,
                connection_id: None,
            },
            source_rect,
        ),
        (
            RenderedPortKey {
                address: PortAddress::new(PortOwner::Node(alternate_id), IMAGE_OUTPUT_PORT),
                direction: PortDirection::Output,
                connection_id: None,
            },
            egui::Rect::from_center_size(alternate_position, egui::vec2(14.0, 14.0)),
        ),
    ])));

    // An unselected connected socket keeps the ordinary fan-out gesture.
    let mut fanout_state = NodeEditorState::default();
    assert!(
        run_wire_interaction_frames(
            &project,
            &edge,
            &rendered_ports,
            &mut fanout_state,
            vec![
                vec![egui::Event::PointerMoved(edge.start)],
                vec![egui::Event::PointerButton {
                    pos: edge.start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
            ],
        )
        .is_empty()
    );
    assert!(fanout_state.normal_connect_gesture.is_some());
    assert!(fanout_state.wire_gesture.is_none());

    let source_handle = reconnect_handle_position(
        &edge,
        crate::state::context_types::NodeEditorWireDragKind::ReconnectSource,
    )
    .unwrap();
    assert_eq!(WIRE_RECONNECT_HANDLE_OFFSET, 9.0);
    assert_eq!(WIRE_RECONNECT_HANDLE_RADIUS, 4.0);
    assert_eq!(WIRE_ENDPOINT_RADIUS, 12.0);
    const {
        assert!(WIRE_RECONNECT_HANDLE_RADIUS < WIRE_RECONNECT_HANDLE_OFFSET);
        assert!(WIRE_RECONNECT_HANDLE_OFFSET < WIRE_ENDPOINT_RADIUS);
        assert!(WIRE_RECONNECT_HANDLE_OFFSET + WIRE_RECONNECT_HANDLE_RADIUS > WIRE_ENDPOINT_RADIUS);
    }
    assert_eq!(
        source_handle.distance(edge.start),
        WIRE_RECONNECT_HANDLE_OFFSET
    );
    assert!(!source_rect.contains(source_handle));

    let initial = project.clone();
    let mut state = NodeEditorState {
        selected_connection_id: Some(connection.id),
        ..Default::default()
    };
    let edits = run_wire_interaction_frames(
        &project,
        &edge,
        &rendered_ports,
        &mut state,
        vec![
            vec![egui::Event::PointerMoved(source_handle)],
            vec![egui::Event::PointerButton {
                pos: source_handle,
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
    let [
        QueuedNodeEdit::Atomic(NodeEdit::ReconnectConnection {
            connection_id,
            from,
            to,
        }),
    ] = edits.as_slice()
    else {
        panic!("endpoint drag did not queue one reconnect: {edits:?}");
    };
    assert_eq!(*connection_id, connection.id);
    assert_eq!(from.owner, PortOwner::Node(alternate_id));
    assert_eq!(*to, connection.to);
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    assert!(apply_queued_node_edits(
        &mut project,
        edits,
        &mut history,
        &mut state,
    ));
    let reconnected = project
        .connections
        .iter()
        .find(|candidate| candidate.id == connection.id)
        .unwrap();
    assert_eq!(reconnected.from.owner, PortOwner::Node(alternate_id));
    assert_eq!(reconnected.to, connection.to);
    assert_eq!(reconnected.order, connection.order);
    assert_eq!(reconnected.blend_mode, connection.blend_mode);
    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);
}

#[test]
fn selected_target_handle_reconnects_through_real_pointer_frames_as_one_history_edit() {
    let (mut project, _, _, clip_id, solid_id, merge_id) = fixture();
    let mut alternate = Node::new_merge("Alternate target");
    alternate.ui_position = [650.0, 520.0];
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
    let alternate_position = egui::pos2(520.0, 280.0);
    let target_rect = egui::Rect::from_center_size(edge.end, egui::vec2(14.0, 14.0));
    let rendered_ports = Arc::new(Mutex::new(HashMap::from([
        (
            RenderedPortKey {
                address: connection.to.clone(),
                direction: PortDirection::Input,
                connection_id: Some(connection.id),
            },
            target_rect,
        ),
        (
            RenderedPortKey {
                address: PortAddress::new(PortOwner::Node(alternate_id), MERGE_IMAGES_PORT),
                direction: PortDirection::Input,
                connection_id: None,
            },
            egui::Rect::from_center_size(alternate_position, egui::vec2(14.0, 14.0)),
        ),
    ])));
    let target_handle = reconnect_handle_position(
        &edge,
        crate::state::context_types::NodeEditorWireDragKind::ReconnectTarget,
    )
    .unwrap();
    assert_eq!(
        target_handle.distance(edge.end),
        WIRE_RECONNECT_HANDLE_OFFSET
    );
    assert!(!target_rect.contains(target_handle));

    let initial = project.clone();
    let mut state = NodeEditorState {
        selected_connection_id: Some(connection.id),
        ..Default::default()
    };
    let edits = run_wire_interaction_frames(
        &project,
        &edge,
        &rendered_ports,
        &mut state,
        vec![
            vec![egui::Event::PointerMoved(target_handle)],
            vec![egui::Event::PointerButton {
                pos: target_handle,
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
    assert!(matches!(
        edits.as_slice(),
        [QueuedNodeEdit::Atomic(NodeEdit::ReconnectConnection {
            connection_id,
            from,
            to,
        })] if *connection_id == connection.id
            && *from == connection.from
            && *to == PortAddress::new(PortOwner::Node(alternate_id), MERGE_IMAGES_PORT)
    ));

    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    assert!(apply_queued_node_edits(
        &mut project,
        edits,
        &mut history,
        &mut state,
    ));
    let reconnected = project
        .connections
        .iter()
        .find(|candidate| candidate.id == connection.id)
        .unwrap();
    assert_eq!(reconnected.from, connection.from);
    assert_eq!(
        reconnected.to,
        PortAddress::new(PortOwner::Node(alternate_id), MERGE_IMAGES_PORT)
    );
    assert_eq!(reconnected.order, connection.order);
    assert_eq!(reconnected.blend_mode, connection.blend_mode);
    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);
}
