use super::*;

#[test]
fn graph_is_derived_from_three_container_levels_and_canonical_connections() {
    let (project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
    let (snarl, containers) = build_snarl(&project, composition_id);
    let items = snarl.nodes().copied().collect::<Vec<_>>();

    assert_eq!(containers.len(), 3);
    for owner in [
        PortOwner::Composition(composition_id),
        PortOwner::Track(track_id),
        PortOwner::Clip(clip_id),
    ] {
        assert!(items.contains(&GraphItem::Container(owner)));
        assert!(items.contains(&GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::ExternalInputs,
        }));
        assert!(items.contains(&GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::InternalMetadata,
        }));
        assert!(items.contains(&GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::OutputSinks,
        }));
        assert!(items.contains(&GraphItem::PortAnchor {
            owner,
            kind: PortAnchorKind::ExternalOutputs,
        }));
        assert_eq!(
            input_definitions(
                &project,
                GraphItem::PortAnchor {
                    owner,
                    kind: PortAnchorKind::ExternalInputs,
                },
            )
            .len(),
            3
        );
    }
    assert!(items.contains(&GraphItem::Node(solid_id)));
    assert!(items.contains(&GraphItem::Node(merge_id)));
    assert!(snarl.wires().count() >= 2);

    let merge_inputs = input_definitions(&project, GraphItem::Node(merge_id));
    assert!(merge_inputs.iter().any(|pin| pin.key == MERGE_IMAGES_PORT));
    let metadata = output_definitions(
        &project,
        GraphItem::PortAnchor {
            owner: PortOwner::Clip(clip_id),
            kind: PortAnchorKind::InternalMetadata,
        },
    );
    assert_eq!(metadata[0].key, TIME_PORT);
    assert_eq!(metadata[1].key, FRAME_PORT);
}

#[test]
fn transformed_container_chrome_exclusively_owns_secondary_click_but_empty_body_does_not() {
    let (project, composition_id, track_id, _, _, _) = fixture();
    let (rects, exclusion_rects, rendered_to_global, containers) =
        render_test_graph_with_context_menu_exclusions(&project, composition_id);
    let header_screen_rect = rects
        .get(&format!(
            "node_editor.container_header.{}",
            qa_container_key(PortOwner::Track(track_id))
        ))
        .copied()
        .expect("rendered Track header");
    let header_graph_position = rendered_to_global.inverse() * header_screen_rect.center();
    assert!(
        exclusion_rects
            .iter()
            .any(|rect| rect.contains(header_graph_position)),
        "the rendered Track header must be claimed by a Snarl hit rect"
    );

    let track_visual = containers
        .iter()
        .find(|container| container.owner == PortOwner::Track(track_id))
        .expect("Track visual");
    let track_rect = track_visual.rect();
    let empty_body_graph_position =
        egui::pos2(track_rect.right() - 40.0, track_rect.bottom() - 40.0);
    let track_body = egui::Rect::from_min_max(
        egui::pos2(
            track_rect.left(),
            track_rect.top() + CONTAINER_HEADER_HEIGHT,
        ),
        track_rect.max,
    );
    assert!(track_body.contains(empty_body_graph_position));
    assert!(
        !exclusion_rects
            .iter()
            .any(|rect| rect.contains(empty_body_graph_position)),
        "empty container body must not be claimed by Snarl chrome"
    );

    // Exercise the same inverse-transform path as a panned and zoomed
    // Node Editor canvas, rather than comparing screen-space rectangles.
    let to_global = egui::emath::TSTransform::new(egui::vec2(347.0, -73.0), 0.72);
    let canvas_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
    let header_screen_position = to_global * header_graph_position;
    let empty_body_screen_position = to_global * empty_body_graph_position;
    assert!(canvas_rect.contains(header_screen_position));
    assert!(canvas_rect.contains(empty_body_screen_position));

    let mut state = None;
    update_global_context_menu_for_secondary_click(
        &mut state,
        true,
        Some(header_screen_position),
        canvas_rect,
        &exclusion_rects,
        to_global,
        1.0,
    );
    assert!(
        state.is_none(),
        "Track header click must not open global Create menu"
    );

    state = Some(ContextMenuState::new(egui::pos2(1.0, 1.0), 0.0));
    update_global_context_menu_for_secondary_click(
        &mut state,
        true,
        Some(header_screen_position),
        canvas_rect,
        &exclusion_rects,
        to_global,
        2.0,
    );
    assert!(
        state.is_none(),
        "Track header click must close a stale global Create menu"
    );

    update_global_context_menu_for_secondary_click(
        &mut state,
        true,
        Some(empty_body_screen_position),
        canvas_rect,
        &exclusion_rects,
        to_global,
        3.0,
    );
    let context = state.expect("empty Track body should open global Create menu");
    assert_eq!(context.position, empty_body_screen_position);
}

#[test]
fn rendered_edges_include_explicit_output_binding_and_structural_connections() {
    let (mut project, composition_id, track_id, clip_id, _solid_id, merge_id) = fixture();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .unwrap();
    let rects = render_test_graph(&project, composition_id);

    for connection in &project.connections {
        let id = format!("node_editor.edge:{}", connection.id);
        let rect = rects
            .get(&id)
            .unwrap_or_else(|| panic!("missing rendered explicit edge {id}"));
        assert!(rect.is_positive(), "explicit edge has empty bbox: {rect:?}");
    }

    let binding = format!(
        "node_editor.edge.output_binding:{}:image:{merge_id}",
        qa_container_key(PortOwner::Clip(clip_id))
    );
    assert!(
        rects.get(&binding).is_some_and(egui::Rect::is_positive),
        "missing Clip output binding edge"
    );

    let track_merge_id = project
        .get_track(track_id)
        .unwrap()
        .structural_merge_node_id;
    let track_dependency = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Clip(clip_id)
                && connection.to.owner == PortOwner::Node(track_merge_id)
        })
        .expect("persisted Clip → structural Merge edge");
    assert!(
        rects
            .get(&format!("node_editor.edge:{}", track_dependency.id))
            .is_some_and(egui::Rect::is_positive),
        "missing explicit Clip → structural Merge edge"
    );
}

#[test]
fn clip_activity_and_disabled_state_have_distinct_inactive_reasons() {
    let (mut project, _, _, clip_id, solid_id, _) = fixture();
    let clip = project.get_clip(clip_id).unwrap();

    assert!(!clip_is_active(clip, 0.999));
    assert!(clip_is_active(clip, 1.0));
    assert!(clip_is_active(clip, 5.999));
    assert!(!clip_is_active(clip, 6.0));
    assert_eq!(
        graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 0.5),
        Some(GraphItemInactiveReason::OutsideClipRange)
    );
    assert_eq!(
        graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 1.0),
        None
    );

    project.get_node_mut(solid_id).unwrap().enabled = false;
    assert_eq!(
        graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 1.0),
        Some(GraphItemInactiveReason::Disabled)
    );
    // Disabled is the primary authored reason even when the Clip is also
    // outside its half-open active range.
    assert_eq!(
        graph_item_inactive_reason(&project, GraphItem::Node(solid_id), 0.5),
        Some(GraphItemInactiveReason::Disabled)
    );
    assert!(graph_item_inactive(
        &project,
        GraphItem::Node(solid_id),
        0.5
    ));
}

#[test]
fn alt_drag_knife_batches_explicit_and_output_binding() -> Result<(), String> {
    let (mut project, _, track_id, clip_id, solid_id, merge_id) = fixture();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .map_err(|error| error.to_string())?;
    let connection_ids = project
        .connections
        .iter()
        .filter(|connection| {
            matches!(
                connection.to.owner,
                PortOwner::Node(node_id) if node_id == solid_id || node_id == merge_id
            )
        })
        .map(|connection| connection.id)
        .collect::<Vec<_>>();
    assert_eq!(connection_ids.len(), 2);
    let edges = vec![
        RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: connection_ids[0],
            },
            start: egui::pos2(100.0, 160.0),
            control_a: egui::pos2(180.0, 120.0),
            control_b: egui::pos2(320.0, 200.0),
            end: egui::pos2(400.0, 160.0),
        },
        RenderedEdge {
            kind: RenderedEdgeKind::ProjectConnection {
                connection_id: connection_ids[1],
            },
            start: egui::pos2(100.0, 230.0),
            control_a: egui::pos2(180.0, 190.0),
            control_b: egui::pos2(320.0, 270.0),
            end: egui::pos2(400.0, 230.0),
        },
        RenderedEdge {
            kind: RenderedEdgeKind::OutputBinding {
                owner: PortOwner::Clip(clip_id),
                node_id: merge_id,
                data_type: PortDataType::Image,
            },
            start: egui::pos2(100.0, 300.0),
            control_a: egui::pos2(180.0, 260.0),
            control_b: egui::pos2(320.0, 340.0),
            end: egui::pos2(400.0, 300.0),
        },
    ];
    assert!(knife_segment_hits_edge(
        egui::pos2(250.0, 100.0),
        egui::pos2(250.0, 290.0),
        &edges[0],
    ));
    assert!(!knife_segment_hits_edge(
        egui::pos2(460.0, 100.0),
        egui::pos2(460.0, 290.0),
        &edges[0],
    ));

    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
    let start = egui::pos2(250.0, 100.0);
    let end = egui::pos2(250.0, 390.0);
    let alt = egui::Modifiers {
        alt: true,
        ..Default::default()
    };
    let frames = vec![
        vec![egui::Event::PointerMoved(start)],
        vec![egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: alt,
        }],
        vec![egui::Event::PointerMoved(egui::pos2(250.0, 245.0))],
        vec![egui::Event::PointerMoved(end)],
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: alt,
        }],
    ];
    let ports = Arc::new(Mutex::new(HashMap::new()));
    let mut state = NodeEditorState::default();
    let mut queued = Vec::new();
    for (frame, events) in frames.into_iter().enumerate() {
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                events,
                modifiers: alt,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    queued.extend(wire_interactions(
                        ui,
                        &mut state,
                        WireInteractionFrame {
                            project: &project,
                            edges: &edges,
                            rendered_ports: &ports,
                            canvas_clip: screen,
                            graph_item_rects: &[],
                            to_global: egui::emath::TSTransform::IDENTITY,
                        },
                    ));
                });
            },
        ));
    }
    let [QueuedNodeEdit::Atomic(NodeEdit::DisconnectWires { wires: crossed })] = queued.as_slice()
    else {
        return Err(format!("knife did not emit one batch: {queued:?}"));
    };
    let mut expected = connection_ids
        .iter()
        .copied()
        .map(|connection_id| NodeEditorEditableWire::ProjectConnection { connection_id })
        .chain(std::iter::once(NodeEditorEditableWire::OutputBinding {
            owner: PortOwner::Clip(clip_id),
            node_id: merge_id,
            data_type: PortDataType::Image,
        }))
        .collect::<Vec<_>>();
    expected.sort_by_key(|target| editable_wire_sort_key(*target));
    assert_eq!(crossed, &expected);
    let initial = project.clone();
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    assert!(apply_queued_node_edits(
        &mut project,
        queued,
        &mut history,
        &mut state,
    ));
    assert!(
        project
            .connections
            .iter()
            .all(|connection| !connection_ids.contains(&connection.id))
    );
    let edited_clip = project
        .get_clip(clip_id)
        .ok_or_else(|| "knife removed its Clip".to_string())?;
    assert_eq!(edited_clip.output_node_id, None);
    assert_eq!(project.find_track_for_clip(clip_id), Some(track_id));
    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);
    Ok(())
}

#[test]
fn zero_hit_knife_stroke_emits_no_edit_and_keeps_history_clean() {
    let (project, _, _, _, solid_id, merge_id) = fixture();
    let connection_id = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(solid_id)
                && connection.to.owner == PortOwner::Node(merge_id)
        })
        .unwrap()
        .id;
    let edge = RenderedEdge {
        kind: RenderedEdgeKind::ProjectConnection { connection_id },
        start: egui::pos2(100.0, 180.0),
        control_a: egui::pos2(200.0, 180.0),
        control_b: egui::pos2(300.0, 180.0),
        end: egui::pos2(400.0, 180.0),
    };
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
    let alt = egui::Modifiers {
        alt: true,
        ..Default::default()
    };
    let start = egui::pos2(500.0, 80.0);
    let end = egui::pos2(560.0, 120.0);
    let frames = vec![
        vec![egui::Event::PointerMoved(start)],
        vec![egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: alt,
        }],
        vec![egui::Event::PointerMoved(end)],
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: alt,
        }],
    ];
    let ports = Arc::new(Mutex::new(HashMap::new()));
    let mut state = NodeEditorState::default();
    let mut queued = Vec::new();
    for (frame, events) in frames.into_iter().enumerate() {
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                events,
                modifiers: alt,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    queued.extend(wire_interactions(
                        ui,
                        &mut state,
                        WireInteractionFrame {
                            project: &project,
                            edges: std::slice::from_ref(&edge),
                            rendered_ports: &ports,
                            canvas_clip: screen,
                            graph_item_rects: &[],
                            to_global: egui::emath::TSTransform::IDENTITY,
                        },
                    ));
                });
            },
        ));
    }
    assert!(queued.is_empty());
    assert!(state.wire_knife.is_none());
}
