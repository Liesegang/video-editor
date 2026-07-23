use super::*;
use crate::state::context::EditorContext;
use crate::state::context_types::{NodeEditorPendingEdit, SelectionTarget};

#[allow(
    clippy::too_many_arguments,
    reason = "the headless production-panel frame owns the complete app interaction boundary"
)]
fn run_node_editor_panel_frame(
    context: &egui::Context,
    screen: egui::Rect,
    frame: usize,
    events: Vec<egui::Event>,
    project: &Arc<RwLock<Project>>,
    service: &EditorService,
    history: &mut HistoryManager,
    editor_context: &mut EditorContext,
) {
    reset_test_rects();
    let command_registry =
        crate::command::CommandRegistry::new(&crate::config::AppConfig::new());
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(frame as f64 / 60.0),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                node_editor_panel(
                    ui,
                    project,
                    service,
                    history,
                    editor_context,
                    &command_registry,
                );
            });
        },
    ));
}

fn primary_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn escape_key() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[derive(Clone, Copy)]
enum MoveCancellationInput {
    Escape,
    PointerGone,
}

fn assert_production_move_cancellation(
    cancellation: MoveCancellationInput,
    isolate_unrelated_edit: bool,
) {
    let (mut project, composition_id, track_id, source_clip_id, solid_id, merge_id) = fixture();
    project.get_composition_mut(composition_id).unwrap().ui_size = [1_900.0, 1_000.0];
    project.get_track_mut(track_id).unwrap().ui_size = [1_600.0, 720.0];
    let mut target_clip = library::model::Clip::new("Cancellation target", 0.0, 5.0);
    target_clip.ui_position = [1_100.0, 260.0];
    target_clip.ui_size = [500.0, 480.0];
    let target_clip_id = target_clip.id;
    project.add_clip(target_clip);
    project
        .attach_clip_to_track(track_id, target_clip_id)
        .unwrap();

    let project = Arc::new(RwLock::new(project));
    let service = EditorService::new(
        Arc::clone(&project),
        Arc::new(PluginManager::default()),
        Arc::new(library::cache::CacheManager::new()),
    )
    .expect("production EditorService");
    let mut history = HistoryManager::new();
    history.push_project_state(project.read().unwrap().clone());
    let mut editor_context = EditorContext::new(composition_id);
    editor_context.select_target(SelectionTarget::Node(merge_id));
    editor_context
        .node_editor_state
        .repaired_compositions
        .insert(composition_id);
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(3_000.0, 1_800.0));

    for frame in 0..6 {
        run_node_editor_panel_frame(
            &context,
            screen,
            frame,
            Vec::new(),
            &project,
            &service,
            &mut history,
            &mut editor_context,
        );
    }
    let source_header = test_rect(&format!("node_editor.node_header:{merge_id}"))
        .filter(|rect| rect.is_positive())
        .expect("production Node header geometry");
    let source_node = test_rect(&format!("node_editor.node:{merge_id}"))
        .filter(|rect| rect.is_positive())
        .expect("production Node geometry");
    let target = test_rect(&format!("node_editor.container.clip:{target_clip_id}"))
        .filter(|rect| rect.is_positive())
        .expect("production target Clip geometry");
    let start = source_header.center();
    let end = target.center() + (start - source_node.center());

    let before_move = project.read().unwrap().clone();
    let history_before_unrelated = history.undo_depth();
    if isolate_unrelated_edit {
        project
            .write()
            .unwrap()
            .get_node_mut(solid_id)
            .unwrap()
            .name = "Unrelated pending name".to_string();
        editor_context.node_editor_state.pending_continuous_edit = Some(NodeEditorPendingEdit {
            owner: PortOwner::Node(solid_id),
            key: "name".to_string(),
        });
    }
    let movement_base = project.read().unwrap().clone();

    run_node_editor_panel_frame(
        &context,
        screen,
        6,
        vec![
            egui::Event::PointerMoved(start),
            primary_button(start, true),
            egui::Event::PointerMoved(end),
        ],
        &project,
        &service,
        &mut history,
        &mut editor_context,
    );
    let moved_before_cancel = project.read().unwrap().clone();
    assert_ne!(moved_before_cancel, movement_base);
    assert_eq!(
        moved_before_cancel.find_node_container(merge_id),
        Some(NodeContainer::Clip(source_clip_id))
    );
    assert_eq!(
        editor_context
            .node_editor_state
            .node_reparent
            .as_ref()
            .and_then(|gesture| gesture.hovered_target),
        Some(NodeContainer::Clip(target_clip_id)),
        "the held pointer must be over a legal release-only reparent target"
    );
    let history_before_cancel = history.undo_depth();
    assert_eq!(
        history_before_cancel,
        history_before_unrelated + usize::from(isolate_unrelated_edit),
        "a pending unrelated edit must close before movement is applied"
    );

    let cancel_event = match cancellation {
        MoveCancellationInput::Escape => escape_key(),
        MoveCancellationInput::PointerGone => egui::Event::PointerGone,
    };
    run_node_editor_panel_frame(
        &context,
        screen,
        7,
        vec![cancel_event],
        &project,
        &service,
        &mut history,
        &mut editor_context,
    );
    let cancelled = project.read().unwrap().clone();
    assert_eq!(
        cancelled, moved_before_cancel,
        "cancellation must retain live positions"
    );
    assert_eq!(
        cancelled.find_node_container(merge_id),
        Some(NodeContainer::Clip(source_clip_id)),
        "cancellation must not reparent onto the hovered target"
    );
    assert_eq!(cancelled.connections, movement_base.connections);
    assert_eq!(history.undo_depth(), history_before_cancel + 1);
    assert_eq!(history.redo_depth(), 0);
    assert!(editor_context.node_editor_state.node_reparent.is_none());
    assert!(!editor_context
        .node_editor_state
        .surface_interaction
        .is_active());
    assert!(!editor_context.node_editor_state.layout_changed_during_drag);

    // The physical button can still release after Escape/pointer loss. It is
    // inert because the typed cancellation already closed the transaction.
    run_node_editor_panel_frame(
        &context,
        screen,
        8,
        vec![primary_button(end, false)],
        &project,
        &service,
        &mut history,
        &mut editor_context,
    );
    assert_eq!(*project.read().unwrap(), cancelled);
    assert_eq!(history.undo_depth(), history_before_cancel + 1);

    assert_eq!(history.undo(&cancelled), Some(movement_base.clone()));
    assert_eq!(history.redo(&movement_base), Some(cancelled.clone()));
    if isolate_unrelated_edit {
        assert_ne!(movement_base, before_move);
        assert_eq!(history.undo(&cancelled), Some(movement_base));
    }
}

#[test]
fn real_node_header_capture_includes_the_visual_frame_padding() {
    let (project, ids) = adversarial_hierarchy_fixture();
    render_test_graph(&project, ids.composition);
    let component_id = format!("node_editor.node_header:{}", ids.solid);
    let metadata = test_metadata(&component_id).unwrap();
    let rect = |key: &str| {
        let value = &metadata[key];
        egui::Rect::from_min_max(
            egui::pos2(
                value["min_x"].as_f64().unwrap() as f32,
                value["min_y"].as_f64().unwrap() as f32,
            ),
            egui::pos2(
                value["max_x"].as_f64().unwrap() as f32,
                value["max_y"].as_f64().unwrap() as f32,
            ),
        )
    };
    let visual = test_rect(&component_id).unwrap();
    let content = rect("content_rect");
    let padding_point = egui::pos2(content.center().x, visual.top() + 1.0);

    assert!(visual.contains(padding_point));
    assert!(!content.contains(padding_point));
    assert!(visual.width() > content.width());
    assert!(visual.height() > content.height());
}

#[test]
fn production_container_metadata_exposes_selected_visual_for_all_group_kinds() {
    let (project, composition_id, track_id, clip_id, _, _) = fixture();
    let (_, containers) = build_snarl(&project, composition_id);
    reset_test_rects();
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(3_000.0, 2_000.0));

    for container in &containers {
        register_container_chrome(
            container,
            egui::emath::TSTransform::IDENTITY,
            canvas,
            &project,
            0.0,
            true,
        );
    }

    for owner in [
        PortOwner::Composition(composition_id),
        PortOwner::Track(track_id),
        PortOwner::Clip(clip_id),
    ] {
        let id = format!("node_editor.container.{}", qa_container_key(owner));
        let metadata = test_metadata(&id).expect("selected container QA metadata");
        assert_eq!(metadata["selected"], true);
        assert_eq!(metadata["highlight_style"]["state"], "selected");
        assert_eq!(
            metadata["highlight_style"]["outer_stroke"]["width_screen"],
            3.0
        );
        let move_id = format!(
            "node_editor.container_move_header.{}",
            qa_container_key(owner)
        );
        let move_header = test_rect(&move_id).expect("generic Group move header QA geometry");
        assert_eq!(move_header.height(), CONTAINER_HEADER_HEIGHT);
        let move_metadata = test_metadata(&move_id).expect("detail Group move metadata");
        assert_eq!(move_metadata["selection_enabled"], true);
        assert_eq!(move_metadata["move_enabled"], true);
    }

    reset_test_rects();
    let overview = egui::emath::TSTransform::new(egui::Vec2::ZERO, NODE_EDITOR_MIN_SCALE);
    for container in &containers {
        register_container_chrome(container, overview, canvas, &project, 0.0, true);
    }
    for owner in [
        PortOwner::Composition(composition_id),
        PortOwner::Track(track_id),
        PortOwner::Clip(clip_id),
    ] {
        let move_id = format!(
            "node_editor.container_move_header.{}",
            qa_container_key(owner)
        );
        let metadata = test_metadata(&move_id).expect("overview Group move metadata");
        assert_eq!(metadata["selection_enabled"], true);
        assert_eq!(metadata["move_enabled"], false);
    }
}

#[test]
fn production_panel_header_drag_commits_once_and_round_trips_history() {
    let (project, composition_id, _track_id, clip_id, _solid_id, merge_id) = fixture();
    let initial_position = project.get_node(merge_id).unwrap().ui_position;
    let initial = project.clone();
    let project = Arc::new(RwLock::new(project));
    let service = EditorService::new(
        Arc::clone(&project),
        Arc::new(PluginManager::default()),
        Arc::new(library::cache::CacheManager::new()),
    )
    .expect("production EditorService");
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    let mut editor_context = EditorContext::new(composition_id);
    editor_context.select_target(SelectionTarget::Node(merge_id));
    editor_context
        .node_editor_state
        .repaired_compositions
        .insert(composition_id);
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2_400.0, 1_600.0));

    for frame in 0..6 {
        run_node_editor_panel_frame(
            &context,
            screen,
            frame,
            Vec::new(),
            &project,
            &service,
            &mut history,
            &mut editor_context,
        );
    }
    let header = test_rect(&format!("node_editor.node_header:{merge_id}"))
        .filter(|rect| rect.is_positive())
        .expect("production Node header geometry");
    let header_metadata = test_metadata(&format!("node_editor.node_header:{merge_id}"))
        .expect("detail Node header interaction metadata");
    assert_eq!(header_metadata["selection_enabled"], true);
    assert_eq!(header_metadata["move_enabled"], true);
    let transform = editor_context
        .node_editor_state
        .node_editor_canvas_transform
        .expect("production Snarl transform");
    let start = header.center();
    let final_position = [initial_position[0] + 42.0, initial_position[1] + 24.0];
    let graph_delta = egui::vec2(
        final_position[0] - initial_position[0],
        final_position[1] - initial_position[1],
    );
    let end = start + graph_delta * transform.scaling;
    let pointer_frames = [
        vec![egui::Event::PointerMoved(start)],
        vec![
            egui::Event::PointerMoved(start),
            primary_button(start, true),
            egui::Event::PointerMoved(end),
        ],
        vec![primary_button(end, false)],
    ];
    for (offset, events) in pointer_frames.into_iter().enumerate() {
        run_node_editor_panel_frame(
            &context,
            screen,
            6 + offset,
            events,
            &project,
            &service,
            &mut history,
            &mut editor_context,
        );
    }

    let edited = project.read().unwrap().clone();
    let node = edited.get_node(merge_id).unwrap();
    assert_eq!(
        edited.find_node_container(merge_id),
        Some(NodeContainer::Clip(clip_id))
    );
    assert!(
        (node.ui_position[0] - final_position[0]).abs() < 0.01,
        "x position {:?}, expected {final_position:?}",
        node.ui_position
    );
    assert!(
        (node.ui_position[1] - final_position[1]).abs() < 0.01,
        "y position {:?}, expected {final_position:?}",
        node.ui_position
    );
    assert!(edited.validate_connections().is_empty());
    assert_eq!(history.undo_depth(), 2);
    assert_eq!(history.undo(&edited), Some(initial.clone()));
    assert_eq!(history.redo(&initial), Some(edited));
}

#[test]
fn production_escape_after_delta_commits_movement_only_and_isolates_unrelated_edit() {
    assert_production_move_cancellation(MoveCancellationInput::Escape, true);
}

#[test]
fn production_pointer_loss_after_delta_commits_movement_only_once() {
    assert_production_move_cancellation(MoveCancellationInput::PointerGone, false);
}
