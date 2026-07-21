use super::*;
use crate::test_support::{generator_node, media_node_for_canvas};
use library::editor::project_service::{GeneratorNodeRequest, MediaNodeRequest};
use library::model::frame::color::Color;
use library::model::project::{
    NodeContainer, PortAddress, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
    SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::Asset;
use library::plugin::PluginManager;

fn project_with_clip(name: &str) -> (Project, Uuid) {
    let mut project = Project::new("timeline semantic source");
    let clip = Clip::new(name, 0.0, 3.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    (project, clip_id)
}

fn attach_node(project: &mut Project, clip_id: Uuid, node: Node) -> Uuid {
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();
    node_id
}

fn style_result_project(source: Node) -> (Project, Uuid, Uuid, Uuid) {
    let (mut project, clip_id) = project_with_clip("styled source");
    // Storage order is not semantic authority. This disconnected source
    // deliberately precedes the connected one.
    attach_node(
        &mut project,
        clip_id,
        generator_node(
            "Unreachable",
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 100 V 100 Z".to_string(),
            },
        ),
    );
    let source_id = attach_node(&mut project, clip_id, source);
    let style = PluginManager::default()
        .create_style_operation_node("fill")
        .unwrap();
    let style_id = attach_node(&mut project, clip_id, style);
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
        )
        .unwrap();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(style_id))
        .unwrap();
    (project, clip_id, source_id, style_id)
}

#[test]
fn value_output_is_never_projected_as_a_timeline_image_source() {
    let (mut project, clip_id) = project_with_clip("value output");
    let value_id = attach_node(&mut project, clip_id, Node::new_fmod("Fmod"));
    project.get_clip_mut(clip_id).unwrap().output_node_id = Some(value_id);

    let graph = clip_graph_nodes(project.get_clip(clip_id).unwrap(), &project);
    assert_eq!(graph.output.map(|node| node.id), Some(value_id));
    assert!(graph.semantic_source.is_none());
}

#[test]
fn style_results_preserve_reachable_text_and_shape_semantics() {
    for (request, name, label, color) in [
        (
            GeneratorNodeRequest::Text {
                text: "Main title".to_string(),
                font: "Arial".to_string(),
            },
            "Main title",
            "Text · Main title",
            (200, 150, 100),
        ),
        (
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 100 V 100 Z".to_string(),
            },
            "Logo path",
            "Shape · Logo path",
            (200, 200, 100),
        ),
    ] {
        let source = generator_node(name, request);
        let (mut project, clip_id, source_id, style_id) = style_result_project(source);
        let clip = project.get_clip(clip_id).unwrap();
        let graph = clip_graph_nodes(clip, &project);
        assert_eq!(graph.output.map(|node| node.id), Some(style_id));
        assert_eq!(graph.semantic_source.map(|node| node.id), Some(source_id));
        assert_eq!(semantic_source_label(graph.semantic_source.unwrap()), label);
        assert_eq!(get_clip_color(graph.semantic_source, &project), color);

        project.get_node_mut(source_id).unwrap().enabled = false;
        let graph = clip_graph_nodes(project.get_clip(clip_id).unwrap(), &project);
        assert_eq!(graph.output.map(|node| node.id), Some(style_id));
        assert_eq!(graph.semantic_source.map(|node| node.id), Some(source_id));
    }
}

#[test]
fn effect_result_preserves_media_identity_while_nodes_are_disabled() {
    let (mut project, clip_id) = project_with_clip("effect media");
    let mut asset = Asset::new("dialog", "dialog.mov", AssetKind::Video);
    asset.stream_index = Some(2);
    let asset_id = asset.id;
    project.assets.push(asset);
    let media = media_node_for_canvas(
        "Dialog",
        MediaNodeRequest::Video {
            asset_id,
            file_path: "dialog.mov".to_string(),
            stream_index: Some(2),
            audio_stream_index: Some(7),
        },
        1920,
        1080,
        1920,
        1080,
    );
    let media_id = attach_node(&mut project, clip_id, media);
    let effect = PluginManager::default()
        .create_effect_operation_node("blur")
        .unwrap();
    let effect_id = attach_node(&mut project, clip_id, effect);
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(media_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
        )
        .unwrap();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(effect_id))
        .unwrap();

    let graph = clip_graph_nodes(project.get_clip(clip_id).unwrap(), &project);
    assert_eq!(graph.output.map(|node| node.id), Some(effect_id));
    assert_eq!(graph.semantic_source.map(|node| node.id), Some(media_id));
    assert_eq!(
        get_clip_color(graph.semantic_source, &project),
        (100, 100, 200)
    );
    let NodeContent::Media(media) = graph.semantic_source.unwrap().content() else {
        panic!("Effect result must resolve to its Media source")
    };
    assert_eq!(
        audio_stream_index_for_media(&project.assets[0], media),
        Some(7)
    );

    project.get_node_mut(effect_id).unwrap().enabled = false;
    let graph = clip_graph_nodes(project.get_clip(clip_id).unwrap(), &project);
    assert_eq!(graph.output.map(|node| node.id), Some(effect_id));
    assert_eq!(graph.semantic_source.map(|node| node.id), Some(media_id));

    project.get_node_mut(effect_id).unwrap().enabled = true;
    project.get_node_mut(media_id).unwrap().enabled = false;
    assert_eq!(
        clip_graph_nodes(project.get_clip(clip_id).unwrap(), &project)
            .semantic_source
            .map(|node| node.id),
        Some(media_id)
    );
}

#[test]
fn merge_semantic_source_follows_canonical_order_independent_of_enabled_state() {
    let (mut project, clip_id) = project_with_clip("multi input");
    let unreachable_id = attach_node(
        &mut project,
        clip_id,
        generator_node(
            "Unreachable text",
            GeneratorNodeRequest::Text {
                text: "Unreachable text".to_string(),
                font: "Arial".to_string(),
            },
        ),
    );
    let first_id = attach_node(
        &mut project,
        clip_id,
        generator_node(
            "First solid",
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        ),
    );
    let second_id = attach_node(
        &mut project,
        clip_id,
        generator_node(
            "Second solid",
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        ),
    );
    let merge_id = attach_node(&mut project, clip_id, Node::new_merge("Result"));
    let first_connection = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(first_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let second_connection = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .unwrap();

    let semantic_id = |project: &Project| {
        clip_graph_nodes(project.get_clip(clip_id).unwrap(), project)
            .semantic_source
            .map(|node| node.id)
    };
    assert_eq!(semantic_id(&project), Some(first_id));
    assert_ne!(semantic_id(&project), Some(unreachable_id));

    project.reorder_connection(second_connection, 0).unwrap();
    assert_eq!(semantic_id(&project), Some(second_id));
    project.get_node_mut(second_id).unwrap().enabled = false;
    assert_eq!(semantic_id(&project), Some(second_id));
    project.get_node_mut(first_id).unwrap().enabled = false;
    assert_eq!(semantic_id(&project), Some(second_id));

    assert!(project.disconnect_connection(first_connection));
}

fn expanded_track_project() -> (Project, Uuid, Vec<Uuid>) {
    let mut project = Project::new("timeline reorder");
    let mut track = Track::new("Track");
    let track_id = track.id;
    let clips = [
        Clip::new("A", 0.0, 1.0),
        Clip::new("B", 1.0, 1.0),
        Clip::new("C", 2.0, 1.0),
    ];
    let clip_ids = clips.iter().map(|clip| clip.id).collect::<Vec<_>>();
    track.clip_ids = clip_ids.clone();
    for clip in clips {
        project.add_clip(clip);
    }
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    (project, track_id, clip_ids)
}

#[test]
fn same_track_preview_matches_canonical_release_order() {
    let (project, track_id, clip_ids) = expanded_track_project();
    let expanded = HashSet::from([track_id]);
    let rows = flatten_tracks_to_rows(&project, &[track_id], &expanded);
    let preview = clip_reorder_preview(&project, clip_ids[0], track_id, track_id, clip_ids.len());
    assert!(preview.is_some());
    let Some(preview) = preview else {
        return;
    };
    assert_eq!(preview.source_index(), 0);
    assert_eq!(preview.destination_index(), 2);
    let projection = clip_reorder_projection(&rows, &project, preview);

    assert_eq!(projection.row_for_track(track_id), Some(0));
    assert_eq!(projection.row_for_clip(clip_ids[0]), Some(1));
    assert_eq!(projection.row_for_clip(clip_ids[2]), Some(2));
    assert_eq!(projection.row_for_clip(clip_ids[1]), Some(3));
    assert_eq!(
        destination_index_for_clip_slot(true, 0, clip_ids.len(), clip_ids.len()),
        Some(2)
    );
}

#[test]
fn cross_track_preview_reflows_both_groups_to_the_release_order() {
    let (mut project, source_track_id, source_clip_ids) = expanded_track_project();
    let target_clips = [Clip::new("D", 0.0, 1.0), Clip::new("E", 1.0, 1.0)];
    let target_clip_ids = target_clips.iter().map(|clip| clip.id).collect::<Vec<_>>();
    let mut target_track = Track::new("Front Track");
    let target_track_id = target_track.id;
    target_track.clip_ids = target_clip_ids.clone();
    for clip in target_clips {
        project.add_clip(clip);
    }
    assert!(project.add_track(target_track).is_ok());

    let canonical_track_ids = [source_track_id, target_track_id];
    let expanded = HashSet::from([source_track_id, target_track_id]);
    let rows = flatten_tracks_to_rows(&project, &canonical_track_ids, &expanded);
    let preview = clip_reorder_preview(
        &project,
        source_clip_ids[1],
        source_track_id,
        target_track_id,
        target_clip_ids.len(),
    );
    assert!(preview.is_some());
    let Some(preview) = preview else {
        return;
    };
    let projection = clip_reorder_projection(&rows, &project, preview);

    // Front Track is visually first. B is inserted at its canonical front
    // and therefore occupies the first Clip row under that header.
    assert_eq!(projection.row_for_track(target_track_id), Some(0));
    assert_eq!(projection.row_for_clip(source_clip_ids[1]), Some(1));
    assert_eq!(projection.row_for_clip(target_clip_ids[1]), Some(2));
    assert_eq!(projection.row_for_clip(target_clip_ids[0]), Some(3));
    // The source group closes the removed row and all following headers
    // shift to the exact row they will use after release.
    assert_eq!(projection.row_for_track(source_track_id), Some(4));
    assert_eq!(projection.row_for_clip(source_clip_ids[2]), Some(5));
    assert_eq!(projection.row_for_clip(source_clip_ids[0]), Some(6));
    assert_eq!(
        destination_index_for_clip_slot(false, 1, target_clip_ids.len(), 2),
        Some(2)
    );
}

#[test]
fn cross_track_preview_projects_the_dragged_clip_onto_a_collapsed_target_header() {
    let (mut project, source_track_id, source_clip_ids) = expanded_track_project();
    let target_clip = Clip::new("Collapsed target Clip", 0.0, 1.0);
    let target_clip_id = target_clip.id;
    let mut target_track = Track::new("Collapsed target");
    let target_track_id = target_track.id;
    target_track.clip_ids.push(target_clip_id);
    project.add_clip(target_clip);
    assert!(project.add_track(target_track).is_ok());

    let expanded = HashSet::from([source_track_id]);
    let rows = flatten_tracks_to_rows(&project, &[source_track_id, target_track_id], &expanded);
    let preview = clip_reorder_preview(
        &project,
        source_clip_ids[1],
        source_track_id,
        target_track_id,
        1,
    );
    assert!(preview.is_some());
    let Some(preview) = preview else {
        return;
    };
    let projection = clip_reorder_projection(&rows, &project, preview);

    assert_eq!(projection.row_for_track(target_track_id), Some(0));
    assert_eq!(projection.row_for_clip(target_clip_id), Some(0));
    assert_eq!(projection.row_for_clip(source_clip_ids[1]), Some(0));
    let dragged_source_row = rows.iter().find(
        |row| matches!(row, DisplayRow::ClipRow { clip, .. } if clip.id == source_clip_ids[1]),
    );
    assert!(dragged_source_row.is_some());
    if let Some(dragged_source_row) = dragged_source_row {
        assert_eq!(projection.row_for(dragged_source_row), Some(0));
    }
}

fn selection_geometry() -> ClipAreaGeometry {
    ClipAreaGeometry {
        content_rect: egui::Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(400.0, 300.0)),
        scroll_offset: egui::vec2(75.0, 32.0),
        pixels_per_unit: 100.0,
        row_height: 30.0,
        row_spacing: 2.0,
    }
}

#[test]
fn box_selection_matches_collapsed_clip_geometry_with_zoom_and_pan() {
    let (project, track_id, clip_ids) = expanded_track_project();
    // With horizontal pan, A is partially visible from x=100 to x=125.
    // Every collapsed Clip shares the Track header row at y=168..198.
    let selection_rect =
        egui::Rect::from_min_max(egui::pos2(100.0, 170.0), egui::pos2(120.0, 190.0));

    let selected = get_clips_in_box(
        selection_rect,
        BoxSelectionContext {
            project: &project,
            track_ids: &[track_id],
            expanded_tracks: &HashSet::new(),
            geometry: selection_geometry(),
        },
    );

    assert_eq!(selected, vec![(clip_ids[0], track_id)]);
}

#[test]
fn box_selection_checks_each_expanded_clip_only_on_its_visible_row() {
    let (project, track_id, clip_ids) = expanded_track_project();
    let expanded_tracks = HashSet::from([track_id]);
    // Expanded order is header, C, B, A. At this zoom and pan B occupies
    // x=125..225 and row 2 at y=232..262.
    let selection_rect =
        egui::Rect::from_min_max(egui::pos2(130.0, 235.0), egui::pos2(220.0, 258.0));

    let selected = get_clips_in_box(
        selection_rect,
        BoxSelectionContext {
            project: &project,
            track_ids: &[track_id],
            expanded_tracks: &expanded_tracks,
            geometry: selection_geometry(),
        },
    );

    assert_eq!(selected, vec![(clip_ids[1], track_id)]);
}

#[test]
fn failed_timing_update_cancels_gesture_without_history_or_preview_damage() {
    let project = Arc::new(RwLock::new(Project::new("timing failure")));
    let project_before = project.read().unwrap().clone();
    let mut history = HistoryManager::new();
    history.push_project_state(project_before.clone());

    let mut editor_context = EditorContext::new(Uuid::new_v4());
    begin_resize_gesture(&mut editor_context);
    mark_resize_timing_changed(&mut editor_context);
    editor_context.interaction.is_moving_selected_entity = true;
    editor_context.interaction.dragged_entity_original_track_id = Some(Uuid::new_v4());
    editor_context.interaction.dragged_entity_hovered_track_id = Some(Uuid::new_v4());
    editor_context.interaction.dragged_entity_has_moved = true;
    editor_context.preview_texture_id = Some(42);
    editor_context.preview_texture_width = 1920;
    editor_context.preview_texture_height = 1080;
    editor_context.preview_render_revision = 9;
    editor_context.preview_region = Some(library::model::frame::frame::Region {
        x: 10.0,
        y: 20.0,
        width: 640.0,
        height: 360.0,
    });
    let preview_before = (
        editor_context.preview_texture_id,
        editor_context.preview_texture_width,
        editor_context.preview_texture_height,
        editor_context.preview_render_revision,
        editor_context.preview_region,
    );

    // Move frame: the authoritative update fails and cancels the active
    // gesture. No history request is emitted in this frame.
    let mut move_frame_commit = ClipMutationCommit::default();
    apply_timing_update_result(
        Uuid::new_v4(),
        Err(library::LibraryError::Project(
            "Clip disappeared during drag".to_string(),
        )),
        &mut editor_context,
        &mut move_frame_commit,
    );
    push_clip_history_if_needed(&move_frame_commit, &project, &mut history);

    // Release frame: egui still reports `drag_stopped`, but the failure
    // reset from the previous frame prevents a no-op history snapshot.
    let mut release_frame_commit = ClipMutationCommit::default();
    if finish_resize_gesture(&mut editor_context) {
        release_frame_commit.timing_history_requested = true;
    }
    push_clip_history_if_needed(&release_frame_commit, &project, &mut history);

    assert!(move_frame_commit.timing_update_failed);
    assert!(!move_frame_commit.should_push_history());
    assert!(!release_frame_commit.should_push_history());
    assert_eq!(history.undo_depth(), 1);
    assert!(!editor_context.interaction.is_resizing_entity);
    assert!(!editor_context.interaction.is_moving_selected_entity);
    assert!(editor_context
        .interaction
        .dragged_entity_original_track_id
        .is_none());
    assert!(editor_context
        .interaction
        .dragged_entity_hovered_track_id
        .is_none());
    assert!(!editor_context.interaction.dragged_entity_has_moved);
    assert_eq!(
        (
            editor_context.preview_texture_id,
            editor_context.preview_texture_width,
            editor_context.preview_texture_height,
            editor_context.preview_render_revision,
            editor_context.preview_region,
        ),
        preview_before
    );
    assert_eq!(*project.read().unwrap(), project_before);
}

#[test]
fn clip_move_result_preserves_typed_clip_selection() {
    let composition_id = Uuid::new_v4();
    let clip_id = Uuid::new_v4();
    let mut editor_context = EditorContext::new(composition_id);
    editor_context.select_target(SelectionTarget::Clip(clip_id));
    let mut commit = ClipMutationCommit::default();

    apply_move_clip_result(clip_id, Ok(()), &mut commit);

    assert!(commit.persistent_change);
    assert_eq!(
        editor_context.selection.primary(),
        Some(SelectionTarget::Clip(clip_id))
    );

    let mut failed_commit = ClipMutationCommit::default();
    apply_move_clip_result(
        clip_id,
        Err(library::LibraryError::Project("rejected move".to_string())),
        &mut failed_commit,
    );

    assert!(!failed_commit.persistent_change);
    assert_eq!(
        editor_context.selection.primary(),
        Some(SelectionTarget::Clip(clip_id))
    );
}

#[test]
fn resize_click_and_invalid_zero_change_drag_do_not_create_history() {
    let project = Arc::new(RwLock::new(Project::new("zero change resize")));
    let initial = project.read().unwrap().clone();
    let mut history = HistoryManager::new();
    history.push_project_state(initial);
    let mut editor_context = EditorContext::new(Uuid::new_v4());

    // Press/release without movement.
    begin_resize_gesture(&mut editor_context);
    let click_release = ClipMutationCommit {
        timing_history_requested: finish_resize_gesture(&mut editor_context),
        ..ClipMutationCommit::default()
    };
    push_clip_history_if_needed(&click_release, &project, &mut history);

    // A drag beyond a valid timing boundary never queues an update, so it
    // likewise never marks the gesture as changed.
    begin_resize_gesture(&mut editor_context);
    let invalid_drag_release = ClipMutationCommit {
        timing_history_requested: finish_resize_gesture(&mut editor_context),
        ..ClipMutationCommit::default()
    };
    push_clip_history_if_needed(&invalid_drag_release, &project, &mut history);

    assert!(!click_release.should_push_history());
    assert!(!invalid_drag_release.should_push_history());
    assert_eq!(history.undo_depth(), 1);

    // The positive path remains explicit: a queued timing update marks
    // the release as commit-worthy exactly once.
    begin_resize_gesture(&mut editor_context);
    mark_resize_timing_changed(&mut editor_context);
    assert!(finish_resize_gesture(&mut editor_context));
    assert!(!finish_resize_gesture(&mut editor_context));
}

#[test]
fn expanded_track_exposes_every_canonical_insertion_slot_in_reverse_screen_order() {
    let (project, track_id, _) = expanded_track_project();
    let rows = flatten_tracks_to_rows(&project, &[track_id], &HashSet::from([track_id]));
    let markers = clip_insertion_markers(
        &rows,
        track_id,
        &project,
        ClipRowLayout {
            content_min_y: 100.0,
            scroll_y: 30.0,
            row_height: 30.0,
            row_spacing: 2.0,
        },
    );

    assert_eq!(
        markers,
        vec![(0, 198.0), (1, 166.0), (2, 134.0), (3, 102.0)]
    );
    assert_eq!(nearest_clip_insertion_slot(103.0, &markers), Some(3));
    assert_eq!(nearest_clip_insertion_slot(197.0, &markers), Some(0));
}

#[test]
fn horizontal_same_track_drop_is_a_noop_but_vertical_slot_reorders() {
    // A is index 0. Its adjacent slots (0 and 1) retain its order while
    // the slot after C detaches A and inserts it at destination index 2.
    assert_eq!(destination_index_for_clip_slot(true, 0, 0, 3), None);
    assert_eq!(destination_index_for_clip_slot(true, 0, 1, 3), None);
    assert_eq!(destination_index_for_clip_slot(true, 0, 3, 3), Some(2));
    assert_eq!(destination_index_for_clip_slot(true, 2, 0, 3), Some(0));

    // Cross-Track slots are already destination indices because the
    // source is detached from a different list.
    assert_eq!(destination_index_for_clip_slot(false, 0, 2, 2), Some(2));
}

#[test]
fn converted_same_track_slot_produces_the_expected_authoritative_order() {
    let (mut project, track_id, clip_ids) = expanded_track_project();
    let destination = destination_index_for_clip_slot(true, 0, 3, 3).unwrap();
    project
        .attach_clip_to_track_at(track_id, clip_ids[0], Some(destination))
        .unwrap();

    assert_eq!(
        project.get_track(track_id).unwrap().clip_ids,
        vec![clip_ids[1], clip_ids[2], clip_ids[0]]
    );
}

#[test]
fn left_edge_trim_keeps_the_content_frame_at_the_new_boundary() {
    let mut clip = Clip::new("trim", 2.0, 6.0);
    clip.trim_in = ordered_float::OrderedFloat(1.5);
    clip.time_stretch = ordered_float::OrderedFloat(1.75);
    let delta = 0.8;
    let expected_local_time_at_new_boundary = clip.local_time(2.0 + delta);

    let timing = timing_after_left_edge_drag(&clip, delta).unwrap();
    assert!((timing.start_time - 2.8).abs() < 1e-9);
    assert!((timing.duration - 5.2).abs() < 1e-9);
    assert!((timing.trim_in - expected_local_time_at_new_boundary).abs() < 1e-9);

    clip.start_time = ordered_float::OrderedFloat(timing.start_time);
    clip.duration = ordered_float::OrderedFloat(timing.duration);
    clip.trim_in = ordered_float::OrderedFloat(timing.trim_in);
    assert!(
        (clip.local_time(timing.start_time) - expected_local_time_at_new_boundary).abs() < 1e-9
    );
}

#[test]
fn left_edge_trim_rejects_negative_source_or_empty_duration() {
    let mut clip = Clip::new("trim", 2.0, 1.0);
    clip.trim_in = ordered_float::OrderedFloat(0.25);
    clip.time_stretch = ordered_float::OrderedFloat(1.0);
    assert!(timing_after_left_edge_drag(&clip, 1.0).is_none());
    assert!(timing_after_left_edge_drag(&clip, -0.5).is_none());
}

#[test]
fn body_drag_applies_frame_delta_without_changing_source_timing() {
    let mut clip = Clip::new("move", 2.0, 6.0);
    clip.trim_in = ordered_float::OrderedFloat(1.5);
    let timing = timing_after_body_drag(&clip, 0.75).unwrap();

    assert_eq!(timing.start_time, 2.75);
    assert_eq!(timing.duration, 6.0);
    assert_eq!(timing.trim_in, 1.5);
    assert!(timing_after_body_drag(&clip, 0.0).is_none());

    clip.start_time = ordered_float::OrderedFloat(0.25);
    let clamped = timing_after_body_drag(&clip, -1.0).unwrap();
    assert_eq!(clamped.start_time, 0.0);
}

#[test]
fn raw_pointer_drag_keeps_motion_before_egui_claims_the_gesture() {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 160.0));
    let start = egui::pos2(60.0, 80.0);
    let mut applied_x = 0.0;
    let mut started_count = 0;
    let frames = [
        vec![egui::Event::PointerMoved(start)],
        vec![egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerMoved(start + egui::vec2(4.0, 0.0))],
        vec![egui::Event::PointerMoved(start + egui::vec2(12.0, 0.0))],
        vec![egui::Event::PointerMoved(start + egui::vec2(24.0, 0.0))],
        vec![egui::Event::PointerButton {
            pos: start + egui::vec2(24.0, 0.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];

    for (frame, events) in frames.into_iter().enumerate() {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(frame as f64 / 60.0),
            events,
            ..egui::RawInput::default()
        };
        let _output = context.run(input, |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let response = ui.interact(
                    egui::Rect::from_min_max(egui::pos2(20.0, 40.0), egui::pos2(220.0, 120.0)),
                    egui::Id::new("timeline_clip_drag"),
                    egui::Sense::drag(),
                );
                if response.drag_started() {
                    started_count += 1;
                }
                applied_x += timeline_drag_delta(&response).x;
            });
        });
    }

    assert_eq!(started_count, 1);
    assert!((applied_x - 24.0).abs() < 1.0e-5, "applied {applied_x}");
}
