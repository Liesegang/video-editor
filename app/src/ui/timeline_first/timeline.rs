mod geometry;
mod interaction;
mod painting;

use std::collections::HashMap;

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as icons;
use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, MediaTime, TimelineId, TimelineItem, TimelineItemId, TimelineTrackId,
};

use crate::state::authoring::{
    AuthoringSelection, AuthoringUiState, TimelineGestureKind, TimelineItemGesture,
};

use geometry::{
    format_time, row_top, seconds_to_screen_x, EDGE_WIDTH, MIN_CLIP_WIDTH, ROW_HEIGHT,
    RULER_HEIGHT, SIDEBAR_WIDTH,
};
use interaction::{
    background_context_menu, finish_item_gesture, handle_library_drop, handle_wheel_navigation,
    projected_row_top, run_item_actions, update_item_projection,
};
use painting::{draw_playhead, draw_ruler, item_colors, item_icon, open_icon, paint_background};

#[derive(Clone)]
enum RowKind {
    Track {
        track_id: TimelineTrackId,
        expanded: bool,
    },
    Clip {
        track_id: TimelineTrackId,
        item_id: TimelineItemId,
    },
}

#[derive(Clone)]
struct DisplayRow {
    kind: RowKind,
    label: String,
}

#[derive(Clone, Copy)]
enum DeferredItemAction {
    Split(TimelineItemId),
    Duplicate(TimelineItemId),
    Delete(TimelineItemId),
    Open(TimelineItemId),
}

pub fn timeline_panel(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        ui.centered_and_justified(|ui| ui.label("No Timeline selected"));
        return;
    };

    timeline_header(ui, project, timeline.name.as_str(), state);
    ui.separator();

    let transport_height = 39.0;
    let available = ui.available_rect_before_wrap();
    let canvas_rect = Rect::from_min_max(
        available.min,
        Pos2::new(
            available.max.x,
            (available.max.y - transport_height).max(available.min.y),
        ),
    );
    let transport_rect =
        Rect::from_min_max(Pos2::new(available.min.x, canvas_rect.max.y), available.max);
    ui.allocate_rect(available, Sense::hover());

    let rows = display_rows(project, timeline.id, &state.timeline.expanded_tracks);
    let content_rect = Rect::from_min_max(
        Pos2::new(
            canvas_rect.min.x + SIDEBAR_WIDTH,
            canvas_rect.min.y + RULER_HEIGHT,
        ),
        canvas_rect.max,
    );
    let sidebar_rect = Rect::from_min_max(
        Pos2::new(canvas_rect.min.x, content_rect.min.y),
        Pos2::new(content_rect.min.x, canvas_rect.max.y),
    );

    paint_background(ui, canvas_rect, content_rect, &rows, state);
    handle_wheel_navigation(ui, content_rect, state, rows.len());
    draw_ruler(ui, timeline, state, canvas_rect, content_rect);

    let mut actions = Vec::new();
    background_context_menu(
        ui,
        project,
        timeline.id,
        state,
        service,
        sidebar_rect.union(content_rect),
    );
    draw_rows(
        ui,
        project,
        state,
        &rows,
        sidebar_rect,
        content_rect,
        &mut actions,
    );
    handle_library_drop(
        ui,
        project,
        timeline.id,
        state,
        service,
        &rows,
        content_rect,
    );
    finish_item_gesture(ui, state, service);
    run_item_actions(project, state, service, actions);
    transport(ui, timeline, state, transport_rect);
}

fn timeline_header(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    name: &str,
    state: &mut AuthoringUiState,
) {
    ui.horizontal(|ui| {
        if state.active_timeline_id != project.root_timeline_id {
            let tooltip = if state
                .active_instance_path
                .as_ref()
                .is_some_and(|path| !path.composition_items.is_empty())
            {
                "Back to parent Timeline"
            } else {
                "Back to root Timeline"
            };
            if ui
                .small_button(icons::CARET_LEFT)
                .on_hover_text(tooltip)
                .clicked()
            {
                navigate_to_parent(project, state);
            }
        }
        ui.label(egui::RichText::new(format!("{} {name}", icons::FILM_STRIP)).strong());
        ui.separator();
        ui.label("Clips");
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Drag media or a Node Clip here").weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let zoom_in = ui
                .small_button(icons::PLUS)
                .on_hover_text("Zoom Timeline in");
            if zoom_in.clicked() {
                state.timeline.pixels_per_second =
                    (state.timeline.pixels_per_second * 1.25).min(2_000.0);
            }
            let zoom_out = ui
                .small_button(icons::MINUS)
                .on_hover_text("Zoom Timeline out");
            if zoom_out.clicked() {
                state.timeline.pixels_per_second =
                    (state.timeline.pixels_per_second / 1.25).max(8.0);
            }
            ui.label(format!("{:.0} px/s", state.timeline.pixels_per_second));
        });
    });
}

fn navigate_to_parent(project: &AuthoringProject, state: &mut AuthoringUiState) {
    let parent = state
        .active_instance_path
        .as_mut()
        .and_then(|path| path.composition_items.pop())
        .and_then(|item_id| project.items.get(&item_id))
        .and_then(|item| project.tracks.get(&item.track_id))
        .map(|track| track.timeline_id);
    state.active_timeline_id = parent.unwrap_or(project.root_timeline_id);
    if let Some(timeline) = project.timelines.get(&state.active_timeline_id) {
        state
            .timeline
            .expanded_tracks
            .extend(timeline.track_order.iter().copied());
    }
    if parent.is_none() {
        state.active_instance_path = Some(library::model::authoring::InstancePath::root(
            project.root_timeline_id,
        ));
    }
    state
        .selection
        .replace(AuthoringSelection::Timeline(state.active_timeline_id));
    state.timeline.current_frame = 0;
    state.timeline.set_playing(false);
    state.preview.auto_fit = true;
    state.inspector.invalidate();
}

fn display_rows(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    expanded: &std::collections::HashSet<TimelineTrackId>,
) -> Vec<DisplayRow> {
    let Some(timeline) = project.timelines.get(&timeline_id) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for track_id in timeline.track_order.iter().rev() {
        let Some(track) = project.tracks.get(track_id) else {
            continue;
        };
        let is_expanded = expanded.contains(track_id);
        rows.push(DisplayRow {
            kind: RowKind::Track {
                track_id: *track_id,
                expanded: is_expanded,
            },
            label: track.name.clone(),
        });
        if is_expanded {
            let mut items = project
                .items
                .values()
                .filter(|item| item.track_id == *track_id)
                .collect::<Vec<_>>();
            items.sort_by(|left, right| {
                right
                    .layer
                    .cmp(&left.layer)
                    .then(left.interval.start.cmp(&right.interval.start))
                    .then(left.id.cmp(&right.id))
            });
            rows.extend(items.into_iter().map(|item| DisplayRow {
                kind: RowKind::Clip {
                    track_id: *track_id,
                    item_id: item.id,
                },
                label: item.name.clone(),
            }));
        }
    }
    rows
}
fn draw_rows(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    rows: &[DisplayRow],
    sidebar_rect: Rect,
    content_rect: Rect,
    actions: &mut Vec<DeferredItemAction>,
) {
    let mut collapsed_items: HashMap<TimelineTrackId, Vec<&TimelineItem>> = HashMap::new();
    for item in project.items.values() {
        collapsed_items.entry(item.track_id).or_default().push(item);
    }
    for items in collapsed_items.values_mut() {
        items.sort_by_key(|item| (item.layer, item.interval.start, item.id));
    }

    for (row_index, row) in rows.iter().enumerate() {
        let y = row_top(content_rect, state, row_index);
        let left_rect = Rect::from_min_size(
            Pos2::new(sidebar_rect.min.x, y),
            Vec2::new(sidebar_rect.width(), ROW_HEIGHT),
        );
        let clip_row_rect = Rect::from_min_size(
            Pos2::new(content_rect.min.x, y),
            Vec2::new(content_rect.width(), ROW_HEIGHT),
        );
        if !left_rect.intersects(sidebar_rect) {
            continue;
        }
        match &row.kind {
            RowKind::Track { track_id, expanded } => {
                draw_track_header(ui, state, *track_id, *expanded, &row.label, left_rect);
                if !expanded {
                    for item in collapsed_items.get(track_id).into_iter().flatten() {
                        draw_item(
                            ui,
                            project,
                            state,
                            item,
                            clip_row_rect,
                            content_rect.top(),
                            true,
                            actions,
                        );
                    }
                }
            }
            RowKind::Clip { item_id, .. } => {
                draw_clip_label(ui, state, *item_id, &row.label, left_rect);
                if let Some(item) = project.items.get(item_id) {
                    draw_item(
                        ui,
                        project,
                        state,
                        item,
                        clip_row_rect,
                        content_rect.top(),
                        false,
                        actions,
                    );
                }
            }
        }
    }

    draw_playhead(ui, project, state, content_rect);
}

fn draw_track_header(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    track_id: TimelineTrackId,
    expanded: bool,
    label: &str,
    rect: Rect,
) {
    let response = ui.interact(rect, ui.id().with(("track", track_id)), Sense::click());
    crate::qa::register_component_with_metadata(
        format!("timeline.track:{track_id}"),
        "timeline_track",
        rect,
        true,
        Some(serde_json::json!({"track_id": track_id, "expanded": expanded})),
    );
    if response.clicked() {
        state.selection.replace(AuthoringSelection::Track(track_id));
    }
    let caret_rect = Rect::from_min_size(rect.min, Vec2::new(27.0, rect.height()));
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
    let selected = state
        .selection
        .contains(AuthoringSelection::Track(track_id));
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            0.0,
            if selected {
                Color32::from_rgb(42, 67, 94)
            } else {
                Color32::from_gray(42)
            },
        );
    }
    ui.painter().text(
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
    ui.painter().text(
        Pos2::new(rect.left() + 31.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        ui.visuals().text_color(),
    );
}

fn draw_clip_label(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    item_id: TimelineItemId,
    label: &str,
    rect: Rect,
) {
    let response = ui.interact(rect, ui.id().with(("clip-label", item_id)), Sense::click());
    if response.clicked() {
        state.selection.replace(AuthoringSelection::Item(item_id));
    }
    if state.selection.contains(AuthoringSelection::Item(item_id)) {
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_rgb(40, 59, 78));
    }
    ui.painter().text(
        Pos2::new(rect.left() + 28.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "one clip paint pass needs the shared model, row geometry, and deferred action sink"
)]
fn draw_item(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    item: &TimelineItem,
    mut row_rect: Rect,
    content_top: f32,
    summary: bool,
    actions: &mut Vec<DeferredItemAction>,
) {
    let projected = state
        .timeline
        .item_gesture
        .as_ref()
        .filter(|gesture| gesture.item_id == item.id);
    let interval = projected.map_or(item.interval, |gesture| gesture.projected_interval);
    if let Some(gesture) = projected {
        if gesture.kind == TimelineGestureKind::Move {
            if let Some(projected_top) = projected_row_top(project, state, gesture, content_top) {
                row_rect =
                    Rect::from_min_size(Pos2::new(row_rect.left(), projected_top), row_rect.size());
            }
        }
    }
    let x = seconds_to_screen_x(interval.start.to_seconds_f64() as f32, row_rect, state);
    let width = (interval.duration.to_seconds_f64() as f32 * state.timeline.pixels_per_second)
        .max(MIN_CLIP_WIDTH);
    let vertical_inset = if summary {
        3.0 + (item.layer.rem_euclid(3) as f32)
    } else {
        3.0
    };
    let clip_rect = Rect::from_min_size(
        Pos2::new(x, row_rect.top() + vertical_inset),
        Vec2::new(width, row_rect.height() - vertical_inset - 3.0),
    );
    if !clip_rect.intersects(row_rect) {
        return;
    }

    let response = ui.interact(
        clip_rect,
        ui.id().with(("timeline-item", item.id)),
        Sense::click_and_drag(),
    );
    crate::qa::register_component_with_metadata(
        format!("timeline.item:{}", item.id),
        "timeline_clip",
        clip_rect,
        true,
        Some(serde_json::json!({
            "item_id": item.id,
            "track_id": item.track_id,
            "layer": item.layer,
            "start_seconds": interval.start.to_seconds_f64(),
            "duration_seconds": interval.duration.to_seconds_f64(),
            "summary": summary,
        })),
    );
    if response.clicked() {
        state.selection.replace(AuthoringSelection::Item(item.id));
    }
    if response.double_clicked() {
        actions.push(DeferredItemAction::Open(item.id));
    }
    if response.drag_started() && state.timeline.item_gesture.is_none() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let kind = if pointer.x <= clip_rect.left() + EDGE_WIDTH {
                TimelineGestureKind::TrimStart
            } else if pointer.x >= clip_rect.right() - EDGE_WIDTH {
                TimelineGestureKind::TrimEnd
            } else {
                TimelineGestureKind::Move
            };
            state.selection.replace(AuthoringSelection::Item(item.id));
            state.timeline.item_gesture = Some(TimelineItemGesture {
                item_id: item.id,
                kind,
                pointer_origin: pointer,
                original_track_id: item.track_id,
                original_layer: item.layer,
                original_interval: item.interval,
                projected_track_id: item.track_id,
                projected_layer: item.layer,
                projected_row_item_id: Some(item.id),
                projected_interval: item.interval,
            });
        }
    }
    if response.dragged() {
        if let Some(pointer) = response.interact_pointer_pos() {
            update_item_projection(project, state, pointer, content_top);
        }
    }
    response.context_menu(|ui| {
        if ui.button(format!("{} Open", open_icon(item))).clicked() {
            actions.push(DeferredItemAction::Open(item.id));
            ui.close();
        }
        if ui
            .button(format!("{} Split at playhead", icons::SCISSORS))
            .clicked()
        {
            actions.push(DeferredItemAction::Split(item.id));
            ui.close();
        }
        if ui.button(format!("{} Duplicate", icons::COPY)).clicked() {
            actions.push(DeferredItemAction::Duplicate(item.id));
            ui.close();
        }
        ui.separator();
        if ui.button(format!("{} Delete", icons::TRASH)).clicked() {
            actions.push(DeferredItemAction::Delete(item.id));
            ui.close();
        }
    });

    let (base, accent) = item_colors(project, item);
    let selected = state.selection.contains(AuthoringSelection::Item(item.id));
    ui.painter().rect_filled(clip_rect, 4.0, base);
    ui.painter().rect_stroke(
        clip_rect,
        4.0,
        Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected { Color32::WHITE } else { accent },
        ),
        StrokeKind::Inside,
    );
    ui.painter().rect_filled(
        Rect::from_min_size(clip_rect.min, Vec2::new(4.0, clip_rect.height())),
        3.0,
        accent,
    );
    let text_rect = clip_rect.shrink2(Vec2::new(8.0, 0.0));
    ui.painter().text(
        text_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        format!("{} {}", item_icon(item), item.name),
        egui::FontId::proportional(11.5),
        Color32::WHITE,
    );
    if !summary {
        ui.painter().line_segment(
            [clip_rect.left_top(), clip_rect.left_bottom()],
            Stroke::new(2.0, Color32::from_white_alpha(170)),
        );
        ui.painter().line_segment(
            [clip_rect.right_top(), clip_rect.right_bottom()],
            Stroke::new(2.0, Color32::from_white_alpha(170)),
        );
    }
}
fn transport(
    ui: &mut egui::Ui,
    timeline: &library::model::authoring::Timeline,
    state: &mut AuthoringUiState,
    rect: Rect,
) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(8.0, 4.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                if ui
                    .small_button(icons::SKIP_BACK)
                    .on_hover_text("Go to start")
                    .clicked()
                {
                    state.timeline.seek_frame(0);
                }
                if ui
                    .small_button(icons::CARET_LEFT)
                    .on_hover_text("Previous frame")
                    .clicked()
                {
                    state.timeline.seek_frame(state.timeline.current_frame - 1);
                }
                let play_icon = if state.timeline.is_playing {
                    icons::PAUSE
                } else {
                    icons::PLAY
                };
                let play = ui.button(egui::RichText::new(play_icon).size(18.0));
                crate::qa::register_component("timeline.play", "transport_button", play.rect);
                if play.clicked() {
                    state.timeline.set_playing(!state.timeline.is_playing);
                }
                if ui
                    .small_button(icons::CARET_RIGHT)
                    .on_hover_text("Next frame")
                    .clicked()
                {
                    state.timeline.seek_frame(state.timeline.current_frame + 1);
                }
                ui.separator();
                let seconds =
                    MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)
                        .map_or(0.0, MediaTime::to_seconds_f64);
                ui.monospace(format!(
                    "{}  |  frame {}",
                    format_time(seconds as f32),
                    state.timeline.current_frame
                ));
            });
        },
    );
}
