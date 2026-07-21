use super::*;

#[test]
fn text_add_inserts_one_clean_graph_without_replacing_the_clip_output() {
    let (mut project, composition_id, _, clip_id, _, merge_id) = fixture();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .unwrap();
    let initial_layout = compute_full_composition_layout(&project, composition_id).unwrap();
    assert!(apply_auto_layout(
        &mut project,
        composition_id,
        &initial_layout
    ));
    assert!(!layout_needs_reflow(&project, composition_id));

    let factory = style_graph_factory();
    let graph = factory
        .create_text_graph("Hello", "Arial", 1920, 1080)
        .unwrap();
    let consumer_id = graph.output_node_id.expect("Text factory sink");
    let source_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Text)
            )
        })
        .unwrap()
        .id;
    let fill_id = graph
        .nodes
        .iter()
        .find(|node| plugin_operation_component(node) == Some("fill"))
        .unwrap()
        .id;
    let connection = graph.connections[0].clone();
    let bundled_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();

    let mut laid_out = graph.clone();
    layout_detached_node_graph(&project, &mut laid_out);
    assert_detached_graph_has_clean_ltr_layout(&project, &laid_out);
    let relative_positions = laid_out
        .nodes
        .iter()
        .map(|node| (node.id, node.ui_position))
        .collect::<HashMap<_, _>>();

    let clip = project.get_clip(clip_id).unwrap();
    let desired = egui::pos2(
        clip.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
        clip.ui_position[1] + AUTO_LAYOUT_CLIP_TOP,
    );
    let initial = project.clone();
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    assert!(insert_prebuilt_graph(
        &mut project,
        desired,
        graph,
        composition_id
    ));
    history.push_project_state(project.clone());

    let clip = project.get_clip(clip_id).unwrap();
    assert_eq!(clip.output_node_id, Some(merge_id));
    assert_eq!(
        &clip.node_ids[clip.node_ids.len() - bundled_ids.len()..],
        bundled_ids.as_slice()
    );
    assert_eq!(
        project.find_node_container(consumer_id),
        Some(NodeContainer::Clip(clip_id))
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|candidate| candidate.id == connection.id),
        Some(&connection)
    );

    let shape_output = project
        .port_definition(
            &PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT),
            PortDirection::Output,
        )
        .unwrap();
    let shape_input = project
        .port_definition(
            &PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
            PortDirection::Input,
        )
        .unwrap();
    let image_output = project
        .port_definition(
            &PortAddress::new(PortOwner::Node(fill_id), IMAGE_OUTPUT_PORT),
            PortDirection::Output,
        )
        .unwrap();
    assert_eq!(shape_output.data_type, PortDataType::Shape);
    assert_eq!(shape_input.data_type, PortDataType::Shape);
    assert_eq!(image_output.data_type, PortDataType::Image);

    let first_id = bundled_ids[0];
    let first = project.get_node(first_id).unwrap().ui_position;
    let first_relative = relative_positions[&first_id];
    let translation = [first[0] - first_relative[0], first[1] - first_relative[1]];
    for node_id in &bundled_ids {
        let inserted = project.get_node(*node_id).unwrap().ui_position;
        let relative = relative_positions[node_id];
        assert!((inserted[0] - relative[0] - translation[0]).abs() < 0.01);
        assert!((inserted[1] - relative[1] - translation[1]).abs() < 0.01);
    }
    assert!(!layout_needs_reflow(&project, composition_id));

    let rects = render_test_graph(&project, composition_id);
    for id in [
        format!("node_editor.port.node:{source_id}.output:{SHAPE_OUTPUT_PORT}"),
        format!("node_editor.port.node:{fill_id}.input:{SHAPE_INPUT_PORT}"),
        format!("node_editor.port.node:{fill_id}.output:{IMAGE_OUTPUT_PORT}"),
        format!("node_editor.edge:{}", connection.id),
    ] {
        assert!(
            rects.get(&id).is_some_and(egui::Rect::is_positive),
            "missing visible typed Shape/Image graph component {id}"
        );
    }

    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);
}

#[test]
fn shape_add_preserves_none_output_and_fill_stroke_order_without_overlap() {
    let (mut project, composition_id, _, clip_id, _, _) = fixture();
    let initial_layout = compute_full_composition_layout(&project, composition_id).unwrap();
    assert!(apply_auto_layout(
        &mut project,
        composition_id,
        &initial_layout
    ));
    assert_eq!(project.get_clip(clip_id).unwrap().output_node_id, None);

    let factory = style_graph_factory();
    let graph = factory
        .create_shape_graph("M0 0 H100 V100 Z", 1920, 1080, 100, 100)
        .unwrap();
    let consumer_id = graph.output_node_id.expect("Shape factory sink");
    let bundled_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let connection_ids = graph
        .connections
        .iter()
        .map(|connection| connection.id)
        .collect::<Vec<_>>();
    let mut laid_out = graph.clone();
    layout_detached_node_graph(&project, &mut laid_out);
    assert_detached_graph_has_clean_ltr_layout(&project, &laid_out);

    let clip = project.get_clip(clip_id).unwrap();
    let desired = egui::pos2(
        clip.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
        clip.ui_position[1] + AUTO_LAYOUT_CLIP_TOP,
    );
    assert!(insert_prebuilt_graph(
        &mut project,
        desired,
        graph,
        composition_id
    ));

    let clip = project.get_clip(clip_id).unwrap();
    assert_eq!(clip.output_node_id, None);
    assert_eq!(
        &clip.node_ids[clip.node_ids.len() - bundled_ids.len()..],
        bundled_ids.as_slice()
    );
    let appended = project
        .connections
        .iter()
        .filter(|connection| {
            connection_ids.contains(&connection.id)
                && connection.to
                    == PortAddress::new(PortOwner::Node(consumer_id), MERGE_IMAGES_PORT)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        appended
            .iter()
            .map(|connection| connection.order)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        appended
            .iter()
            .map(|connection| {
                let PortOwner::Node(node_id) = connection.from.owner else {
                    panic!("Style source must be a Node")
                };
                plugin_operation_component(project.get_node(node_id).unwrap()).unwrap()
            })
            .collect::<Vec<_>>(),
        vec!["fill", "stroke"]
    );
    assert!(!layout_needs_reflow(&project, composition_id));
}

#[test]
fn standalone_style_add_has_shape_input_image_output_and_failed_graph_add_is_atomic() {
    let (mut project, composition_id, _, clip_id, _, merge_id) = fixture();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .unwrap();
    let factory = style_graph_factory();
    let plugins = factory.get_plugin_manager();

    let fill = plugins.create_style_operation_node("fill").unwrap();
    let fill_id = fill.id;
    assert!(insert_prebuilt_graph(
        &mut project,
        egui::pos2(500.0, 350.0),
        NodeGraphBundle::new(vec![fill], Vec::new(), None),
        composition_id,
    ));
    assert_eq!(
        project.get_clip(clip_id).unwrap().output_node_id,
        Some(merge_id)
    );
    assert_eq!(
        project.find_node_container(fill_id),
        Some(NodeContainer::Clip(clip_id))
    );
    assert_eq!(
        project
            .port_definition(
                &PortAddress::new(PortOwner::Node(fill_id), IMAGE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .unwrap()
            .data_type,
        PortDataType::Image
    );
    assert_eq!(
        project
            .port_definition(
                &PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
                PortDirection::Input,
            )
            .unwrap()
            .data_type,
        PortDataType::Shape
    );

    let stroke = plugins.create_style_operation_node("stroke").unwrap();
    let stroke_id = stroke.id;
    let width = node_property_definition(Some(&plugins), &stroke, "width")
        .expect("runtime descriptor width metadata");
    assert!(matches!(
        width.ui_type(),
        PropertyUiType::Float {
            min: 0.0,
            max: 100.0,
            step: 1.0,
            suffix,
            ..
        } if suffix == "px"
    ));
    let join = node_property_definition(Some(&plugins), &stroke, "join")
        .expect("runtime descriptor enum metadata");
    assert!(matches!(
        join.ui_type(),
        PropertyUiType::Dropdown { options }
            if options.iter().map(String::as_str).eq(["Miter", "Round", "Bevel"])
    ));
    assert!(insert_prebuilt_graph(
        &mut project,
        egui::pos2(500.0, 350.0),
        NodeGraphBundle::new(vec![stroke], Vec::new(), None),
        composition_id,
    ));
    assert_eq!(
        project.get_clip(clip_id).unwrap().output_node_id,
        Some(merge_id)
    );
    assert_eq!(
        project.find_node_container(stroke_id),
        Some(NodeContainer::Clip(clip_id))
    );
    let rects = render_test_graph(&project, composition_id);
    for node_id in [fill_id, stroke_id] {
        let output = format!("node_editor.port.node:{node_id}.output:{IMAGE_OUTPUT_PORT}");
        assert!(rects.get(&output).is_some_and(egui::Rect::is_positive));
    }

    let mut invalid = factory
        .create_text_graph("duplicate", "Arial", 1920, 1080)
        .unwrap();
    invalid.nodes[0].id = merge_id;
    let before = project.clone();
    assert!(!insert_prebuilt_graph(
        &mut project,
        egui::pos2(500.0, 350.0),
        invalid,
        composition_id,
    ));
    assert_eq!(
        project, before,
        "failed insertion must not partially mutate Project"
    );
}

#[test]
fn effect_operation_add_is_ltr_typed_atomic_and_preserves_the_clip_output() {
    let (mut project, composition_id, _, clip_id, _, merge_id) = fixture();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .unwrap();
    let initial_layout = compute_full_composition_layout(&project, composition_id).unwrap();
    assert!(apply_auto_layout(
        &mut project,
        composition_id,
        &initial_layout
    ));

    let factory = style_graph_factory();
    let plugins = factory.get_plugin_manager();
    assert!(plugins
        .get_available_effects()
        .iter()
        .any(|(id, _, _)| id == "blur"));
    let source = factory
        .create_solid_node(
            library::model::frame::color::Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            1920,
            1080,
        )
        .unwrap();
    let effect = plugins.create_effect_operation_node("blur").unwrap();
    let source_id = source.id;
    let effect_id = effect.id;
    let sigma_x = node_property_definition(Some(&plugins), &effect, "sigma_x")
        .expect("Blur numeric metadata");
    assert!(matches!(
        sigma_x.ui_type(),
        PropertyUiType::Float {
            min: 0.0,
            max: 100.0,
            step: 0.1,
            suffix,
            min_hard_limit: true,
            max_hard_limit: false,
        } if suffix == "px"
    ));
    let tile_mode =
        node_property_definition(Some(&plugins), &effect, "tile_mode").expect("Blur enum metadata");
    assert!(matches!(
        tile_mode.ui_type(),
        PropertyUiType::Dropdown { options }
            if options.iter().map(String::as_str).eq(["clamp", "repeat", "mirror", "decal"])
    ));

    let wire = ProjectConnection::new(
        PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
        0,
    );
    let graph = NodeGraphBundle::new(vec![source, effect], vec![wire.clone()], Some(effect_id));
    let bundled_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let mut laid_out = graph.clone();
    layout_detached_node_graph(&project, &mut laid_out);
    assert_detached_graph_has_clean_ltr_layout(&project, &laid_out);

    let clip = project.get_clip(clip_id).unwrap();
    let desired = egui::pos2(
        clip.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
        clip.ui_position[1] + AUTO_LAYOUT_CLIP_TOP,
    );
    let initial = project.clone();
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    assert!(insert_prebuilt_graph(
        &mut project,
        desired,
        graph,
        composition_id,
    ));
    history.push_project_state(project.clone());

    let clip = project.get_clip(clip_id).unwrap();
    assert_eq!(clip.output_node_id, Some(merge_id));
    assert_eq!(
        &clip.node_ids[clip.node_ids.len() - bundled_ids.len()..],
        bundled_ids.as_slice()
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == wire.id),
        Some(&wire)
    );
    for (port, direction) in [
        (IMAGE_INPUT_PORT, PortDirection::Input),
        (IMAGE_OUTPUT_PORT, PortDirection::Output),
    ] {
        assert_eq!(
            project
                .port_definition(
                    &PortAddress::new(PortOwner::Node(effect_id), port),
                    direction,
                )
                .unwrap()
                .data_type,
            PortDataType::Image
        );
    }
    assert!(!layout_needs_reflow(&project, composition_id));

    let rects = render_test_graph(&project, composition_id);
    for id in [
        format!("node_editor.port.node:{effect_id}.input:{IMAGE_INPUT_PORT}"),
        format!("node_editor.port.node:{effect_id}.output:{IMAGE_OUTPUT_PORT}"),
        format!("node_editor.edge:{}", wire.id),
    ] {
        assert!(
            rects.get(&id).is_some_and(egui::Rect::is_positive),
            "missing visible Effect graph component {id}"
        );
    }
    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);

    let mut duplicate = plugins.create_effect_operation_node("blur").unwrap();
    duplicate.id = merge_id;
    let before_failure = project.clone();
    assert!(!insert_prebuilt_graph(
        &mut project,
        desired,
        NodeGraphBundle::new(vec![duplicate], Vec::new(), None),
        composition_id,
    ));
    assert_eq!(project, before_failure);
}

#[test]
fn effector_operation_nodes_and_menu_use_the_authoritative_descriptor() {
    let factory = style_graph_factory();
    let plugins = factory.get_plugin_manager();
    let menu_entries = available_effector_menu_entries(plugins.as_ref());
    assert!(menu_entries.contains(&("transform".to_string(), "Transform Modulation".to_string())));
    assert!(menu_entries.contains(&("opacity".to_string(), "Opacity Modulation".to_string())));
    assert!(menu_entries
        .windows(2)
        .all(|entries| entries[0].1 <= entries[1].1));

    for component_id in ["transform", "opacity"] {
        let descriptor = plugins
            .operation_descriptor(EFFECTOR_CATEGORY, component_id, EFFECTOR_APPLY_OPERATION)
            .unwrap();
        let node = plugins
            .create_effector_operation_node(component_id)
            .unwrap();
        assert_eq!(
            node.properties()
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<BTreeSet<_>>(),
            descriptor
                .properties()
                .iter()
                .map(PropertyDefinition::name)
                .collect::<BTreeSet<_>>()
        );
        for definition in descriptor.properties() {
            assert_eq!(
                node.properties()
                    .get(definition.name())
                    .and_then(|property| property.evaluate_at(0.0).ok()),
                Some(definition.default_value().clone()),
                "{component_id}.{} was not initialized by its descriptor factory",
                definition.name(),
            );
        }
    }
    let transform = plugins.create_effector_operation_node("transform").unwrap();
    assert_eq!(
        transform
            .properties()
            .get("target")
            .and_then(|property| property.evaluate_at(0.0).ok()),
        Some(PropertyValue::String("Block".to_string()))
    );
    let opacity = plugins.create_effector_operation_node("opacity").unwrap();
    assert_eq!(
        opacity
            .properties()
            .get("mode")
            .and_then(|property| property.evaluate_at(0.0).ok()),
        Some(PropertyValue::String("Set".to_string()))
    );
    assert_eq!(
        opacity
            .properties()
            .get("target")
            .and_then(|property| property.evaluate_at(0.0).ok()),
        Some(PropertyValue::String("Block".to_string()))
    );
}

#[test]
fn node_editor_effector_control_responds_to_real_pointer_drag() {
    let (mut project, composition_id, _, clip_id, _, _) = fixture();
    let plugins = PluginManager::default();
    let mut effector = plugins.create_effector_operation_node("transform").unwrap();
    effector.ui_position = [520.0, 390.0];
    let effector_id = effector.id;
    project.add_node(effector);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), effector_id)
        .unwrap();
    let layout = compute_full_composition_layout(&project, composition_id).unwrap();
    assert!(apply_auto_layout(&mut project, composition_id, &layout));
    assert!(!layout_needs_reflow(&project, composition_id));
    let (mut snarl, containers) = build_snarl(&project, composition_id);
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
    let component_id = format!("node_editor.property.node:{effector_id}:tx");
    let mut queued = Vec::new();
    reset_test_rects();

    let mut frames = vec![Vec::new(); 5];
    for (frame, events) in frames.drain(..).enumerate() {
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                events,
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
                    let mut merge_layer_reorder = None;
                    let mut viewer = ProjectNodeViewer {
                        project: &project,
                        plugin_manager: Some(&plugins),
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
                        previous_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                        merge_layer_reorder: &mut merge_layer_reorder,
                        rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                    };
                    snarl.show(
                        &mut viewer,
                        &node_editor_snarl_style(),
                        egui::Id::new(("effector-real-event", composition_id)),
                        ui,
                    );
                    queued.extend(edits);
                });
            },
        );
        drop(output);
    }
    let rect = test_rect(&component_id).expect("rendered Transform tx control");
    assert!(rect.is_positive());
    let start = rect.center();
    let end = start + egui::vec2(52.0, 0.0);
    let event_frames = [
        vec![egui::Event::PointerMoved(start)],
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        vec![egui::Event::PointerMoved(end)],
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    for (offset, events) in event_frames.into_iter().enumerate() {
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some((offset + 5) as f64 / 60.0),
                events,
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
                    let mut merge_layer_reorder = None;
                    let mut viewer = ProjectNodeViewer {
                        project: &project,
                        plugin_manager: Some(&plugins),
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
                        previous_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                        merge_layer_reorder: &mut merge_layer_reorder,
                        rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        rendered_selection_hits: Arc::new(Mutex::new(Vec::new())),
                    };
                    snarl.show(
                        &mut viewer,
                        &node_editor_snarl_style(),
                        egui::Id::new(("effector-real-event", composition_id)),
                        ui,
                    );
                    queued.extend(edits);
                });
            },
        );
        drop(output);
    }

    assert!(
        queued.iter().any(|edit| matches!(
            edit,
            QueuedNodeEdit::Continuous {
                edit: Some(NodeEdit::SetProperty {
                    owner: PortOwner::Node(id),
                    key,
                    value: PropertyValue::Number(value),
                    ..
                }),
                ..
            } if *id == effector_id && key == "tx" && value.into_inner() > 0.0
        )),
        "real pointer drag over {rect:?} did not edit tx: {queued:#?}"
    );
    assert!(queued.iter().any(|edit| matches!(
        edit,
        QueuedNodeEdit::Continuous {
            pending,
            finished: true,
            ..
        } if pending.owner == PortOwner::Node(effector_id) && pending.key == "tx"
    )));
}
