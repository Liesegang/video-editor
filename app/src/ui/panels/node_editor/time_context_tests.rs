use super::test_fixture::fixture;
use super::*;
use library::model::project::TIME_PORT;

fn render_time_context_test_graph(
    project: &Project,
    composition_id: Uuid,
    nodes: &[TimeContextNode],
) -> (usize, Vec<RenderedEdgeKind>) {
    let context = egui::Context::default();
    let (mut snarl, containers) = build_snarl(project, composition_id);
    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
    let mut selected_node_ids = nodes
        .iter()
        .filter(|node| node.selected)
        .map(|node| node.node_id)
        .collect::<Vec<_>>();
    selected_node_ids.sort_unstable();
    let mut context_count = 0;
    let mut physical_kinds = Vec::new();
    reset_test_rects();

    for frame in 0..5 {
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
                rendered_ports.lock().unwrap().clear();
                let mut edits = Vec::new();
                let mut navigation = None;
                let mut wire_context_request = None;
                let mut exclusions = Vec::new();
                let mut to_global = egui::emath::TSTransform::default();
                let mut canvas_clip = ui.clip_rect();
                let mut merge_layer_reorder = None;
                let mut viewer = ProjectNodeViewer {
                    project,
                    plugin_manager: None,
                    containers: &containers,
                    edits: &mut edits,
                    pending_navigation: &mut navigation,
                    selected_node_ids: &selected_node_ids,
                    current_time: 0.0,
                    context_menu_exclusion_rects: &mut exclusions,
                    wire_context_request: &mut wire_context_request,
                    suppress_wire_connect: false,
                    locked_canvas_transform: None,
                    previous_canvas_transform: None,
                    to_global: &mut to_global,
                    canvas_clip: &mut canvas_clip,
                    rendered_ports: Arc::clone(&rendered_ports),
                    merge_layer_reorder: &mut merge_layer_reorder,
                    rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                };
                snarl.show(
                    &mut viewer,
                    &node_editor_snarl_style(),
                    egui::Id::new(("implicit-time-context-test", composition_id)),
                    ui,
                );
                drop(viewer);
                let physical = register_rendered_edges(project, &rendered_ports, canvas_clip, None);
                physical_kinds = physical.iter().map(|edge| edge.kind).collect();
                let painter = ui.painter().with_clip_rect(canvas_clip);
                context_count = register_implicit_time_context_wires(
                    project,
                    &rendered_ports,
                    nodes,
                    canvas_clip,
                    &painter,
                );
            });
        }));
    }
    (context_count, physical_kinds)
}

#[test]
fn implicit_time_context_wire_is_transient_qa_only_and_tracks_explicit_state() {
    let (mut project, composition_id, _, clip_id, solid_id, merge_id) = fixture();
    let initial = project.clone();
    let selected_merge = [TimeContextNode {
        node_id: merge_id,
        selected: true,
        hovered: false,
    }];

    let (implicit_count, physical_before) =
        render_time_context_test_graph(&project, composition_id, &selected_merge);
    assert_eq!(implicit_count, 1);
    let wire_id = format!("node_editor.time_context_wire.node:{merge_id}");
    assert!(test_rect(&wire_id).is_some_and(|rect| rect.is_positive()));
    let wire_metadata = test_metadata(&wire_id).expect("implicit Time wire QA metadata");
    assert_eq!(wire_metadata["kind"], "implicit_time");
    assert_eq!(wire_metadata["state"], "inherited");
    assert_eq!(wire_metadata["editable"], false);
    assert_eq!(wire_metadata["hit_testable"], false);
    assert_eq!(wire_metadata["wire_collection"], "context_only");
    assert_eq!(wire_metadata["selected"], true);
    let implicit_node_rect =
        test_rect(&format!("node_editor.node:{merge_id}")).expect("implicit Merge node geometry");
    let selected_node =
        test_metadata(&format!("node_editor.node:{merge_id}")).expect("selected Node QA metadata");
    assert_eq!(selected_node["selected"], true);
    assert_eq!(selected_node["highlight_style"]["state"], "selected");
    assert_eq!(
        selected_node["highlight_style"]["outer_stroke"]["width_screen"],
        3.0
    );

    let inherited_badge = test_metadata(&format!("node_editor.time_source.node:{merge_id}"))
        .expect("Merge Time source badge");
    assert_eq!(inherited_badge["state"], "inherited");
    assert!(inherited_badge["label"]
        .as_str()
        .is_some_and(|label| label.starts_with("Inherited · Clip Clip")));
    let explicit_badge = test_metadata(&format!("node_editor.time_source.node:{solid_id}"))
        .expect("Solid Time source badge");
    assert_eq!(explicit_badge["state"], "explicit");
    assert!(explicit_badge["label"]
        .as_str()
        .is_some_and(|label| label.starts_with("Explicit · Clip Clip.Time")));
    assert_eq!(project, initial, "rendering must not mutate Project");

    let connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(merge_id), TIME_PORT),
        )
        .unwrap();
    let (explicit_count, physical_explicit) =
        render_time_context_test_graph(&project, composition_id, &selected_merge);
    assert_eq!(explicit_count, 0, "authored Time hides the context wire");
    assert!(test_rect(&wire_id).is_none());
    assert_eq!(physical_explicit.len(), physical_before.len() + 1);
    let merge_badge = test_metadata(&format!("node_editor.time_source.node:{merge_id}"))
        .expect("explicit Merge Time badge");
    assert_eq!(merge_badge["state"], "explicit");
    let explicit_node_rect =
        test_rect(&format!("node_editor.node:{merge_id}")).expect("explicit Merge node geometry");
    assert_eq!(explicit_node_rect.size(), implicit_node_rect.size());

    assert!(project.disconnect_connection(connection_id));
    let (restored_count, physical_restored) =
        render_time_context_test_graph(&project, composition_id, &selected_merge);
    assert_eq!(restored_count, 1);
    assert_eq!(physical_restored, physical_before);
    assert_eq!(project, initial);
}
