use egui::Ui;
#[cfg(test)]
use library::audio::mixer::audio_stream_index_for_media;
use library::model::asset::AssetKind;
use library::model::project::{PortOwner, Project};
use library::model::{Clip, Node, NodeContent, Track};
use library::EditorService as ProjectService;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{
    action::HistoryManager,
    state::{context::EditorContext, context_types::SelectionTarget},
    ui::layer_order::reverse_slot,
};

use super::super::utils::flatten::{flatten_tracks_to_rows, DisplayRow};
use super::reorder::{
    calculate_insert_index, clip_insertion_markers, clip_reorder_preview, clip_reorder_projection,
    destination_index_for_clip_slot, nearest_clip_insertion_slot, ClipReorderProjection,
};

mod item;

use item::{draw_single_clip, SingleClipDrawContext};

const EDGE_DRAG_WIDTH: f32 = 5.0;

#[derive(Clone, Copy)]
pub(crate) struct ClipRowLayout {
    pub(crate) content_min_y: f32,
    pub(crate) scroll_y: f32,
    pub(crate) row_height: f32,
    pub(crate) row_spacing: f32,
}

impl ClipRowLayout {
    pub(super) fn row_step(self) -> f32 {
        self.row_height + self.row_spacing
    }
}

#[derive(Clone, Copy)]
pub(super) struct ClipAreaGeometry {
    pub(super) content_rect: egui::Rect,
    pub(super) scroll_offset: egui::Vec2,
    pub(super) pixels_per_unit: f32,
    pub(super) row_height: f32,
    pub(super) row_spacing: f32,
}

impl ClipAreaGeometry {
    fn row_layout(self) -> ClipRowLayout {
        ClipRowLayout {
            content_min_y: self.content_rect.min.y,
            scroll_y: self.scroll_offset.y,
            row_height: self.row_height,
            row_spacing: self.row_spacing,
        }
    }

    fn clip_rect(self, start_time: f64, duration: f64, row_index: usize) -> egui::Rect {
        let initial_x = self.content_rect.min.x + start_time as f32 * self.pixels_per_unit
            - self.scroll_offset.x;
        let initial_y = self.content_rect.min.y - self.scroll_offset.y
            + row_index as f32 * (self.row_height + self.row_spacing);
        let width = (duration as f32 * self.pixels_per_unit).max(1.0);

        egui::Rect::from_min_size(
            egui::pos2(initial_x, initial_y),
            egui::vec2(width, self.row_height),
        )
    }
}

/// Deferred actions collected during UI phase, executed after read lock is released
#[derive(Debug)]
enum DeferredClipAction {
    /// Update clip timing (resize/move start)
    UpdateClipTiming {
        clip_id: Uuid,
        new_start_time: f64,
        new_duration: f64,
        new_trim_in: f64,
    },
    MoveClip {
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
        target_index: Option<usize>,
    },

    /// Remove clip from track
    RemoveClip { track_id: Uuid, clip_id: Uuid },
    /// Push history state after changes
    PushHistory,
}

fn collect_track_clips<'a>(project: &'a Project, track: &'a Track, clips: &mut Vec<&'a Clip>) {
    for clip_id in &track.clip_ids {
        if let Some(clip) = project.get_clip(*clip_id) {
            clips.push(clip);
        }
    }
}

#[derive(Clone, Copy)]
struct ClipGraphNodes<'a> {
    /// The authored Clip result binding. This can be a Style, Effect, or
    /// Merge and is deliberately not used as the Timeline's semantic label.
    output: Option<&'a Node>,
    /// The first authored visual source feeding `output`, following canonical
    /// typed connections while ignoring runtime enabled/range state.
    semantic_source: Option<&'a Node>,
}

fn clip_graph_nodes<'a>(clip: &Clip, project: &'a Project) -> ClipGraphNodes<'a> {
    let semantics = project.container_graph_semantics(PortOwner::Clip(clip.id));
    let output = semantics
        .explicit_output_node_id()
        .filter(|_| semantics.explicit_output_is_directly_contained())
        .and_then(|node_id| project.get_node(node_id));
    let semantic_source = semantics
        .authored_source_node_id()
        .and_then(|node_id| project.get_node(node_id));
    ClipGraphNodes {
        output,
        semantic_source,
    }
}

fn semantic_source_kind(node: &Node) -> &'static str {
    match node.content() {
        NodeContent::Media(_) => "Media",
        NodeContent::Generator(library::model::GeneratorContent::Text) => "Text",
        NodeContent::Generator(library::model::GeneratorContent::Shape) => "Shape",
        NodeContent::Generator(library::model::GeneratorContent::SkSL) => "Shader",
        NodeContent::Generator(library::model::GeneratorContent::Solid) => "Solid",
        NodeContent::CompositionInstance(_) => "Composition Instance",
        NodeContent::PluginOperation(_) | NodeContent::Merge | NodeContent::SoundMerge => "Result",
        NodeContent::Value(_) => "Value",
        NodeContent::List(_) => "List",
        NodeContent::SoundAnalysis(_) => "Analysis",
        NodeContent::NativeOperation(_) => "Native Operation",
    }
}

fn semantic_source_label(node: &Node) -> String {
    let kind = semantic_source_kind(node);
    match node.content() {
        NodeContent::Generator(library::model::GeneratorContent::Text)
        | NodeContent::Generator(library::model::GeneratorContent::Shape) => {
            if node.name.eq_ignore_ascii_case(kind) {
                kind.to_string()
            } else {
                format!("{kind} · {}", node.name)
            }
        }
        _ => node.name.clone(),
    }
}

fn get_clip_color(source: Option<&Node>, project: &Project) -> (u8, u8, u8) {
    match source.map(Node::content) {
        Some(NodeContent::Media(m)) => {
            if let Some(asset) = project.assets.iter().find(|a| a.id == m.asset_id) {
                match asset.kind {
                    AssetKind::Audio => (100, 200, 100),
                    AssetKind::Video => (100, 100, 200),
                    AssetKind::Image => (150, 100, 200),
                    _ => (150, 150, 150),
                }
            } else {
                (200, 50, 50) // Missing asset
            }
        }
        Some(NodeContent::Generator(generator)) => match generator {
            library::model::GeneratorContent::Shape => (200, 200, 100),
            library::model::GeneratorContent::Text => (200, 150, 100),
            library::model::GeneratorContent::SkSL => (100, 200, 200),
            library::model::GeneratorContent::Solid => (150, 150, 150),
        },
        Some(NodeContent::PluginOperation(_)) => (180, 110, 210),
        Some(NodeContent::Value(_)) => (90, 180, 200),
        Some(NodeContent::List(_)) => (90, 190, 145),
        Some(NodeContent::NativeOperation(_)) => (210, 145, 90),
        Some(NodeContent::CompositionInstance(_) | NodeContent::Merge) | None => (150, 150, 150),
        Some(NodeContent::SoundMerge) => (170, 135, 205),
        Some(NodeContent::SoundAnalysis(_)) => (120, 190, 205),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ClipTiming {
    start_time: f64,
    duration: f64,
    trim_in: f64,
}

fn timing_after_left_edge_drag(clip: &Clip, delta_time: f64) -> Option<ClipTiming> {
    let start_time = clip.start_time.into_inner() + delta_time;
    let duration = clip.duration.into_inner() - delta_time;
    // local_time(t) = (t - start) * time_stretch + trim_in. At the new
    // boundary, preserving content therefore advances trim by delta*stretch.
    let trim_in = clip.trim_in.into_inner() + delta_time * clip.time_stretch.into_inner();
    (start_time >= 0.0 && duration > 0.0 && trim_in >= 0.0).then_some(ClipTiming {
        start_time,
        duration,
        trim_in,
    })
}

fn timing_after_body_drag(clip: &Clip, delta_time: f64) -> Option<ClipTiming> {
    let start_time = (clip.start_time.into_inner() + delta_time).max(0.0);
    (start_time != clip.start_time.into_inner()).then_some(ClipTiming {
        start_time,
        duration: clip.duration.into_inner(),
        trim_in: clip.trim_in.into_inner(),
    })
}

/// Return the pointer movement not yet applied to the authoritative Clip.
///
/// `Response::drag_delta` only reports movement from the current frame. egui
/// does not claim a drag until its threshold is crossed, so those first few
/// points would otherwise be discarded. On the transition frame the total
/// delta includes that pre-threshold movement; later frames remain
/// incremental and can be applied directly to the current Project value.
fn timeline_drag_delta(response: &egui::Response) -> egui::Vec2 {
    if response.drag_started() {
        response
            .total_drag_delta()
            .unwrap_or_else(|| response.drag_delta())
    } else {
        response.drag_delta()
    }
}

pub(super) struct DrawClipsContext<'a> {
    pub(super) editor_context: &'a mut EditorContext,
    pub(super) project_service: &'a mut ProjectService,
    pub(super) history_manager: &'a mut HistoryManager,
    pub(super) project: &'a Arc<RwLock<Project>>,
    pub(super) track_ids: &'a [Uuid],
    pub(super) geometry: ClipAreaGeometry,
}

#[derive(Default)]
struct ClipMutationCommit {
    persistent_change: bool,
    timing_history_requested: bool,
    timing_update_failed: bool,
}

impl ClipMutationCommit {
    fn should_push_history(&self) -> bool {
        self.persistent_change || (self.timing_history_requested && !self.timing_update_failed)
    }
}

fn begin_resize_gesture(editor_context: &mut EditorContext) {
    editor_context.interaction.is_resizing_entity = true;
    editor_context.interaction.dragged_entity_has_moved = false;
}

fn mark_resize_timing_changed(editor_context: &mut EditorContext) {
    editor_context.interaction.dragged_entity_has_moved = true;
}

fn finish_resize_gesture(editor_context: &mut EditorContext) -> bool {
    let should_commit = editor_context.interaction.is_resizing_entity
        && editor_context.interaction.dragged_entity_has_moved;
    editor_context.interaction.is_resizing_entity = false;
    editor_context.interaction.dragged_entity_has_moved = false;
    should_commit
}

fn cancel_failed_timing_gesture(editor_context: &mut EditorContext) {
    editor_context.interaction.is_resizing_entity = false;
    editor_context.interaction.is_moving_selected_entity = false;
    editor_context.interaction.dragged_entity_original_track_id = None;
    editor_context.interaction.dragged_entity_hovered_track_id = None;
    editor_context.interaction.dragged_entity_has_moved = false;
}

fn apply_timing_update_result(
    clip_id: Uuid,
    result: Result<(), library::LibraryError>,
    editor_context: &mut EditorContext,
    commit: &mut ClipMutationCommit,
) {
    if let Err(error) = result {
        log::error!("Failed to update timeline Clip {clip_id} timing: {error}");
        commit.timing_update_failed = true;
        cancel_failed_timing_gesture(editor_context);
    }
}

fn apply_move_clip_result(
    clip_id: Uuid,
    result: Result<(), library::LibraryError>,
    commit: &mut ClipMutationCommit,
) {
    match result {
        Ok(()) => {
            commit.persistent_change = true;
        }
        Err(error) => log::error!("Failed to move timeline Clip {clip_id}: {error}"),
    }
}

fn push_clip_history_if_needed(
    commit: &ClipMutationCommit,
    project: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) {
    if !commit.should_push_history() {
        return;
    }

    match project.read() {
        Ok(project) => history_manager.push_project_state(project.clone()),
        Err(error) => log::error!("Failed to snapshot timeline edit history: {error}"),
    }
}

pub(super) fn draw_clips(ui_content: &mut Ui, context: DrawClipsContext<'_>) -> bool {
    let DrawClipsContext {
        editor_context,
        project_service,
        history_manager,
        project,
        track_ids,
        geometry,
    } = context;
    let mut clicked_on_entity = false;
    let mut deferred_actions: Vec<DeferredClipAction> = Vec::new();

    // ===== PHASE 1: Read lock scope - UI rendering and action collection =====
    {
        let proj_read = match project.read() {
            Ok(p) => p,
            Err(_) => return false,
        };

        // Flatten tracks for display using new DisplayRow system
        let display_rows = flatten_tracks_to_rows(
            &proj_read,
            track_ids,
            &editor_context.timeline.expanded_tracks,
        );

        for track_id in track_ids {
            if !editor_context.timeline.expanded_tracks.contains(track_id) {
                continue;
            }
            for (slot, y) in
                clip_insertion_markers(&display_rows, *track_id, &proj_read, geometry.row_layout())
            {
                let visual_slot = proj_read
                    .get_track(*track_id)
                    .and_then(|track| reverse_slot(slot, track.clip_ids.len()));
                let rect = egui::Rect::from_min_max(
                    egui::pos2(geometry.content_rect.min.x, y - 4.0),
                    egui::pos2(geometry.content_rect.max.x, y + 4.0),
                );
                crate::qa::register_component_with_metadata(
                    format!("timeline.clip_insertion_slot.{track_id}:{slot}"),
                    "timeline_clip_insertion_slot",
                    rect,
                    true,
                    Some(serde_json::json!({
                        "track_id": track_id,
                        "slot": slot,
                        "canonical_slot": slot,
                        "visual_slot": visual_slot,
                        "canonical_order_semantics": "back_to_front",
                        "visual_order_semantics": "front_to_back",
                    })),
                );
            }
        }

        // Calculate Reorder State if dragging
        let mut reorder_state = None;
        if let (Some(dragged_id), Some(hovered_tid)) = (
            editor_context
                .selection
                .primary()
                .and_then(SelectionTarget::clip_id),
            editor_context.interaction.dragged_entity_hovered_track_id,
        ) {
            if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                if let Some((canonical_insertion_slot, _)) = calculate_insert_index(
                    mouse_pos.y,
                    &display_rows,
                    &proj_read,
                    hovered_tid,
                    geometry.row_layout(),
                ) {
                    let source_track_id = editor_context
                        .interaction
                        .dragged_entity_original_track_id
                        .unwrap_or(hovered_tid);
                    reorder_state = clip_reorder_preview(
                        &proj_read,
                        dragged_id,
                        source_track_id,
                        hovered_tid,
                        canonical_insertion_slot,
                    );
                }
            }
        }

        let reorder_projection = reorder_state
            .map(|preview| clip_reorder_projection(&display_rows, &proj_read, preview));

        {
            let mut draw_context = SingleClipDrawContext {
                ui_content,
                editor_context,
                deferred_actions: &mut deferred_actions,
                project_service,
                project: &proj_read,
                geometry,
                display_rows: &display_rows,
                reorder_projection: reorder_projection.as_ref(),
            };

            for row in &display_rows {
                match row {
                    DisplayRow::TrackHeader {
                        track,
                        visible_row_index,
                        is_expanded,
                        ..
                    } => {
                        // If collapsed, draw all clips on this row.
                        if !is_expanded {
                            let mut clips_to_draw: Vec<&Clip> = Vec::new();
                            collect_track_clips(&proj_read, track, &mut clips_to_draw);

                            for clip in clips_to_draw {
                                clicked_on_entity |= draw_single_clip(
                                    &mut draw_context,
                                    clip,
                                    track,
                                    *visible_row_index,
                                    false,
                                );
                            }
                        }
                    }
                    DisplayRow::ClipRow {
                        clip,
                        parent_track,
                        visible_row_index,
                        ..
                    } => {
                        clicked_on_entity |= draw_single_clip(
                            &mut draw_context,
                            clip,
                            parent_track,
                            *visible_row_index,
                            false,
                        );
                    }
                }
            }
        }

        // Draw asset drag preview indicator
        if let Some(ref _dragged_item) = editor_context.interaction.dragged_item {
            if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                if geometry.content_rect.contains(mouse_pos) {
                    // Calculate insert position
                    let relative_y = mouse_pos.y - geometry.content_rect.min.y
                        + editor_context.timeline.scroll_offset.y;
                    let row_with_spacing = geometry.row_height + geometry.row_spacing;
                    let row_index = (relative_y / row_with_spacing).floor() as usize;

                    // Determine if we're in the top or bottom half of a row
                    let y_in_row = relative_y % row_with_spacing;
                    let insert_at_top = y_in_row < geometry.row_height / 2.0;

                    // Calculate the Y position for the indicator line
                    let indicator_row = if insert_at_top {
                        row_index
                    } else {
                        row_index + 1
                    };
                    let indicator_y = geometry.content_rect.min.y
                        + (indicator_row as f32 * row_with_spacing)
                        - editor_context.timeline.scroll_offset.y;

                    // Draw a horizontal line indicator
                    let painter = ui_content.painter();
                    let line_start = egui::pos2(geometry.content_rect.min.x, indicator_y);
                    let line_end = egui::pos2(geometry.content_rect.max.x, indicator_y);
                    painter.line_segment(
                        [line_start, line_end],
                        egui::Stroke::new(3.0, egui::Color32::from_rgb(100, 200, 255)),
                    );

                    // Draw small triangles at the edges
                    let triangle_size = 8.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(line_start.x, indicator_y - triangle_size),
                            egui::pos2(line_start.x + triangle_size, indicator_y),
                            egui::pos2(line_start.x, indicator_y + triangle_size),
                        ],
                        egui::Color32::from_rgb(100, 200, 255),
                        egui::Stroke::NONE,
                    ));
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(line_end.x, indicator_y - triangle_size),
                            egui::pos2(line_end.x - triangle_size, indicator_y),
                            egui::pos2(line_end.x, indicator_y + triangle_size),
                        ],
                        egui::Color32::from_rgb(100, 200, 255),
                        egui::Stroke::NONE,
                    ));
                }
            }
        }
    } // proj_read dropped here

    // ===== PHASE 2: Execute deferred actions (no lock held) =====
    let mut commit = ClipMutationCommit::default();
    let mut removed_clip_ids: Vec<Uuid> = Vec::new();
    for action in deferred_actions {
        match action {
            DeferredClipAction::UpdateClipTiming {
                clip_id,
                new_start_time,
                new_duration,
                new_trim_in,
            } => apply_timing_update_result(
                clip_id,
                project_service.update_clip_timing(
                    clip_id,
                    new_start_time,
                    new_duration,
                    new_trim_in,
                ),
                editor_context,
                &mut commit,
            ),
            DeferredClipAction::MoveClip {
                composition_id,
                source_track_id,
                clip_id,
                target_track_id,
                new_start_time,
                target_index,
            } => apply_move_clip_result(
                clip_id,
                project_service.move_clip_to_track_at_index(
                    composition_id,
                    source_track_id,
                    clip_id,
                    target_track_id,
                    new_start_time,
                    target_index,
                ),
                &mut commit,
            ),

            DeferredClipAction::RemoveClip { track_id, clip_id } => {
                if let Err(e) = project_service.remove_clip_from_track(track_id, clip_id) {
                    log::error!("Failed to remove clip: {:?}", e);
                } else {
                    removed_clip_ids.push(clip_id);
                    commit.persistent_change = true;
                }
            }
            DeferredClipAction::PushHistory => {
                commit.timing_history_requested = true;
            }
        }
    }

    if !removed_clip_ids.is_empty() {
        if let Ok(project) = project.read() {
            editor_context.reconcile_selection(&project);
        }
    }

    if commit.timing_update_failed {
        ui_content.ctx().request_repaint();
    }
    push_clip_history_if_needed(&commit, project, history_manager);

    clicked_on_entity
}

pub(super) struct BoxSelectionContext<'a> {
    pub(super) project: &'a Project,
    pub(super) track_ids: &'a [Uuid],
    pub(super) expanded_tracks: &'a HashSet<Uuid>,
    pub(super) geometry: ClipAreaGeometry,
}

fn clip_intersects_selection(
    clip: &Clip,
    row_index: usize,
    selection_rect: egui::Rect,
    geometry: ClipAreaGeometry,
) -> bool {
    selection_rect.intersects(geometry.clip_rect(
        clip.start_time.into_inner(),
        clip.duration.into_inner(),
        row_index,
    ))
}

pub(super) fn get_clips_in_box(
    selection_rect: egui::Rect,
    context: BoxSelectionContext<'_>,
) -> Vec<(Uuid, Uuid)> {
    let BoxSelectionContext {
        project,
        track_ids,
        expanded_tracks,
        geometry,
    } = context;
    let mut found = Vec::new();
    let display_rows = flatten_tracks_to_rows(project, track_ids, expanded_tracks);

    for row in display_rows {
        match row {
            DisplayRow::TrackHeader {
                track,
                visible_row_index,
                is_expanded: false,
                ..
            } => {
                for clip_id in &track.clip_ids {
                    if let Some(clip) = project.get_clip(*clip_id).filter(|clip| {
                        clip_intersects_selection(clip, visible_row_index, selection_rect, geometry)
                    }) {
                        found.push((clip.id, track.id));
                    }
                }
            }
            DisplayRow::ClipRow {
                clip,
                parent_track,
                visible_row_index,
                ..
            } => {
                if clip_intersects_selection(clip, visible_row_index, selection_rect, geometry) {
                    found.push((clip.id, parent_track.id));
                }
            }
            DisplayRow::TrackHeader { .. } => {}
        }
    }
    found
}

#[cfg(test)]
mod tests;
