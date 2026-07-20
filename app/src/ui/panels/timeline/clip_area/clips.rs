use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
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
};

use super::super::utils::flatten::{flatten_tracks_to_rows, DisplayRow};

const EDGE_DRAG_WIDTH: f32 = 5.0;

#[derive(Clone, Copy)]
pub(crate) struct ClipRowLayout {
    pub(crate) content_min_y: f32,
    pub(crate) scroll_y: f32,
    pub(crate) row_height: f32,
    pub(crate) row_spacing: f32,
}

impl ClipRowLayout {
    fn row_step(self) -> f32 {
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
        NodeContent::PluginOperation(_) | NodeContent::Merge => "Result",
        NodeContent::Value(_) => "Value",
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
        Some(NodeContent::CompositionInstance(_) | NodeContent::Merge) | None => (150, 150, 150),
    }
}

pub(crate) fn calculate_insert_index(
    mouse_y: f32,
    display_rows: &[DisplayRow],
    project: &Project,
    hovered_track_id: Uuid,
    layout: ClipRowLayout,
) -> Option<(usize, usize)> {
    // Returns (target_index, header_row_index)

    // Find header row for hovered track
    if let Some((header_idx, _)) = display_rows.iter().enumerate().find(|(_, r)| {
        r.track_id() == hovered_track_id && matches!(r, DisplayRow::TrackHeader { .. })
    }) {
        let current_y_in_clip_area = mouse_y - layout.content_min_y + layout.scroll_y;

        let hovered_row_index = (current_y_in_clip_area / layout.row_step()).floor() as isize;
        let header_row_index = header_idx as isize;

        let raw_target_index = hovered_row_index - header_row_index - 1;

        // Clamp to valid range
        if let Some(track) = project.get_track(hovered_track_id) {
            // Count clips in this track
            let clip_count = track.clip_ids.len();

            // Invert index because display order is reversed (Top of UI = End of List)
            let max_index = clip_count as isize;

            let inverted_target = max_index - raw_target_index;
            let target_index = inverted_target.clamp(0, max_index) as usize;

            return Some((target_index, header_idx));
        }
    }
    None
}

/// Logical insertion slots for an expanded Track.  Slot 0 is before the
/// first canonical clip and `clip_count` is after the last.  The Timeline is
/// visually reversed (later clips are higher), so slot numbers descend as Y
/// increases.
fn clip_insertion_markers(
    display_rows: &[DisplayRow],
    track_id: Uuid,
    project: &Project,
    layout: ClipRowLayout,
) -> Vec<(usize, f32)> {
    let Some(header_row) = display_rows.iter().position(|row| {
        row.track_id() == track_id && matches!(row, DisplayRow::TrackHeader { .. })
    }) else {
        return Vec::new();
    };
    let Some(track) = project.get_track(track_id) else {
        return Vec::new();
    };
    let clip_count = track.clip_ids.len();
    (0..=clip_count)
        .map(|slot| {
            let boundary_row = header_row + 1 + (clip_count - slot);
            (
                slot,
                layout.content_min_y + boundary_row as f32 * layout.row_step() - layout.scroll_y,
            )
        })
        .collect()
}

fn nearest_clip_insertion_slot(pointer_y: f32, markers: &[(usize, f32)]) -> Option<usize> {
    markers
        .iter()
        .min_by(|(_, lhs_y), (_, rhs_y)| {
            (pointer_y - *lhs_y)
                .abs()
                .total_cmp(&(pointer_y - *rhs_y).abs())
        })
        .map(|(slot, _)| *slot)
}

/// Convert an insertion slot in the original list into the index expected
/// after the source Clip is detached.  The two slots directly adjacent to a
/// Clip are intentional no-ops, which is what keeps a horizontal timing drag
/// from silently changing layer order.
fn destination_index_for_clip_slot(
    same_track: bool,
    source_index: usize,
    insertion_slot: usize,
    target_clip_count: usize,
) -> Option<usize> {
    if !same_track {
        return Some(insertion_slot.min(target_clip_count));
    }
    if target_clip_count == 0 || source_index >= target_clip_count {
        return None;
    }
    let destination = if insertion_slot > source_index {
        insertion_slot - 1
    } else {
        insertion_slot
    }
    .min(target_clip_count - 1);
    (destination != source_index).then_some(destination)
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

#[derive(Clone, Copy)]
struct ClipReorderPreview {
    dragged_id: Uuid,
    target_track_id: Uuid,
    source_index: usize,
    target_index: usize,
    header_row_index: usize,
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
                if let Some((target_index, header_idx)) = calculate_insert_index(
                    mouse_pos.y,
                    &display_rows,
                    &proj_read,
                    hovered_tid,
                    geometry.row_layout(),
                ) {
                    // Find dragged clip original info
                    let source_track_id = editor_context
                        .interaction
                        .dragged_entity_original_track_id
                        .unwrap_or(hovered_tid);
                    if let Some(dragged_original_index) =
                        proj_read.get_track(source_track_id).and_then(|track| {
                            track
                                .clip_ids
                                .iter()
                                .position(|clip_id| *clip_id == dragged_id)
                        })
                    {
                        reorder_state = Some(ClipReorderPreview {
                            dragged_id,
                            target_track_id: hovered_tid,
                            source_index: dragged_original_index,
                            target_index,
                            header_row_index: header_idx,
                        });
                    }
                }
            }
        }

        {
            let mut draw_context = SingleClipDrawContext {
                ui_content,
                editor_context,
                deferred_actions: &mut deferred_actions,
                project_service,
                project: &proj_read,
                geometry,
                display_rows: &display_rows,
                reorder_state,
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

struct SingleClipDrawContext<'a> {
    ui_content: &'a mut Ui,
    editor_context: &'a mut EditorContext,
    deferred_actions: &'a mut Vec<DeferredClipAction>,
    project_service: &'a ProjectService,
    project: &'a Project,
    geometry: ClipAreaGeometry,
    display_rows: &'a [DisplayRow<'a>],
    reorder_state: Option<ClipReorderPreview>,
}

fn draw_single_clip(
    context: &mut SingleClipDrawContext<'_>,
    clip: &Clip,
    track: &Track,
    row_index: usize,
    is_summary_clip: bool,
) -> bool {
    let SingleClipDrawContext {
        ui_content,
        editor_context,
        deferred_actions,
        project_service,
        project,
        geometry,
        display_rows,
        reorder_state,
    } = context;
    let graph_nodes = clip_graph_nodes(clip, project);
    // Result and semantic source are separate: explicit Style/Effect/Merge
    // results retain the color, label, and audio identity of their reachable
    // direct source.
    let (r, g, b) = get_clip_color(graph_nodes.semantic_source, project);
    let clip_color = egui::Color32::from_rgb(r, g, b);

    // Apply Live Reordering Visual Shift
    let mut visual_row_index = row_index;

    // Check if we are in a reordering state
    if let Some(reorder) = reorder_state {
        if clip.id == reorder.dragged_id {
            visual_row_index = reorder.header_row_index + 1 + reorder.target_index;
        } else if track.id == reorder.target_track_id {
            // Get original child index from DisplayRow if available
            let mut original_child_index = None;
            if let Some(DisplayRow::ClipRow { child_index, .. }) = display_rows.get(row_index) {
                original_child_index = Some(*child_index);
            }

            if let Some(idx) = original_child_index {
                let mut new_child_index = idx;
                let src = reorder.source_index;
                let dst = reorder.target_index;

                let is_same_track_sort = if let Some(orig_tid) =
                    editor_context.interaction.dragged_entity_original_track_id
                {
                    orig_tid == reorder.target_track_id
                } else {
                    false
                };

                if is_same_track_sort {
                    if src < dst {
                        if idx > src && idx <= dst {
                            new_child_index = idx - 1;
                        }
                    } else if src > dst && idx >= dst && idx < src {
                        new_child_index = idx + 1;
                    }
                } else if idx >= dst {
                    new_child_index = idx + 1;
                }

                if new_child_index != idx {
                    visual_row_index = reorder.header_row_index + 1 + new_child_index;
                }
            }
        }
    }

    let initial_clip_rect = geometry.clip_rect(*clip.start_time, *clip.duration, visual_row_index);
    let safe_width = initial_clip_rect.width();

    // Visibility Culling
    if !geometry.content_rect.intersects(initial_clip_rect) {
        return false;
    }

    if !is_summary_clip {
        let canonical_index = track
            .clip_ids
            .iter()
            .position(|candidate| *candidate == clip.id);
        crate::qa::register_component_with_metadata(
            format!("timeline.clip:{}", clip.id),
            "timeline_clip",
            initial_clip_rect,
            true,
            Some(serde_json::json!({
                "clip_id": clip.id,
                "track_id": track.id,
                "canonical_index": canonical_index,
                "start_time": clip.start_time.into_inner(),
                "duration": clip.duration.into_inner(),
                "pixels_per_second": geometry.pixels_per_unit,
                "output_node_id": graph_nodes.output.map(|node| node.id),
                "semantic_source_node_id": graph_nodes.semantic_source.map(|node| node.id),
                "semantic_source_kind": graph_nodes.semantic_source.map(semantic_source_kind),
            })),
        );
    }

    // --- Interaction for clips ---
    let sense = if is_summary_clip {
        egui::Sense::click()
    } else {
        egui::Sense::click_and_drag()
    };

    let interaction_id = if is_summary_clip {
        egui::Id::new(clip.id).with("summary").with(row_index)
    } else {
        egui::Id::new(clip.id)
    };

    let clip_resp = ui_content.interact(initial_clip_rect, interaction_id, sense);

    if !is_summary_clip {
        clip_resp.context_menu(|ui| {
            let response = ui.button(format!("{} Remove", icons::TRASH));
            crate::qa::register_component(
                format!("timeline.menu.delete.clip:{}", clip.id),
                "timeline_menu_item",
                response.rect,
            );
            if response.clicked() && editor_context.active_composition_id.is_some() {
                deferred_actions.push(DeferredClipAction::RemoveClip {
                    track_id: track.id,
                    clip_id: clip.id,
                });
                ui.ctx().request_repaint();
                ui.close();
            }
        });
    }

    // Edges (Resize)
    let mut left_edge_resp = None;
    let mut right_edge_resp = None;

    if !is_summary_clip {
        let left_edge_rect = egui::Rect::from_min_size(
            egui::pos2(initial_clip_rect.min.x, initial_clip_rect.min.y),
            egui::vec2(EDGE_DRAG_WIDTH, initial_clip_rect.height()),
        );
        left_edge_resp = Some(ui_content.interact(
            left_edge_rect,
            egui::Id::new(clip.id).with("left_edge"),
            egui::Sense::drag(),
        ));
        crate::qa::register_component_with_metadata(
            format!("timeline.clip_edge.left:{}", clip.id),
            "timeline_clip_edge",
            left_edge_rect,
            true,
            Some(serde_json::json!({"side": "left", "clip_id": clip.id})),
        );

        let right_edge_rect = egui::Rect::from_min_size(
            egui::pos2(
                initial_clip_rect.max.x - EDGE_DRAG_WIDTH,
                initial_clip_rect.min.y,
            ),
            egui::vec2(EDGE_DRAG_WIDTH, initial_clip_rect.height()),
        );
        right_edge_resp = Some(ui_content.interact(
            right_edge_rect,
            egui::Id::new(clip.id).with("right_edge"),
            egui::Sense::drag(),
        ));
        crate::qa::register_component_with_metadata(
            format!("timeline.clip_edge.right:{}", clip.id),
            "timeline_clip_edge",
            right_edge_rect,
            true,
            Some(serde_json::json!({"side": "right", "clip_id": clip.id})),
        );
    }

    // Handle edge dragging (resize)
    if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
        if left.drag_started() || right.drag_started() {
            begin_resize_gesture(editor_context);
            editor_context.select_target(SelectionTarget::Clip(clip.id));
        }
    }

    if editor_context.interaction.is_resizing_entity
        && editor_context.selection.primary() == Some(SelectionTarget::Clip(clip.id))
        && !is_summary_clip
    {
        if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
            let mut new_start_time = clip.start_time.into_inner();
            let mut new_duration = clip.duration.into_inner();
            let mut new_trim_in = clip.trim_in.into_inner();

            let delta_x = if left.dragged() {
                timeline_drag_delta(left).x
            } else if right.dragged() {
                timeline_drag_delta(right).x
            } else {
                0.0
            };

            // Convert to time
            let delta_time = delta_x / geometry.pixels_per_unit;

            if left.dragged() {
                if let Some(timing) = timing_after_left_edge_drag(clip, delta_time as f64) {
                    new_start_time = timing.start_time;
                    new_duration = timing.duration;
                    new_trim_in = timing.trim_in;
                }
            } else if right.dragged() {
                // Moving End: Adjust duration only.
                let proposed_duration = new_duration + delta_time as f64;
                if proposed_duration > 0.0 {
                    new_duration = proposed_duration;
                }
            }

            if (new_start_time != clip.start_time.into_inner()
                || new_duration != clip.duration.into_inner()
                || new_trim_in != clip.trim_in.into_inner())
                && editor_context.active_composition_id.is_some()
            {
                deferred_actions.push(DeferredClipAction::UpdateClipTiming {
                    clip_id: clip.id,
                    new_start_time,
                    new_duration,
                    new_trim_in,
                });
                mark_resize_timing_changed(editor_context);
            }
        }
    }

    if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
        let should_commit_resize =
            (left.drag_stopped() || right.drag_stopped()) && finish_resize_gesture(editor_context);
        if should_commit_resize {
            deferred_actions.push(DeferredClipAction::PushHistory);
        }
    }

    let edge_is_dragging = left_edge_resp
        .as_ref()
        .is_some_and(|response| response.dragged())
        || right_edge_resp
            .as_ref()
            .is_some_and(|response| response.dragged());

    if clip_resp.drag_started() && !edge_is_dragging && !is_summary_clip {
        if !editor_context.is_selected(SelectionTarget::Clip(clip.id)) {
            editor_context.select_target(SelectionTarget::Clip(clip.id));
        }
        editor_context.interaction.is_moving_selected_entity = true;
        editor_context.interaction.dragged_entity_original_track_id = Some(track.id);
        editor_context.interaction.dragged_entity_hovered_track_id = Some(track.id);
        editor_context.interaction.dragged_entity_has_moved = false;
    }

    if editor_context.interaction.is_moving_selected_entity
        && editor_context.selection.primary() == Some(SelectionTarget::Clip(clip.id))
        && clip_resp.dragged()
        && !edge_is_dragging
    {
        if let Some(pointer) = clip_resp.interact_pointer_pos() {
            let row = ((pointer.y - geometry.content_rect.min.y
                + editor_context.timeline.scroll_offset.y)
                / (geometry.row_height + geometry.row_spacing))
                .floor()
                .max(0.0) as usize;
            if let Some(target_row) = display_rows.get(row) {
                editor_context.interaction.dragged_entity_hovered_track_id =
                    Some(target_row.track_id());
            }
        }
        let delta_time =
            f64::from(timeline_drag_delta(&clip_resp).x) / f64::from(geometry.pixels_per_unit);
        if let Some(timing) = timing_after_body_drag(clip, delta_time) {
            deferred_actions.push(DeferredClipAction::UpdateClipTiming {
                clip_id: clip.id,
                new_start_time: timing.start_time,
                new_duration: timing.duration,
                new_trim_in: timing.trim_in,
            });
            editor_context.interaction.dragged_entity_has_moved = true;
        }
    }

    if clip_resp.drag_stopped()
        && editor_context.interaction.is_moving_selected_entity
        && editor_context.selection.primary() == Some(SelectionTarget::Clip(clip.id))
        && !edge_is_dragging
        && !is_summary_clip
    {
        let source_track_id = editor_context
            .interaction
            .dragged_entity_original_track_id
            .unwrap_or(track.id);
        let target_track_id = editor_context
            .interaction
            .dragged_entity_hovered_track_id
            .unwrap_or(source_track_id);
        // Horizontal timing changes were applied incrementally to the same
        // authoritative Project while dragging. `drag_delta()` is per-frame
        // and is zero on egui's release frame, so commit the current value.
        let new_start_time = clip.start_time.into_inner();
        let target_index = clip_resp.interact_pointer_pos().and_then(|pointer| {
            let markers = clip_insertion_markers(
                display_rows,
                target_track_id,
                project,
                geometry.row_layout(),
            );
            let insertion_slot = nearest_clip_insertion_slot(pointer.y, &markers)?;
            let source_index = project
                .get_track(source_track_id)?
                .clip_ids
                .iter()
                .position(|candidate| *candidate == clip.id)?;
            let target_count = project.get_track(target_track_id)?.clip_ids.len();
            destination_index_for_clip_slot(
                source_track_id == target_track_id,
                source_index,
                insertion_slot,
                target_count,
            )
        });

        if let Some(composition_id) = editor_context.active_composition_id {
            deferred_actions.push(DeferredClipAction::MoveClip {
                composition_id,
                source_track_id,
                clip_id: clip.id,
                target_track_id,
                new_start_time,
                target_index,
            });
        }
        editor_context.interaction.is_moving_selected_entity = false;
        editor_context.interaction.dragged_entity_original_track_id = None;
        editor_context.interaction.dragged_entity_hovered_track_id = None;
        editor_context.interaction.dragged_entity_has_moved = false;
    }

    // Calculate display position
    let mut display_x = initial_clip_rect.min.x;
    let display_y = initial_clip_rect.min.y;

    if editor_context.is_selected(SelectionTarget::Clip(clip.id))
        && clip_resp.dragged()
        && !is_summary_clip
    {
        display_x += clip_resp.drag_delta().x;
    }

    let drawing_clip_rect = egui::Rect::from_min_size(
        egui::pos2(display_x, display_y),
        egui::vec2(safe_width, geometry.row_height),
    );

    // --- Drawing ---
    let is_sel_entity = editor_context.is_selected(SelectionTarget::Clip(clip.id));
    let mut transparent_color =
        egui::Color32::from_rgba_premultiplied(clip_color.r(), clip_color.g(), clip_color.b(), 150);

    if is_summary_clip {
        transparent_color = egui::Color32::from_rgba_premultiplied(
            clip_color.r(),
            clip_color.g(),
            clip_color.b(),
            100,
        );
    }

    let painter = ui_content.painter_at(geometry.content_rect);
    painter.rect_filled(drawing_clip_rect, 4.0, transparent_color);

    super::waveform::draw_clip_waveform(super::waveform::WaveformDrawContext {
        ctx: ui_content.ctx(),
        painter: &painter,
        clip_rect: drawing_clip_rect,
        viewport_rect: geometry.content_rect,
        pixels_per_second: geometry.pixels_per_unit,
        clip,
        project,
        project_service,
    });

    if is_sel_entity {
        painter.rect_stroke(
            drawing_clip_rect,
            4.0,
            egui::Stroke::new(2.0, egui::Color32::WHITE),
            StrokeKind::Middle,
        );
    }

    let mut clip_text = graph_nodes
        .semantic_source
        .map(semantic_source_label)
        .unwrap_or_else(|| clip.name.clone());

    if is_summary_clip {
        clip_text = format!("(Ref) {}", clip_text);
    }

    painter.text(
        drawing_clip_rect.min + egui::vec2(5.0, 5.0),
        egui::Align2::LEFT_TOP,
        &clip_text,
        egui::FontId::default(),
        egui::Color32::BLACK,
    );

    let edge_hovered = left_edge_resp
        .as_ref()
        .zip(right_edge_resp.as_ref())
        .is_some_and(|(left, right)| left.hovered() || right.hovered());
    if !is_summary_clip && edge_hovered {
        ui_content
            .ctx()
            .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    if !editor_context.interaction.is_resizing_entity && clip_resp.clicked() {
        let action = crate::ui::selection::get_click_action(
            &ui_content.input(|i| i.modifiers),
            Some(clip.id),
        );

        match action {
            crate::ui::selection::ClickAction::Select(id) => {
                editor_context.select_target(SelectionTarget::Clip(id));
            }
            crate::ui::selection::ClickAction::Add(id)
                if !editor_context.is_selected(SelectionTarget::Clip(id)) =>
            {
                editor_context.toggle_selection(SelectionTarget::Clip(id));
            }
            crate::ui::selection::ClickAction::Remove(id)
                if editor_context.is_selected(SelectionTarget::Clip(id)) =>
            {
                editor_context.toggle_selection(SelectionTarget::Clip(id));
            }
            crate::ui::selection::ClickAction::Toggle(id) => {
                editor_context.toggle_selection(SelectionTarget::Clip(id));
            }
            _ => {}
        }
        return true;
    }

    false
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
mod tests {
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
        project.add_track(track);
        (project, track_id, clip_ids)
    }

    fn selection_geometry() -> ClipAreaGeometry {
        ClipAreaGeometry {
            content_rect: egui::Rect::from_min_size(
                egui::pos2(100.0, 200.0),
                egui::vec2(400.0, 300.0),
            ),
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
}
