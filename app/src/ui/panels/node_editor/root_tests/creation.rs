use super::*;

#[test]
fn root_scoped_creation_expands_a_collapsed_composition() {
    let (mut project, composition_id, track_id, clip_id, _, _) = fixture();
    project.get_track_mut(track_id).unwrap().ui_collapsed = true;
    let hidden_track_body = egui::pos2(500.0, 400.0);
    project
        .get_composition_mut(composition_id)
        .unwrap()
        .ui_collapsed = true;
    let node = Node::new_merge("Root Node");
    let node_id = node.id;
    create_prebuilt_node(&mut project, hidden_track_body, node, composition_id);

    let composition = project.get_composition(composition_id).unwrap();
    assert!(!composition.ui_collapsed);
    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Composition(composition_id))
    );
    assert!(project.validate_containment().is_empty());
    let (snarl, _) = build_snarl(&project, composition_id);
    assert!(snarl.nodes().any(|item| *item == GraphItem::Node(node_id)));

    // The original Clip remains owned by the collapsed Track; creation did
    // not mutate hierarchy outside the new Node's single owner.
    assert_eq!(project.find_track_for_clip(clip_id), Some(track_id));
}

#[test]
fn locally_created_node_preserves_every_existing_position_and_stays_visible() {
    let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
    project.get_clip_mut(clip_id).unwrap().ui_collapsed = true;

    let composition_before = project.get_composition(composition_id).unwrap().clone();
    let track_before = project.get_track(track_id).unwrap().clone();
    let clip_before = project.get_clip(clip_id).unwrap().clone();
    let solid_before = project.get_node(solid_id).unwrap().clone();
    let merge_before = project.get_node(merge_id).unwrap().clone();
    let connections_before = project.connections.clone();
    let desired = egui::pos2(500.0, 400.0);
    let node = generator_node(
        "Locally Placed Text",
        GeneratorNodeRequest::Text {
            text: "Locally Placed Text".to_string(),
            font: "Arial".to_string(),
        },
    );
    let node_id = node.id;

    create_prebuilt_node(&mut project, desired, node, composition_id);

    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Track(track_id))
    );
    assert_eq!(
        project.get_composition(composition_id).unwrap().ui_position,
        composition_before.ui_position
    );
    assert_eq!(
        project.get_track(track_id).unwrap().ui_position,
        track_before.ui_position
    );
    assert_eq!(
        project.get_clip(clip_id).unwrap().ui_position,
        clip_before.ui_position
    );
    assert_eq!(
        project.get_node(solid_id).unwrap().ui_position,
        solid_before.ui_position
    );
    assert_eq!(
        project.get_node(merge_id).unwrap().ui_position,
        merge_before.ui_position
    );
    assert_eq!(project.connections, connections_before);

    let (snarl, _) = build_snarl(&project, composition_id);
    assert!(snarl.nodes().any(|item| *item == GraphItem::Node(node_id)));
}

#[test]
fn rendered_ports_stay_vertical_on_their_left_and_right_sides_with_bounded_width() {
    let (project, composition_id, _, _, node_id, _) = fixture();
    let context = egui::Context::default();
    reset_test_rects();

    for frame in 0..3 {
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1800.0, 1200.0),
            )),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        };
        drop(context.run(raw_input, |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let (mut snarl, containers) = build_snarl(&project, composition_id);
                let mut edits = Vec::new();
                let mut navigation = None;
                let mut selection = None;
                let mut wire_context_request = None;
                let mut context_menu_exclusion_rects = Vec::new();
                let mut to_global = egui::emath::TSTransform::default();
                let mut canvas_clip = ui.clip_rect();
                let mut merge_layer_reorder = None;
                let mut viewer = ProjectNodeViewer {
                    project: &project,
                    plugin_manager: None,
                    containers: &containers,
                    edits: &mut edits,
                    pending_navigation: &mut navigation,
                    pending_selection: &mut selection,
                    current_time: 0.0,
                    context_menu_exclusion_rects: &mut context_menu_exclusion_rects,
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
                let style = SnarlStyle {
                    collapsible: Some(false),
                    max_scale: Some(1.0),
                    ..Default::default()
                };
                snarl.show(
                    &mut viewer,
                    &style,
                    egui::Id::new(("node-layout-test", composition_id)),
                    ui,
                );
            });
        }));
    }

    let node = test_rect(&format!("node_editor.node:{node_id}")).expect("rendered node rect");
    let time_input = test_rect(&format!(
        "node_editor.port.node:{node_id}.input:{TIME_PORT}"
    ))
    .expect("time input pin rect");
    let image_output = test_rect(&format!(
        "node_editor.port.node:{node_id}.output:{IMAGE_OUTPUT_PORT}"
    ))
    .expect("image output pin rect");

    assert!(time_input.center().x < node.center().x);
    assert!(image_output.center().x > node.center().x);
    assert!(node.width() <= 500.0, "{node:?}");
}

#[test]
fn qa_geometry_is_clipped_to_the_actual_node_editor_canvas() {
    let canvas = egui::Rect::from_min_max(egui::pos2(100.0, 200.0), egui::pos2(500.0, 400.0));
    let partial = egui::Rect::from_min_max(egui::pos2(80.0, 250.0), egui::pos2(180.0, 450.0));
    assert_eq!(
        clipped_qa_rect(partial, canvas),
        egui::Rect::from_min_max(egui::pos2(100.0, 250.0), egui::pos2(180.0, 400.0))
    );

    let outside = egui::Rect::from_min_max(egui::pos2(700.0, 20.0), egui::pos2(760.0, 80.0));
    let clipped = clipped_qa_rect(outside, canvas);
    assert!(!clipped.is_positive());
    assert_eq!(clipped.min, egui::pos2(canvas.right(), canvas.top()));
    assert_eq!(clipped.max, clipped.min);
    assert!(canvas.contains(clipped.center()));
}
