use super::*;

#[test]
fn extreme_zoom_transform_and_adaptive_grid_remain_finite_and_bounded() {
    let style = node_editor_snarl_style();
    assert_eq!(style.min_scale, Some(0.0065));
    assert_eq!(style.max_scale, Some(1.25));

    let mut corrupt =
        egui::emath::TSTransform::new(egui::vec2(f32::NAN, f32::INFINITY), f32::NEG_INFINITY);
    sanitize_node_editor_transform(&mut corrupt);
    assert_eq!(corrupt, egui::emath::TSTransform::IDENTITY);
    assert!(corrupt.is_valid());
    assert!(corrupt.translation.y.is_finite());

    let mut extreme = egui::emath::TSTransform::new(
        egui::vec2(20_000_000.0, -20_000_000.0),
        NODE_EDITOR_MIN_SCALE / 10.0,
    );
    sanitize_node_editor_transform(&mut extreme);
    assert_eq!(extreme.scaling, NODE_EDITOR_MIN_SCALE);
    assert_eq!(extreme.translation.x, NODE_EDITOR_MAX_TRANSLATION);
    assert_eq!(extreme.translation.y, -NODE_EDITOR_MAX_TRANSLATION);

    let transform = egui::emath::TSTransform::new(egui::vec2(347.0, -73.0), NODE_EDITOR_MIN_SCALE);
    let graph_position = egui::pos2(500_000.0, -250_000.0);
    let screen_position = transform * graph_position;
    let round_trip = transform.inverse() * screen_position;
    assert!(screen_position.x.is_finite() && screen_position.y.is_finite());
    assert!(round_trip.distance(graph_position) < 0.1);
    assert!(!node_editor_details_visible(NODE_EDITOR_MIN_SCALE));
    assert!(node_editor_details_visible(NODE_EDITOR_DETAIL_SCALE));
    assert!(
        (screen_stroke_in_graph_units(1.65, NODE_EDITOR_MIN_SCALE) * NODE_EDITOR_MIN_SCALE - 1.65)
            .abs()
            < 1.0e-5
    );

    let screen_wire = [
        egui::pos2(10.0, 20.0),
        egui::pos2(35.0, 20.0),
        egui::pos2(65.0, 80.0),
        egui::pos2(90.0, 80.0),
    ];
    let graph_wire =
        overview_wire_graph_points(screen_wire, transform).expect("finite overview wire");
    for (screen, graph) in screen_wire.into_iter().zip(graph_wire) {
        assert!((transform * graph).distance(screen) < 0.001);
    }
    assert!(overview_wire_graph_points(
        screen_wire,
        egui::emath::TSTransform::new(egui::Vec2::ZERO, 0.0),
    )
    .is_none());
}

#[test]
fn overview_wire_survives_the_real_egui_layer_transform_in_screen_space() {
    let context = egui::Context::default();
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let to_global = egui::emath::TSTransform::new(egui::vec2(420.0, 310.0), NODE_EDITOR_MIN_SCALE);
    let start = egui::pos2(250.0, 300.0);
    let end = egui::pos2(750.0, 500.0);
    let from = PortAddress::new(PortOwner::Node(Uuid::from_u128(0x901)), "image");
    let to = PortAddress::new(PortOwner::Node(Uuid::from_u128(0x902)), "image");
    let ports = HashMap::from([
        (
            RenderedPortKey {
                address: from.clone(),
                direction: PortDirection::Output,
                connection_id: None,
            },
            egui::Rect::from_center_size(start, egui::Vec2::ZERO),
        ),
        (
            RenderedPortKey {
                address: to.clone(),
                direction: PortDirection::Input,
                connection_id: None,
            },
            egui::Rect::from_center_size(end, egui::Vec2::ZERO),
        ),
    ]);
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(canvas),
            ..Default::default()
        },
        |context| {
            let layer = egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("overview-wire-transform-test"),
            );
            context.set_transform_layer(layer, to_global);
            let painter = egui::Painter::new(context.clone(), layer, to_global.inverse() * canvas);
            let _ = register_edge_component(
                EdgeComponent {
                    id: "node_editor.edge:overview-wire-transform-test".to_string(),
                    kind: RenderedEdgeKind::ProjectConnection {
                        connection_id: Uuid::from_u128(0x8_001),
                    },
                    from: &from,
                    to: &to,
                    wire_color: pin_color(PortDataType::Image),
                    authored_order: None,
                    back_to_front_index: None,
                    layer_count: None,
                    physical_merge_target: false,
                    authored_blend_mode: None,
                    authored_blend_available: false,
                    runtime_first_produced_may_be_normal: false,
                },
                &ports,
                canvas,
                Some(OverviewWirePainter {
                    painter: &painter,
                    to_global,
                }),
            );
        },
    );

    let (clip_rect, wire) = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::CubicBezier(wire) => Some((clipped.clip_rect, wire)),
            _ => None,
        })
        .expect("overview CubicBezier in final egui output");
    let expected_frame = ((end.x - start.x).abs() * 0.45).clamp(2.0, 110.0);
    let expected = [
        start,
        start + egui::vec2(expected_frame, 0.0),
        end - egui::vec2(expected_frame, 0.0),
        end,
    ];
    for (actual, expected) in wire.points.iter().zip(expected) {
        assert!(actual.distance(expected) < 0.01);
    }
    assert!((wire.stroke.width - 1.65).abs() < 0.001);
    assert!(clip_rect.min.distance(canvas.min) < 0.01);
    assert!(clip_rect.max.distance(canvas.max) < 0.01);
}

#[test]
fn overview_canvas_keeps_pan_gestures_while_precision_controls_are_disabled() {
    let (project, composition_id, _, _, _, _) = fixture();
    let (mut snarl, containers) = build_snarl(&project, composition_id);
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let zoom_position = egui::pos2(500.0, 400.0);
    let drag_start = egui::pos2(100.0, 100.0);
    let drag_end = egui::pos2(220.0, 160.0);
    let command_modifiers = egui::Modifiers {
        command: true,
        mac_cmd: cfg!(target_os = "macos"),
        ..egui::Modifiers::NONE
    };
    let frames = [
        (Vec::new(), egui::Modifiers::NONE),
        (
            vec![
                egui::Event::PointerMoved(zoom_position),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -10_000.0),
                    modifiers: command_modifiers,
                },
            ],
            command_modifiers,
        ),
        (
            vec![
                egui::Event::PointerMoved(drag_start),
                egui::Event::PointerButton {
                    pos: drag_start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
        ),
        (
            vec![egui::Event::PointerMoved(drag_end)],
            egui::Modifiers::NONE,
        ),
        (
            vec![egui::Event::PointerButton {
                pos: drag_end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
        ),
    ];
    let mut transforms = Vec::new();
    let mut node_editor_state = NodeEditorState::default();

    for (frame, (events, modifiers)) in frames.into_iter().enumerate() {
        let mut rendered_transform = egui::emath::TSTransform::IDENTITY;
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                events,
                modifiers,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut selection = None;
                    let mut wire_context_request = None;
                    let mut exclusions = Vec::new();
                    let mut to_global = egui::emath::TSTransform::IDENTITY;
                    let mut canvas_clip = ui.clip_rect();
                    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
                    let mut merge_layer_reorder = None;
                    let mut viewer = ProjectNodeViewer {
                        project: &project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut edits,
                        pending_navigation: &mut navigation,
                        pending_selection: &mut selection,
                        selected_node_ids: &[],
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut exclusions,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: false,
                        locked_canvas_transform: None,
                        previous_canvas_transform: node_editor_state.node_editor_canvas_transform,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports,
                        merge_layer_reorder: &mut merge_layer_reorder,
                        rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                    };
                    snarl.show(
                        &mut viewer,
                        &node_editor_snarl_style(),
                        egui::Id::new(("overview-pan-test", composition_id)),
                        ui,
                    );
                    drop(viewer);
                    node_editor_state.node_editor_canvas_transform = Some(to_global);
                    let resize_edits = container_resize_interactions(
                        ui,
                        &project,
                        &containers,
                        to_global,
                        canvas_clip,
                        &mut node_editor_state,
                    );
                    assert!(edits.is_empty());
                    assert!(resize_edits.is_empty());
                    rendered_transform = to_global;
                });
            },
        );
        assert!(!output.shapes.is_empty());
        transforms.push(rendered_transform);
    }

    let zoomed = transforms[1];
    let dragged = transforms[3];
    assert!((zoomed.scaling - NODE_EDITOR_MIN_SCALE).abs() < f32::EPSILON);
    assert_eq!(dragged.scaling, zoomed.scaling);
    assert!(!node_editor_resize_interactions_enabled(zoomed.scaling));
    assert!(!node_editor_port_interactions_enabled(zoomed.scaling));
    let pan_delta = dragged.translation - zoomed.translation;
    assert!((pan_delta.x - (drag_end.x - drag_start.x)).abs() < 1.0);
    assert!((pan_delta.y - (drag_end.y - drag_start.y)).abs() < 1.0);
    assert!(node_editor_state.container_resize.is_none());
}

#[test]
fn canvas_qa_metadata_exposes_the_final_clamped_transform_and_lod_gates() {
    let composition_id = Uuid::from_u128(0xCA_11_A5);
    let metadata = node_editor_canvas_metadata(
        composition_id,
        egui::emath::TSTransform::new(egui::vec2(321.5, -87.25), NODE_EDITOR_MIN_SCALE / 10.0),
    );
    assert_eq!(metadata["composition_id"], composition_id.to_string());
    assert_eq!(metadata["scale"], NODE_EDITOR_MIN_SCALE);
    assert_eq!(metadata["translation"]["x"], 321.5);
    assert_eq!(metadata["translation"]["y"], -87.25);
    assert_eq!(metadata["min_scale"], NODE_EDITOR_MIN_SCALE);
    assert_eq!(metadata["max_scale"], NODE_EDITOR_MAX_SCALE);
    assert_eq!(metadata["detail_enabled"], false);
    assert_eq!(metadata["port_interaction_enabled"], false);
    assert_eq!(metadata["resize_interaction_enabled"], false);
}

#[test]
fn active_knife_owns_the_canvas_transform_instead_of_panning_the_scene() {
    let locked = egui::emath::TSTransform::new(egui::vec2(120.0, 240.0), 0.25);
    let mut scene_pan = egui::emath::TSTransform::new(egui::vec2(1_920.0, -480.0), locked.scaling);
    let previous_scene_pan = scene_pan;
    resolve_node_editor_transform(&mut scene_pan, Some(locked), Some(previous_scene_pan));
    assert_eq!(scene_pan, locked);

    let mut normal_pan = egui::emath::TSTransform::new(egui::vec2(1_920.0, -480.0), locked.scaling);
    let previous_normal_pan = normal_pan;
    resolve_node_editor_transform(&mut normal_pan, None, Some(previous_normal_pan));
    assert_eq!(normal_pan.translation, egui::vec2(1_920.0, -480.0));
}

#[test]
fn overview_port_qa_rect_matches_the_real_reconnect_drop_hit_test() {
    let scale = NODE_EDITOR_DETAIL_SCALE * 0.5;
    assert!(!node_editor_port_interactions_enabled(scale));
    let to_global = egui::emath::TSTransform::new(egui::vec2(410.0, 290.0), scale);
    let graph_position = egui::pos2(120.0, 80.0);
    let rendered_port_rect =
        to_global * egui::Rect::from_center_size(graph_position, egui::Vec2::ZERO);
    let drop_rect = wire_port_drop_rect(rendered_port_rect);
    let position = rendered_port_rect.center();
    assert_eq!(drop_rect.size(), egui::vec2(10.0, 10.0));
    assert!(drop_rect.contains(position));

    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    assert!(clipped_qa_rect(drop_rect, canvas).is_positive());
    let address = PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT);
    let ports = HashMap::from([(
        RenderedPortKey {
            address: address.clone(),
            direction: PortDirection::Output,
            connection_id: None,
        },
        rendered_port_rect,
    )]);
    assert_eq!(
        rendered_port_at_position(&ports, PortDirection::Output, position, canvas),
        Some(address)
    );
    assert!(
        rendered_normal_port_at_position(&ports, position, canvas).is_none(),
        "overview reconnect drop targets must not steal normal Snarl connection gestures"
    );
    let detailed_ports = HashMap::from([(
        RenderedPortKey {
            address: PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT),
            direction: PortDirection::Output,
            connection_id: None,
        },
        egui::Rect::from_center_size(position, egui::vec2(13.0, 13.0)),
    )]);
    assert!(rendered_normal_port_at_position(&detailed_ports, position, canvas).is_some());

    let offscreen =
        egui::Rect::from_center_size(egui::pos2(canvas.right() + 10.0, 400.0), egui::Vec2::ZERO);
    assert!(!clipped_qa_rect(wire_port_drop_rect(offscreen), canvas).is_positive());
    let offscreen_ports = HashMap::from([(
        RenderedPortKey {
            address: PortAddress::new(PortOwner::Node(Uuid::new_v4()), IMAGE_OUTPUT_PORT),
            direction: PortDirection::Output,
            connection_id: None,
        },
        offscreen,
    )]);
    assert!(rendered_port_at_position(
        &offscreen_ports,
        PortDirection::Output,
        offscreen.center(),
        canvas,
    )
    .is_none());
}

#[test]
fn edge_endpoint_qa_metadata_exposes_screen_position_and_unclipped_rect() {
    let connection_id = Uuid::from_u128(0xED6E);
    let position = egui::pos2(321.5, 654.25);
    let rect = egui::Rect::from_center_size(position, egui::vec2(18.0, 18.0));
    let metadata = edge_endpoint_qa_metadata(connection_id, "source", position, rect);
    assert_eq!(metadata["action"], "reconnect");
    assert_eq!(metadata["connection_id"], connection_id.to_string());
    assert_eq!(metadata["endpoint"], "source");
    assert_eq!(metadata["position"]["x"], position.x);
    assert_eq!(metadata["position"]["y"], position.y);
    assert_eq!(metadata["unclipped_rect"]["min_x"], rect.min.x);
    assert_eq!(metadata["unclipped_rect"]["min_y"], rect.min.y);
    assert_eq!(metadata["unclipped_rect"]["max_x"], rect.max.x);
    assert_eq!(metadata["unclipped_rect"]["max_y"], rect.max.y);
}

#[test]
fn extreme_zoom_disables_precision_hits_without_expanding_node_hit_area() {
    assert!(!node_editor_port_interactions_enabled(
        NODE_EDITOR_MIN_SCALE
    ));
    assert!(!node_editor_resize_interactions_enabled(
        NODE_EDITOR_MIN_SCALE
    ));
    assert!(node_editor_resize_interactions_enabled(
        NODE_EDITOR_RESIZE_INTERACTION_SCALE
    ));
    assert!(!node_editor_port_interactions_enabled(
        NODE_EDITOR_RESIZE_INTERACTION_SCALE
    ));
    assert!(node_editor_port_interactions_enabled(
        NODE_EDITOR_DETAIL_SCALE
    ));

    let graph_node = egui::Rect::from_min_size(
        egui::pos2(250_000.0, 80_000.0),
        egui::vec2(NODE_HEADER_WIDTH, 100.0),
    );
    let scale = NODE_EDITOR_MIN_SCALE;
    let desired_center = egui::pos2(500.0, 400.0);
    let translation = desired_center.to_vec2() - graph_node.center().to_vec2() * scale;
    let to_global = egui::emath::TSTransform::new(translation, scale);
    let screen_node = to_global * graph_node;
    assert!(screen_node.width() < 2.0);
    assert!(screen_node.height() < 1.0);

    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let exclusions = [graph_node];
    let mut state = None;
    update_global_context_menu_for_secondary_click(
        &mut state,
        true,
        Some(screen_node.center()),
        canvas,
        &exclusions,
        to_global,
        1.0,
    );
    assert!(
        state.is_none(),
        "the actual tiny node still owns its pixels"
    );

    let nearby_empty_screen = screen_node.center() + egui::vec2(4.0, 0.0);
    update_global_context_menu_for_secondary_click(
        &mut state,
        true,
        Some(nearby_empty_screen),
        canvas,
        &exclusions,
        to_global,
        2.0,
    );
    assert_eq!(
        state
            .expect("nearby overview space remains canvas")
            .position,
        nearby_empty_screen
    );

    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
    let pin = QaPin {
        info: pin_info(PortDataType::Image, false),
        component_id: "node_editor.port.extreme_zoom_test".to_string(),
        to_global,
        graph_center: Some(graph_node.center()),
        address: None,
        data_type: PortDataType::Image,
        direction: PortDirection::Input,
        connected: false,
        connection_id: None,
        canvas_clip: canvas,
        rendered_ports,
    };
    let pin_rect = pin.pin_rect(0.0, 0.0, 20.0, 13.0);
    assert!(
        !pin_rect.is_positive(),
        "overview sockets cannot steal a drag"
    );

    // At this scale every fixed-width screen resize region overlaps this
    // tiny container. The resize dispatcher therefore gates all of them
    // with the same precision-interaction threshold.
    let tiny_container = egui::Rect::from_center_size(
        desired_center,
        egui::vec2(MIN_CONTAINER_SIZE.x * scale, MIN_CONTAINER_SIZE.y * scale),
    );
    assert!(resize_regions(tiny_container)
        .iter()
        .any(|(_, _, rect, _)| rect.contains(desired_center)));
    assert!(!node_editor_resize_interactions_enabled(scale));
}
