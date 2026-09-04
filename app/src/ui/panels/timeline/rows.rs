use std::collections::HashSet;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Sense, Vec2};
use egui_phosphor::regular as icons;
use library::editor::{AuthoringWaveformService, TimelineEditorService};
use library::model::authoring::{
    ordered_track_item_ids, AuthoringProject, TimelineId, TimelineItemId, TimelineTrackId,
};

use crate::state::authoring::{
    AuthoringSelection, AuthoringTimelineView, AuthoringUiState, AutomationTarget,
    TimelineClipDisplayMode, TimelineGestureKind,
};
use crate::ui::automation_lanes;
use crate::ui::media_preview::AuthoringMediaPreviewService;

use super::dope_sheet;
use super::geometry::TimelineRowMetrics;
use super::interaction::TimelineRowProjection;
use super::painting::draw_playhead;
use super::viewport::row_top;
use super::{draw_item, DeferredItemAction};

#[derive(Clone)]
pub(super) enum RowKind {
    Track {
        track_id: TimelineTrackId,
        expanded: bool,
    },
    Clip {
        track_id: TimelineTrackId,
        item_id: TimelineItemId,
    },
    Property {
        item_id: TimelineItemId,
        target: AutomationTarget,
    },
}

#[derive(Clone)]
pub(super) struct DisplayRow {
    pub(super) kind: RowKind,
}

pub(super) fn property_row_items(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    view: &AuthoringTimelineView,
) -> HashSet<TimelineItemId> {
    project
        .items
        .values()
        .filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == timeline_id)
                && view.shows_property_rows(item.id, item.track_id)
        })
        .map(|item| item.id)
        .collect()
}

pub(super) fn display_rows(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    expanded: &HashSet<TimelineTrackId>,
    expanded_items: &HashSet<TimelineItemId>,
) -> Vec<DisplayRow> {
    let Some(timeline) = project.timelines.get(&timeline_id) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for track_id in timeline.track_order.iter().rev() {
        if !project.tracks.contains_key(track_id) {
            continue;
        }
        let is_expanded = expanded.contains(track_id);
        rows.push(DisplayRow {
            kind: RowKind::Track {
                track_id: *track_id,
                expanded: is_expanded,
            },
        });
        if is_expanded {
            for item_id in ordered_track_item_ids(project, *track_id, None)
                .into_iter()
                .rev()
            {
                rows.push(DisplayRow {
                    kind: RowKind::Clip {
                        track_id: *track_id,
                        item_id,
                    },
                });
                if expanded_items.contains(&item_id) {
                    rows.extend(
                        automation_lanes::collect_item_keyframed_lanes(project, item_id)
                            .into_iter()
                            .map(|lane| DisplayRow {
                                kind: RowKind::Property {
                                    item_id,
                                    target: lane.target,
                                },
                            }),
                    );
                }
            }
        }
    }
    rows
}

#[allow(
    clippy::too_many_arguments,
    reason = "row orchestration needs shared Timeline geometry and clip content services"
)]
pub(super) fn draw_rows(
    ui: &mut egui::Ui,
    project: &Arc<AuthoringProject>,
    state: &mut AuthoringUiState,
    rows: &[DisplayRow],
    row_projection: Option<&TimelineRowProjection>,
    sidebar_rect: Rect,
    content_rect: Rect,
    actions: &mut Vec<DeferredItemAction>,
    service: &TimelineEditorService,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) {
    let row_metrics = TimelineRowMetrics::from_view(&state.timeline);
    for (row_index, row) in rows.iter().enumerate() {
        let RowKind::Track { track_id, expanded } = row.kind else {
            continue;
        };
        let display_row_index = row_projection
            .and_then(|projection| projection.row_for_track(track_id))
            .unwrap_or(row_index);
        let y = row_top(content_rect, &state.timeline, display_row_index);
        let left_rect = Rect::from_min_size(
            Pos2::new(sidebar_rect.min.x, y),
            Vec2::new(sidebar_rect.width(), row_metrics.row_height()),
        );
        let clip_row_rect = Rect::from_min_size(
            Pos2::new(content_rect.min.x, y),
            Vec2::new(content_rect.width(), row_metrics.row_height()),
        );
        let header_visible = left_rect.intersects(sidebar_rect);
        if header_visible {
            draw_track_header(
                ui,
                project,
                state,
                track_id,
                expanded,
                &project.tracks[&track_id].name,
                display_row_index,
                left_rect,
                sidebar_rect,
            );
        }

        let projected_item_ids =
            row_projection.and_then(|projection| projection.items_for_track(track_id));
        let canonical_item_ids;
        let item_ids = if let Some(item_ids) = projected_item_ids {
            item_ids
        } else {
            canonical_item_ids = ordered_track_item_ids(project, track_id, None);
            &canonical_item_ids
        };
        if expanded {
            draw_expanded_items(
                ui,
                project,
                state,
                service,
                waveform,
                media_previews,
                rows,
                row_projection,
                sidebar_rect,
                content_rect,
                display_row_index,
                item_ids,
                actions,
            );
        } else if header_visible {
            for item_id in item_ids {
                let Some(item) = project.items.get(item_id) else {
                    continue;
                };
                draw_item(
                    ui,
                    project,
                    state,
                    item,
                    clip_row_rect,
                    content_rect,
                    display_row_index,
                    true,
                    actions,
                    waveform,
                    media_previews,
                );
            }
        }
    }
    draw_playhead(ui, project, state, content_rect);
}

#[allow(
    clippy::too_many_arguments,
    reason = "expanded rows use one geometry and service set from their parent pass"
)]
fn draw_expanded_items(
    ui: &mut egui::Ui,
    project: &Arc<AuthoringProject>,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
    rows: &[DisplayRow],
    row_projection: Option<&TimelineRowProjection>,
    sidebar_rect: Rect,
    content_rect: Rect,
    track_row: usize,
    item_ids: &[TimelineItemId],
    actions: &mut Vec<DeferredItemAction>,
) {
    let metrics = TimelineRowMetrics::from_view(&state.timeline);
    for item_id in item_ids.iter().rev() {
        let item_row = row_projection
            .and_then(|projection| projection.row_for_item(*item_id))
            .or_else(|| canonical_item_row(rows, *item_id))
            .unwrap_or(track_row + 1);
        let item_y = row_top(content_rect, &state.timeline, item_row);
        let label_rect = Rect::from_min_size(
            Pos2::new(sidebar_rect.min.x, item_y),
            Vec2::new(sidebar_rect.width(), metrics.row_height()),
        );
        let content_row = Rect::from_min_size(
            Pos2::new(content_rect.min.x, item_y),
            Vec2::new(content_rect.width(), metrics.row_height()),
        );
        let Some(item) = project.items.get(item_id) else {
            continue;
        };
        let lanes = automation_lanes::collect_item_keyframed_lanes(project, *item_id);
        let display_mode = state.timeline.item_display_mode(item.id, item.track_id);
        let properties_visible = state.timeline.shows_property_rows(item.id, item.track_id);
        if label_rect.intersects(sidebar_rect) {
            draw_clip_label(
                ui,
                state,
                item,
                &item.name,
                !lanes.is_empty(),
                properties_visible,
                display_mode,
                item_row,
                label_rect,
                sidebar_rect,
            );
            draw_item(
                ui,
                project,
                state,
                item,
                content_row,
                content_rect,
                item_row,
                false,
                actions,
                waveform,
                media_previews,
            );
        }
        if properties_visible {
            draw_item_properties(
                ui,
                project,
                state,
                service,
                rows,
                row_projection,
                sidebar_rect,
                content_rect,
                item,
                &lanes,
            );
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "property rows consume the same projected Timeline geometry as their Clip"
)]
fn draw_item_properties(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    rows: &[DisplayRow],
    row_projection: Option<&TimelineRowProjection>,
    sidebar_rect: Rect,
    content_rect: Rect,
    item: &library::model::authoring::TimelineItem,
    lanes: &[automation_lanes::AutomationLane],
) {
    let metrics = TimelineRowMetrics::from_view(&state.timeline);
    for lane in lanes {
        let property_row = row_projection
            .and_then(|projection| projection.row_for_property(item.id, &lane.target))
            .or_else(|| canonical_property_row(rows, item.id, &lane.target));
        let Some(property_row) = property_row else {
            continue;
        };
        let y = row_top(content_rect, &state.timeline, property_row);
        let label_rect = Rect::from_min_size(
            Pos2::new(sidebar_rect.min.x, y),
            Vec2::new(sidebar_rect.width(), metrics.row_height()),
        );
        let row_rect = Rect::from_min_size(
            Pos2::new(content_rect.min.x, y),
            Vec2::new(content_rect.width(), metrics.row_height()),
        );
        if label_rect.intersects(sidebar_rect) {
            dope_sheet::draw_property_row(
                ui,
                project,
                state,
                service,
                item,
                lane,
                property_row,
                label_rect,
                row_rect,
                sidebar_rect,
                content_rect,
            );
        }
    }
}

fn draw_track_header(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    track_id: TimelineTrackId,
    expanded: bool,
    label: &str,
    display_row_index: usize,
    rect: Rect,
    sidebar_rect: Rect,
) {
    let display_mode = state.timeline.track_display_mode(track_id);
    let visible_rect = rect.intersect(sidebar_rect);
    let response = ui.interact(
        visible_rect,
        ui.id().with(("track", track_id)),
        Sense::click(),
    );
    crate::qa::register_component_with_metadata(
        format!("timeline.track:{track_id}"),
        "timeline_track",
        visible_rect,
        true,
        Some(serde_json::json!({
            "track_id": track_id,
            "expanded": expanded,
            "display_mode": display_mode.qa_name(),
            "display_row_index": display_row_index,
            "row_height": rect.height(),
        })),
    );
    if response.clicked() {
        state.selection.replace(AuthoringSelection::Track(track_id));
    }
    let selected = state
        .selection
        .contains(AuthoringSelection::Track(track_id));
    if selected || response.hovered() {
        ui.painter().with_clip_rect(sidebar_rect).rect_filled(
            rect,
            0.0,
            if selected {
                Color32::from_rgb(42, 67, 94)
            } else {
                Color32::from_gray(42)
            },
        );
    }
    let caret_rect =
        Rect::from_min_size(rect.min, Vec2::new(27.0, rect.height())).intersect(sidebar_rect);
    let caret = ui.interact(
        caret_rect,
        ui.id().with(("track-caret", track_id)),
        Sense::click(),
    );
    crate::qa::register_component_with_metadata(
        format!("timeline.track_expand:{track_id}"),
        "timeline_track_expand",
        caret_rect,
        true,
        Some(serde_json::json!({"expanded": expanded})),
    );
    if caret.clicked() {
        if expanded {
            state.timeline.expanded_tracks.remove(&track_id);
        } else {
            state.timeline.expanded_tracks.insert(track_id);
        }
    }
    let mode_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 29.0, rect.top()),
        Vec2::new(29.0, rect.height()),
    )
    .intersect(sidebar_rect);
    if display_mode_toggle(
        ui,
        ("track-display-mode", track_id),
        format!("timeline.track_display:{track_id}"),
        "track",
        track_id,
        display_mode,
        mode_rect,
        sidebar_rect,
    ) {
        set_track_display_mode(project, state, track_id, display_mode.toggled());
    }
    let painter = ui.painter().with_clip_rect(sidebar_rect);
    painter.text(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        if expanded {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        },
        egui::FontId::proportional(13.0),
        ui.visuals().text_color(),
    );
    painter.text(
        Pos2::new(rect.left() + 31.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        ui.visuals().text_color(),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "Clip labels own selection, expansion, and shared row metadata"
)]
fn draw_clip_label(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    item: &library::model::authoring::TimelineItem,
    label: &str,
    can_expand: bool,
    properties_visible: bool,
    display_mode: TimelineClipDisplayMode,
    display_row_index: usize,
    rect: Rect,
    sidebar_rect: Rect,
) {
    let item_id = item.id;
    let visible_rect = rect.intersect(sidebar_rect);
    let label_rect = Rect::from_min_max(
        Pos2::new(visible_rect.left() + 48.0, visible_rect.top()),
        Pos2::new(
            (visible_rect.right() - 29.0).max(visible_rect.left() + 48.0),
            visible_rect.bottom(),
        ),
    );
    let response = ui.interact(
        label_rect,
        ui.id().with(("clip-label", item_id)),
        Sense::click(),
    );
    let gesture = state.timeline.item_gesture.as_ref();
    crate::qa::register_component_with_metadata(
        format!("timeline.row:{item_id}"),
        "timeline_clip_row",
        visible_rect,
        true,
        Some(serde_json::json!({
            "item_id": item_id,
            "display_row_index": display_row_index,
            "row_height": rect.height(),
            "reorder_preview_active": gesture.is_some_and(|gesture| gesture.kind == TimelineGestureKind::Move),
            "dragged": gesture.is_some_and(|gesture| gesture.item_id == item_id),
            "display_mode": display_mode.qa_name(),
        })),
    );
    if response.clicked() {
        state.selection.replace(AuthoringSelection::Item(item_id));
    }
    if state.selection.contains(AuthoringSelection::Item(item_id)) {
        ui.painter().with_clip_rect(sidebar_rect).rect_filled(
            rect,
            0.0,
            Color32::from_rgb(40, 59, 78),
        );
    }
    if can_expand {
        draw_item_caret(
            ui,
            state,
            item_id,
            item.track_id,
            properties_visible,
            rect,
            sidebar_rect,
        );
    }
    let mode_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 29.0, rect.top()),
        Vec2::new(29.0, rect.height()),
    )
    .intersect(sidebar_rect);
    if display_mode_toggle(
        ui,
        ("item-display-mode", item_id),
        format!("timeline.item_display:{item_id}"),
        "item",
        item_id,
        display_mode,
        mode_rect,
        sidebar_rect,
    ) {
        set_item_display_mode(state, item_id, item.track_id, display_mode.toggled());
    }
    ui.painter().with_clip_rect(sidebar_rect).text(
        Pos2::new(rect.left() + 51.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
}

fn draw_item_caret(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    item_id: TimelineItemId,
    track_id: TimelineTrackId,
    properties_visible: bool,
    rect: Rect,
    sidebar_rect: Rect,
) {
    let caret_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 24.0, rect.top()),
        Vec2::new(24.0, rect.height()),
    )
    .intersect(sidebar_rect);
    let _caret = ui
        .interact(
            caret_rect,
            ui.id().with(("clip-caret", item_id)),
            Sense::click(),
        )
        .on_hover_text(if properties_visible {
            "Hide clip properties"
        } else {
            "Show clip properties"
        });
    crate::qa::register_component_with_metadata(
        format!("timeline.item_expand:{item_id}"),
        "timeline_item_expand",
        caret_rect,
        true,
        Some(serde_json::json!({"item_id": item_id, "expanded": properties_visible})),
    );
    let pressed = ui.input(|input| {
        input.pointer.primary_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|position| caret_rect.contains(position))
    });
    if pressed {
        if properties_visible {
            state.timeline.expanded_items.remove(&item_id);
            if state.timeline.item_display_mode(item_id, track_id)
                == TimelineClipDisplayMode::Keyframes
            {
                set_item_display_mode(state, item_id, track_id, TimelineClipDisplayMode::Content);
            }
        } else {
            state.timeline.expanded_items.insert(item_id);
        }
        state.selection.replace(AuthoringSelection::Item(item_id));
        ui.ctx().request_repaint();
    }
    ui.painter().with_clip_rect(sidebar_rect).text(
        Pos2::new(rect.left() + 35.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        if properties_visible {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        },
        egui::FontId::proportional(11.0),
        ui.visuals().text_color(),
    );
}

#[allow(clippy::too_many_arguments)]
fn display_mode_toggle(
    ui: &mut egui::Ui,
    id_source: impl std::hash::Hash,
    component_id: String,
    owner_kind: &str,
    owner_id: impl std::fmt::Display,
    mode: TimelineClipDisplayMode,
    rect: Rect,
    clip_rect: Rect,
) -> bool {
    let response = ui
        .interact(rect, ui.id().with(id_source), Sense::click())
        .on_hover_text(match mode {
            TimelineClipDisplayMode::Content => "Show keyframes",
            TimelineClipDisplayMode::Keyframes => "Show clip content",
        });
    crate::qa::register_component_with_metadata(
        component_id,
        "timeline_display_mode_toggle",
        rect,
        true,
        Some(serde_json::json!({
            "owner_kind": owner_kind,
            "owner_id": owner_id.to_string(),
            "mode": mode.qa_name(),
        })),
    );
    ui.painter().with_clip_rect(clip_rect).text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        match mode {
            TimelineClipDisplayMode::Content => icons::IMAGE,
            TimelineClipDisplayMode::Keyframes => icons::DIAMONDS_FOUR,
        },
        egui::FontId::proportional(12.0),
        if response.hovered() {
            Color32::WHITE
        } else {
            ui.visuals().weak_text_color()
        },
    );
    response.clicked()
}

fn set_track_display_mode(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    track_id: TimelineTrackId,
    mode: TimelineClipDisplayMode,
) {
    if mode == TimelineClipDisplayMode::Content {
        state.timeline.track_display_modes.remove(&track_id);
    } else {
        state.timeline.track_display_modes.insert(track_id, mode);
    }
    for item in project
        .items
        .values()
        .filter(|item| item.track_id == track_id)
    {
        state.timeline.item_display_modes.remove(&item.id);
        state.timeline.expanded_items.remove(&item.id);
    }
}

fn set_item_display_mode(
    state: &mut AuthoringUiState,
    item_id: TimelineItemId,
    track_id: TimelineTrackId,
    mode: TimelineClipDisplayMode,
) {
    if mode == state.timeline.track_display_mode(track_id) {
        state.timeline.item_display_modes.remove(&item_id);
    } else {
        state.timeline.item_display_modes.insert(item_id, mode);
    }
    state.timeline.expanded_items.remove(&item_id);
}

fn canonical_item_row(rows: &[DisplayRow], item_id: TimelineItemId) -> Option<usize> {
    rows.iter()
        .position(|row| matches!(row.kind, RowKind::Clip { item_id: id, .. } if id == item_id))
}

fn canonical_property_row(
    rows: &[DisplayRow],
    item_id: TimelineItemId,
    target: &AutomationTarget,
) -> Option<usize> {
    rows.iter().position(|row| {
        matches!(
            &row.kind,
            RowKind::Property {
                item_id: id,
                target: candidate,
            } if *id == item_id && candidate == target
        )
    })
}
