use super::*;

#[test]
fn typed_port_owner_composition_ignores_same_uuid_clip() {
    let shared_id = Uuid::new_v4();
    let mut project = Project::new("typed owner composition");

    let (clip_composition, clip_track) =
        Composition::new("Clip composition", 1920, 1080, 30.0, 10.0);
    let clip_composition_id = clip_composition.id;
    let clip_track_id = clip_track.id;
    let mut collision = Clip::new("same UUID Clip", 0.0, 5.0);
    collision.id = shared_id;
    assert!(
        project.add_track(clip_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(clip_composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.add_clip(collision);
    project
        .attach_clip_to_track(clip_track_id, shared_id)
        .unwrap();

    let (node_composition, node_track) =
        Composition::new("Node composition", 1920, 1080, 30.0, 10.0);
    let node_composition_id = node_composition.id;
    let mut node = Node::new_merge("same UUID Node");
    node.id = shared_id;
    assert!(
        project.add_track(node_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(node_composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Composition(node_composition_id), shared_id)
        .unwrap();

    assert_eq!(
        project.find_containing_composition(shared_id),
        Some(clip_composition_id)
    );
    assert_eq!(
        port_owner_composition(&project, PortOwner::Node(shared_id)),
        Some(node_composition_id)
    );
    assert_eq!(
        port_owner_composition(&project, PortOwner::Clip(shared_id)),
        Some(clip_composition_id)
    );
}

struct RuntimeCatalogStylePlugin {
    id: String,
}

impl Plugin for RuntimeCatalogStylePlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> String {
        "Runtime Hatch".to_string()
    }

    fn category(&self) -> String {
        "Runtime QA".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl StylePlugin for RuntimeCatalogStylePlugin {
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::style(self.id.clone(), self.name(), Vec::new())
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        source_id: Uuid,
        _properties: &PropertyMap,
        _eval_time: f64,
    ) -> Option<StyleConfig> {
        Some(StyleConfig {
            id: source_id,
            style: DrawStyle::Fill {
                color: Color::white(),
                offset: 0.0,
            },
        })
    }
}

#[test]
fn native_values_have_explicit_operation_presentation() {
    let mut project = Project::new("value node presentation");
    assert_eq!(VALUE_NODE_CATEGORY_LABEL, "Value");
    for value in ValueContent::ALL {
        let node = Node::new_value(value.label(), value);
        let node_id = node.id;
        project.add_node(node);
        assert_eq!(value_operation_label(value), value.label());
        let icon = node_icon(&project, node_id);
        assert_eq!(
            icon.glyph,
            match value {
                ValueContent::Fmod => egui_phosphor::regular::PERCENT,
                ValueContent::Add => egui_phosphor::regular::PLUS,
                ValueContent::Subtract => egui_phosphor::regular::MINUS,
                ValueContent::Multiply => egui_phosphor::regular::X,
                ValueContent::Divide => egui_phosphor::regular::DIVIDE,
            }
        );
        assert!(icon.label.starts_with(value.label()));
        let palette = node_palette(&project, node_id);
        assert_eq!(palette.body, Color32::from_rgb(28, 41, 46));
        assert_eq!(palette.header, Color32::from_rgb(39, 83, 95));
        assert_eq!(palette.accent, Color32::from_rgb(91, 197, 218));
        assert_eq!(estimated_node_size(&project, node_id).y, 220.0);
    }
}

#[test]
fn container_chrome_uses_bundled_phosphor_icons() {
    let id = Uuid::new_v4();
    assert_eq!(
        container_icon(PortOwner::Composition(id)).glyph,
        egui_phosphor::regular::PROJECTOR_SCREEN
    );
    assert_eq!(
        container_icon(PortOwner::Track(id)).glyph,
        egui_phosphor::regular::STACK
    );
    assert_eq!(
        container_icon(PortOwner::Clip(id)).glyph,
        egui_phosphor::regular::FILM_STRIP
    );
    assert_eq!(
        container_icon(PortOwner::Node(id)).glyph,
        egui_phosphor::regular::CIRCLE
    );
}

#[test]
fn math_add_items_create_every_native_value_and_fmod_accepts_explicit_time() {
    let plugins = PluginManager::default();
    let items = node_create_menu_items(&plugins);
    for value in ValueContent::ALL {
        let request = NodeCreateRequest::Native(format!("native.math.{}", value.operation_key()));
        let item = items
            .iter()
            .find(|item| item.value == request)
            .expect("every native value is exposed by the shared Add catalog");
        assert_eq!(item.label, value.label());
        assert_eq!(item.category.as_deref(), Some("Math"));
        assert_eq!(
            item.qa_id.as_deref(),
            Some(format!("node_editor.menu.create.value:{}", value.operation_key()).as_str())
        );
        let node = create_operation_node_for_request(&item.value, &plugins)
            .expect("native value request creates a Node");
        assert_eq!(node.content(), &NodeContent::Value(value));
    }
    let item = items
        .iter()
        .find(|item| item.value == NodeCreateRequest::Native("native.math.fmod".to_string()))
        .expect("Fmod is exposed by the shared Add catalog");
    let matches =
        crate::ui::widgets::searchable_context_menu::filter_searchable_items(&items, "loop value");
    assert!(matches.iter().any(|index| items[*index] == *item));

    let (mut project, composition_id, _, clip_id, _, _) = fixture();
    let node = create_operation_node_for_request(&item.value, &plugins)
        .expect("native value request creates a Node");
    let node_id = node.id;
    let clip = project.get_clip(clip_id).unwrap();
    let position = egui::pos2(clip.ui_position[0] + 260.0, clip.ui_position[1] + 280.0);
    assert!(create_prebuilt_node(
        &mut project,
        position,
        node,
        composition_id,
    ));
    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Clip(clip_id))
    );
    let connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(node_id), FMOD_X_INPUT_PORT),
        )
        .expect("container Time connects explicitly to Fmod.x");
    let connection = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)
        .unwrap();
    assert_eq!(connection.from.port, TIME_PORT);
    assert_eq!(connection.to.port, FMOD_X_INPUT_PORT);
}

#[test]
fn real_snarl_connected_output_fans_out_to_time_value_without_reconnect_or_pan() {
    let (mut project, composition_id, _, clip_id, _, _) = fixture();
    let mut modulo = Node::new_fmod("Fmod");
    modulo.ui_position = [520.0, 620.0];
    let modulo_id = modulo.id;
    project.add_node(modulo);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), modulo_id)
        .unwrap();
    let original_time_connection = project
        .connections
        .iter()
        .find(|connection| {
            connection.from == PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT)
                && connection.to.port == TIME_PORT
        })
        .expect("fixture Time output is already connected")
        .clone();
    let initial = project.clone();
    let (mut snarl, containers) = build_snarl(&project, composition_id);
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
    let mut state = NodeEditorState::default();
    let mut queued = Vec::new();
    let mut gesture_transforms = Vec::new();
    reset_test_rects();

    let source_address = PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT);
    let target_address = PortAddress::new(PortOwner::Node(modulo_id), FMOD_X_INPUT_PORT);
    let source_key = RenderedPortKey {
        address: source_address.clone(),
        direction: PortDirection::Output,
        connection_id: None,
    };
    let target_key = RenderedPortKey {
        address: target_address.clone(),
        direction: PortDirection::Input,
        connection_id: None,
    };

    // Let Snarl finish its initial look-at/layout pass before using the
    // published socket rectangles as physical input coordinates.
    for frame in 0..6 {
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    rendered_ports.lock().unwrap().clear();
                    let mut navigation = None;
                    let mut wire_context_request = None;
                    let mut exclusions = Vec::new();
                    let mut to_global = egui::emath::TSTransform::default();
                    let mut canvas_clip = ui.clip_rect();
                    let mut viewer = ProjectNodeViewer {
                        project: &project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut queued,
                        pending_navigation: &mut navigation,
                        selected_node_ids: &[],
                        selected_container_owners: &[],
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut exclusions,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: state.wire_gesture.is_some()
                            || state.normal_connect_gesture.is_some()
                            || state.normal_connect_cancel_pending_release,
                        locked_canvas_transform: None,
                        previous_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::clone(&rendered_ports),
                        merge_layer_reorder: &mut state.merge_layer_reorder,
                        rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        surface_capture: Arc::new(Mutex::new(SurfaceCapture::default())),
                    };
                    snarl.show(
                        &mut viewer,
                        &node_editor_snarl_style(),
                        egui::Id::new(("time-value-port-drag", composition_id)),
                        ui,
                    );
                    drop(viewer);
                    let edges =
                        register_rendered_edges(&project, &rendered_ports, canvas_clip, None);
                    queued.extend(wire_interactions(
                        ui,
                        &mut state,
                        WireInteractionFrame {
                            project: &project,
                            edges: &edges,
                            rendered_ports: &rendered_ports,
                            canvas_clip,
                            graph_item_rects: &exclusions,
                            to_global,
                        },
                    ));
                });
            },
        ));
    }
    assert!(queued.is_empty());
    let ports = rendered_ports.lock().unwrap();
    let source_rect = *ports.get(&source_key).expect("actual Time output socket");
    let target_rect = *ports.get(&target_key).expect("actual value input socket");
    drop(ports);
    assert!(source_rect.is_positive());
    assert!(target_rect.is_positive());
    assert_eq!(
        test_rect(&qa_port_id(
            &project,
            Some(GraphItem::PortAnchor {
                owner: PortOwner::Clip(clip_id),
                kind: PortAnchorKind::InternalMetadata,
            }),
            "output",
            TIME_PORT,
        ))
        .expect("Time output QA hit")
        .center(),
        source_rect.center(),
    );
    assert_eq!(
        test_rect(&qa_port_id(
            &project,
            Some(GraphItem::Node(modulo_id)),
            "input",
            FMOD_X_INPUT_PORT,
        ))
        .expect("value input QA hit")
        .center(),
        target_rect.center(),
    );

    let source = source_rect.center();
    let target = target_rect.center();
    let drag_start = source + egui::vec2(WIRE_DRAG_THRESHOLD + 2.0, 0.0);
    let input_frames = vec![
        // First cancel the forward fan-out. Snarl still owns internal
        // drag state, so suppression must survive through release.
        vec![egui::Event::PointerMoved(source)],
        vec![egui::Event::PointerButton {
            pos: source,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerMoved(target)],
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        // Then perform the same connected-output fan-out successfully.
        vec![egui::Event::PointerMoved(source)],
        vec![egui::Event::PointerButton {
            pos: source,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        // Cross egui's drag threshold while the captured Time output is
        // still nearby. A single endpoint-to-endpoint jump can enter the
        // destination before Snarl has observed `drag_started_by`.
        vec![egui::Event::PointerMoved(drag_start)],
        vec![egui::Event::PointerMoved(target)],
        vec![egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    for (frame, events) in input_frames.into_iter().enumerate() {
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some((frame + 6) as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    rendered_ports.lock().unwrap().clear();
                    let mut navigation = None;
                    let mut wire_context_request = None;
                    let mut exclusions = Vec::new();
                    let mut to_global = egui::emath::TSTransform::default();
                    let mut canvas_clip = ui.clip_rect();
                    let mut viewer = ProjectNodeViewer {
                        project: &project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut queued,
                        pending_navigation: &mut navigation,
                        selected_node_ids: &[],
                        selected_container_owners: &[],
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut exclusions,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: state.wire_gesture.is_some()
                            || state.normal_connect_gesture.is_some()
                            || state.normal_connect_cancel_pending_release,
                        locked_canvas_transform: state
                            .normal_connect_gesture
                            .as_ref()
                            .map(|gesture| gesture.canvas_transform),
                        previous_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::clone(&rendered_ports),
                        merge_layer_reorder: &mut state.merge_layer_reorder,
                        rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                        surface_capture: Arc::new(Mutex::new(SurfaceCapture::default())),
                    };
                    snarl.show(
                        &mut viewer,
                        &node_editor_snarl_style(),
                        egui::Id::new(("time-value-port-drag", composition_id)),
                        ui,
                    );
                    drop(viewer);
                    let edges =
                        register_rendered_edges(&project, &rendered_ports, canvas_clip, None);
                    queued.extend(wire_interactions(
                        ui,
                        &mut state,
                        WireInteractionFrame {
                            project: &project,
                            edges: &edges,
                            rendered_ports: &rendered_ports,
                            canvas_clip,
                            graph_item_rects: &exclusions,
                            to_global,
                        },
                    ));
                    gesture_transforms.push(to_global);
                });
            },
        ));
        match frame {
            1 | 2 | 6..=8 => {
                assert!(state.normal_connect_gesture.is_some());
                assert!(!state.normal_connect_cancel_pending_release);
            }
            3 => {
                assert!(state.normal_connect_gesture.is_none());
                assert!(state.normal_connect_cancel_pending_release);
                assert!(queued.is_empty());
            }
            4 | 5 => {
                assert!(state.normal_connect_gesture.is_none());
                assert!(!state.normal_connect_cancel_pending_release);
                assert!(queued.is_empty());
            }
            0 | 9 => {
                assert!(state.normal_connect_gesture.is_none());
                assert!(!state.normal_connect_cancel_pending_release);
            }
            unexpected => panic!("unexpected input frame {unexpected}"),
        }
        assert!(!state.normal_wire_drag_active);
    }

    assert!(gesture_transforms.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(state.wire_gesture.is_none());
    assert!(state.selected_connection_id.is_none());
    assert!(
        queued.iter().any(|queued| {
            matches!(
                queued,
                QueuedNodeEdit::Atomic(NodeEdit::Connect { from, to })
                    if *from == source_address && *to == target_address
            )
        }),
        "real Snarl drag queued {queued:?}; wire state: {:?}",
        state.wire_gesture,
    );
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    assert!(apply_queued_node_edits(
        &mut project,
        queued,
        &mut history,
        &mut state,
    ));
    assert!(project.connections.iter().any(|connection| {
        connection.from == source_address && connection.to == target_address
    }));
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == original_time_connection.id),
        Some(&original_time_connection),
        "fan-out must preserve the original Time wire byte-for-byte"
    );
    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);
}

#[test]
fn add_menu_discovers_descriptor_operations_and_wire_menu_filters_them_by_type() {
    let plugins = Arc::new(PluginManager::default());
    let runtime_style_id = "runtime.qa.style.hatch";
    plugins.register_style_plugin(Arc::new(RuntimeCatalogStylePlugin {
        id: runtime_style_id.to_string(),
    }));
    let items = node_create_menu_items(plugins.as_ref());
    assert!(items.iter().all(|item| item.category.is_some()));
    assert!(items
        .iter()
        .any(|item| { item.value == NodeCreateRequest::Native("native.sksl-shader".to_string()) }));

    let root_transform = items
        .iter()
        .find(|item| item.value == NodeCreateRequest::ShapeTransform)
        .expect("root Transform is exposed as its own Add request");
    assert_eq!(root_transform.label, "Shape Transform");
    assert_eq!(
        root_transform.category.as_deref(),
        Some("Shape Operations / Transform")
    );
    assert_eq!(
        root_transform.qa_id.as_deref(),
        Some("node_editor.menu.create.transform")
    );
    assert_eq!(
        root_transform.qa_metadata.as_ref().unwrap()["operation_category"],
        TRANSFORM_CATEGORY
    );
    assert_eq!(
        root_transform.qa_metadata.as_ref().unwrap()["operation"],
        TRANSFORM_APPLY_OPERATION
    );
    let root_matches = crate::ui::widgets::searchable_context_menu::filter_searchable_items(
        &items,
        "root placement",
    );
    assert!(root_matches
        .iter()
        .any(|index| items[*index] == *root_transform));
    let root_node = create_operation_node_for_request(&root_transform.value, plugins.as_ref())
        .expect("root Transform request uses the operation factory");
    let NodeContent::PluginOperation(root_operation) = root_node.content() else {
        panic!("root Transform factory must create a PluginOperation")
    };
    assert_eq!(root_operation.category, TRANSFORM_CATEGORY);
    assert_eq!(root_operation.component_id, SHAPE_TRANSFORM_COMPONENT_ID);
    assert_eq!(root_operation.operation, TRANSFORM_APPLY_OPERATION);

    for (component_id, label) in [
        ("transform", "Effector · Transform Modulation"),
        ("opacity", "Effector · Opacity Modulation"),
    ] {
        let item = items
            .iter()
            .find(
                |item| matches!(&item.value, NodeCreateRequest::Effector(id) if id == component_id),
            )
            .expect("built-in modulation Effector is exposed in the Add menu");
        assert_eq!(item.label, label);
    }

    for component_id in ["fill", "stroke"] {
        let style = items
            .iter()
            .find(|item| matches!(&item.value, NodeCreateRequest::Style(id) if id == component_id))
            .unwrap();
        assert_eq!(
            style.qa_metadata.as_ref().unwrap()["component_id"],
            component_id
        );
        assert_eq!(
            style.qa_metadata.as_ref().unwrap()["operation"],
            STYLE_APPLY_OPERATION
        );
    }
    let backplate = items
        .iter()
        .find(|item| matches!(&item.value, NodeCreateRequest::Decorator(id) if id == "backplate"))
        .unwrap();
    assert_eq!(
        backplate.qa_id.as_deref(),
        Some("node_editor.menu.create.decorator:backplate")
    );

    let runtime_style = items
        .iter()
        .find(|item| matches!(&item.value, NodeCreateRequest::Style(id) if id == runtime_style_id))
        .expect("a style registered after PluginManager construction must be discoverable");
    assert_eq!(
        runtime_style.qa_id.as_deref(),
        Some("node_editor.menu.create.style:runtime.qa.style.hatch")
    );
    assert_eq!(
        runtime_style.qa_metadata.as_ref().unwrap()["label"],
        "Runtime Hatch"
    );
    assert!(runtime_style
        .keywords
        .iter()
        .any(|keyword| keyword == runtime_style_id));

    let blur = items
        .iter()
        .find(|item| {
            matches!(&item.value, NodeCreateRequest::Effect(effect_id) if effect_id == "blur")
        })
        .expect("built-in Blur effect is exposed in the Add menu");
    assert!(blur
        .category
        .as_deref()
        .is_some_and(|category| category.starts_with("Image Effects /")));
    assert_eq!(
        blur.qa_id.as_deref(),
        Some("node_editor.menu.create.effect:blur")
    );
    assert!(blur.keywords.iter().any(|keyword| keyword == "blur"));
    let matches = crate::ui::widgets::searchable_context_menu::filter_searchable_items(
        &items,
        "runtime.qa hatch",
    );
    assert!(matches.iter().any(|index| items[*index] == *runtime_style));

    let (mut project, _, _, clip_id, _, _) = fixture();
    let shape = generator_node(
        "Shape",
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 100 V 100 Z".to_string(),
        },
    );
    let shape_id = shape.id;
    let transform = plugins.create_effector_operation_node("transform").unwrap();
    let transform_id = transform.id;
    project.add_node(shape);
    project.add_node(transform);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), shape_id)
        .unwrap();
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), transform_id)
        .unwrap();
    let shape_connection = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT),
        )
        .unwrap();
    let splice_items = wire_splice_menu_items(&project, shape_connection, plugins.as_ref());
    assert!(splice_items
        .iter()
        .any(|item| item.value == NodeCreateRequest::ShapeTransform));
    assert!(splice_items.iter().any(|item| {
        matches!(&item.value, NodeCreateRequest::Decorator(id) if id == "backplate")
    }));
    assert!(!splice_items.iter().any(|item| {
        matches!(&item.value, NodeCreateRequest::Style(id) if id == runtime_style_id)
    }));

    let root =
        create_operation_node_for_request(&NodeCreateRequest::ShapeTransform, plugins.as_ref())
            .expect("wire insertion uses the root Transform factory");
    let root_id = root.id;
    project.add_node(root);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), root_id)
        .unwrap();
    assert!(splice_existing_node_on_connection(
        &mut project,
        shape_connection,
        root_id,
    ));
    assert!(project.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(root_id), SHAPE_INPUT_PORT)
    }));
    assert!(project.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(root_id), SHAPE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT)
    }));
    assert!(!project.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT)
    }));
}
