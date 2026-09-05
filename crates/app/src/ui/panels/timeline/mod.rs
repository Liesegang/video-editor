mod clip_menu;
mod content;
mod documents;
mod dope_sheet;
pub(crate) mod geometry;
mod interaction;
mod painting;
mod rows;
mod selection;
mod tracks;
mod transition_assignment;
mod transitions;
mod viewport;

#[cfg(test)]
mod interaction_tests;
#[cfg(test)]
mod particle_clip_tests;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as icons;
use library::editor::{AuthoringWaveformService, TimelineEditorService};
use library::model::authoring::{
    AuthoringProject, MediaTime, ModuleDefinitionId, ProjectRevision, TimelineItem, TimelineItemId,
    TransitionCreationCandidate, TransitionId,
};
use library::plugin::PluginManager;

use crate::state::authoring::{
    AuthoringSelection, AuthoringUiState, TimelineClipDisplayMode, TimelineGestureKind,
    TimelineItemGesture,
};
use crate::ui::media_preview::AuthoringMediaPreviewService;

use clip_menu::background_context_menu;
use content::{paint_item_content, ItemContentContext};
use geometry::{
    clip_rect as timeline_clip_rect, format_time, trim_edge_rects, TimelineRowMetrics,
    RULER_HEIGHT, SIDEBAR_WIDTH,
};
use interaction::{
    finish_item_gesture, handle_library_drop, item_gesture_kind, run_item_actions,
    timeline_row_projection, update_item_projection, TimelineRowProjection,
};
use painting::{draw_ruler, item_colors, item_icon, open_icon, paint_background};
use rows::{display_rows, DisplayRow, RowKind};
use rows::{draw_rows, property_row_items};
use selection::{
    apply_item_click_selection, handle_marquee_selection, prepare_item_drag_selection,
};
use viewport::navigate;

#[derive(Clone, Copy)]
enum DeferredItemAction {
    Split(TimelineItemId),
    Duplicate(TimelineItemId),
    Delete(TimelineItemId),
    Open(TimelineItemId),
    ConvertSourceToNodeClip(TimelineItemId),
    AddTransition(TransitionCreationCandidate),
    RemoveTransition(TransitionId),
    EditTransitionLogic(TransitionId),
    AssignTransitionModule {
        transition_id: TransitionId,
        definition_id: ModuleDefinitionId,
    },
    ConfigureTransitionModule {
        transition_id: TransitionId,
        definition_id: ModuleDefinitionId,
    },
    AssignBuiltinTransition(TransitionId),
}

pub fn timeline_panel(
    ui: &mut egui::Ui,
    project_frame: (&Arc<AuthoringProject>, ProjectRevision),
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) {
    let (project, project_revision) = project_frame;
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        ui.centered_and_justified(|ui| ui.label("No Timeline selected"));
        return;
    };

    timeline_header(ui, project, timeline.name.as_str(), state);
    ui.separator();

    let regions = crate::ui::panel_layout::allocate_panel_with_footer(ui, 33.0);
    let canvas_rect = regions.body;
    let transport_rect = regions.footer;

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

    let update_active_gesture =
        ui.input(|input| input.pointer.primary_down() || input.pointer.primary_released());
    if state.timeline.item_gesture.is_some() && update_active_gesture {
        if let Some(pointer) = ui.ctx().pointer_latest_pos() {
            update_item_projection(project, state, pointer, content_rect.top());
            ui.ctx().request_repaint();
        }
    }
    if state.timeline.keyframe_gesture.is_some() && update_active_gesture {
        if let Some(pointer) = ui.ctx().pointer_latest_pos() {
            dope_sheet::update_key_projection(project, state, pointer.x);
            ui.ctx().request_repaint();
        }
    }
    let property_items = property_row_items(project, timeline.id, &state.timeline);
    let rows = display_rows(
        project,
        timeline.id,
        &state.timeline.expanded_tracks,
        &property_items,
        state.active_instance_path.as_ref(),
    );
    tracks::update_projection(ui, project, state, &rows, content_rect, project_revision);
    let rows = tracks::project_rows(rows, state.timeline.track_gesture.as_ref());
    tracks::register_projection_qa(&rows, state.timeline.track_gesture.as_ref(), sidebar_rect);
    let row_projection = timeline_row_projection(
        project,
        &rows,
        &state.timeline.expanded_tracks,
        &property_items,
        state.timeline.item_gesture.as_ref(),
    );
    let visible_row_count = row_projection
        .as_ref()
        .map_or(rows.len(), TimelineRowProjection::visible_row_count);

    let navigation = navigate(ui, content_rect, state, visible_row_count);
    let timeline_canvas = viewport::canvas_state(&state.timeline);
    let row_metrics = TimelineRowMetrics::from_view(&state.timeline);
    crate::qa::register_component_with_metadata(
        "timeline.canvas",
        "timeline_canvas",
        content_rect,
        true,
        Some(serde_json::json!({
            "pan": {"x": timeline_canvas.pan.x, "y": timeline_canvas.pan.y},
            "zoom": {"x": timeline_canvas.zoom.x, "y": timeline_canvas.zoom.y},
            "screen_origin": {"x": content_rect.min.x, "y": content_rect.min.y},
            "row_metrics": {
                "track_height": row_metrics.row_height(),
                "expanded_clip_height": row_metrics.row_height(),
                "gap": row_metrics.gap(),
                "stride": row_metrics.stride(),
                "vertical_zoom": row_metrics.vertical_zoom(),
            },
            "expanded_item_count": state.timeline.expanded_items.len(),
            "keyframe_gesture_active": state.timeline.keyframe_gesture.is_some(),
        })),
    );
    paint_background(ui, canvas_rect, content_rect, visible_row_count, state);
    draw_ruler(ui, timeline, state, canvas_rect, content_rect);
    if let (Some(projection), Some(gesture)) = (
        row_projection.as_ref(),
        state.timeline.item_gesture.as_ref(),
    ) {
        let item_rows = projection
            .ordered_item_rows()
            .into_iter()
            .map(|(item_id, display_row_index)| {
                serde_json::json!({
                    "item_id": item_id,
                    "display_row_index": display_row_index,
                })
            })
            .collect::<Vec<_>>();
        crate::qa::register_component_with_metadata(
            "timeline.reorder_preview",
            "timeline_reorder_preview",
            sidebar_rect.union(content_rect),
            true,
            Some(serde_json::json!({
                "item_id": gesture.item_id,
                "original_track_id": gesture.original_track_id,
                "original_layer": gesture.original_layer,
                "projected_track_id": gesture.projected_track_id,
                "projected_layer": gesture.projected_layer,
                "visible_row_count": projection.visible_row_count(),
                "item_rows": item_rows,
            })),
        );
    }

    let mut actions = Vec::new();
    background_context_menu(
        project,
        timeline.id,
        state,
        service,
        plugins,
        navigation.as_ref().map(|(response, _)| response),
    );
    draw_rows(
        ui,
        project,
        project_revision,
        state,
        &rows,
        row_projection.as_ref(),
        sidebar_rect,
        content_rect,
        &mut actions,
        service,
        waveform,
        media_previews,
    );
    handle_marquee_selection(ui, project, state, &rows, content_rect, navigation);
    handle_library_drop(ui, project, state, service, plugins, &rows, content_rect);
    finish_item_gesture(ui, state, service);
    tracks::finish_gesture(ui, project, state, service);
    dope_sheet::finish_keyframe_gesture(ui, state, service);
    run_item_actions(project, state, service, plugins, actions);
    transport(
        ui,
        timeline,
        state,
        transport_rect,
        content_rect,
        visible_row_count,
    );
    transition_assignment::show(ui.ctx(), project, state, service);
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

#[allow(
    clippy::too_many_arguments,
    reason = "one clip paint pass needs the shared model, row geometry, and deferred action sink"
)]
fn draw_item(
    ui: &mut egui::Ui,
    project: &Arc<AuthoringProject>,
    state: &mut AuthoringUiState,
    item: &TimelineItem,
    row_rect: Rect,
    content_rect: Rect,
    display_row_index: usize,
    summary: bool,
    actions: &mut Vec<DeferredItemAction>,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) {
    let projected = projected_gesture_for_item(state.timeline.item_gesture.as_ref(), item.id);
    let interval = projected.map_or(item.interval, |gesture| gesture.projected_interval);
    let clip_rect = timeline_clip_rect(interval, item.layer, row_rect, &state.timeline, summary);
    let visible_clip_rect = clip_rect.intersect(content_rect);
    if !visible_clip_rect.is_positive() {
        return;
    }

    let response = ui.interact(
        visible_clip_rect,
        ui.id().with(("timeline-item", item.id)),
        Sense::click_and_drag(),
    );
    crate::qa::register_component_with_metadata(
        format!("timeline.item:{}", item.id),
        "timeline_clip",
        visible_clip_rect,
        true,
        Some(serde_json::json!({
            "item_id": item.id,
            "track_id": item.track_id,
            "layer": item.layer,
            "model_layer": item.layer,
            "display_row_index": display_row_index,
            "projected_track_id": projected.filter(|gesture| gesture.kind == TimelineGestureKind::Move).map(|gesture| gesture.projected_track_id),
            "projected_layer": projected.filter(|gesture| gesture.kind == TimelineGestureKind::Move).map(|gesture| gesture.projected_layer),
            "reorder_preview_active": state.timeline.item_gesture.as_ref().is_some_and(|gesture| gesture.kind == TimelineGestureKind::Move),
            "start_seconds": interval.start.to_seconds_f64(),
            "duration_seconds": interval.duration.to_seconds_f64(),
            "summary": summary,
            "row_height": row_rect.height(),
            "display_mode": state.timeline.item_display_mode(item.id, item.track_id).qa_name(),
        })),
    );
    let (trim_start_rect, trim_end_rect) = trim_edge_rects(clip_rect, content_rect);
    for (edge, rect) in [("start", trim_start_rect), ("end", trim_end_rect)] {
        if rect.is_positive() {
            crate::qa::register_component_with_metadata(
                format!("timeline.item.trim_{edge}:{}", item.id),
                "timeline_clip_trim_edge",
                rect,
                true,
                Some(serde_json::json!({
                    "item_id": item.id,
                    "edge": edge,
                    "start_seconds": interval.start.to_seconds_f64(),
                    "duration_seconds": interval.duration.to_seconds_f64(),
                })),
            );
        }
    }
    if response.hovered()
        && ui
            .input(|input| input.pointer.hover_pos())
            .is_some_and(|pointer| {
                !matches!(
                    item_gesture_kind(clip_rect, pointer),
                    TimelineGestureKind::Move
                )
            })
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if response.clicked() {
        apply_item_click_selection(state, item.id, ui.input(|input| input.modifiers));
    }
    if response.double_clicked() {
        actions.push(DeferredItemAction::Open(item.id));
    }
    if response.drag_started() && state.timeline.item_gesture.is_none() {
        if let Some(pointer) = ui
            .input(|input| input.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos())
        {
            let kind = item_gesture_kind(clip_rect, pointer);
            if prepare_item_drag_selection(state, item.id, ui.input(|input| input.modifiers)) {
                state.timeline.item_gesture = Some(TimelineItemGesture {
                    item_id: item.id,
                    kind,
                    pointer_origin: pointer,
                    original_track_id: item.track_id,
                    original_layer: item.layer,
                    original_interval: item.interval,
                    projected_track_id: item.track_id,
                    projected_layer: item.layer,
                    projected_interval: item.interval,
                });
                ui.ctx().request_repaint();
            }
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
        let duplicate = ui.button(format!("{} Duplicate", icons::COPY));
        crate::qa::register_component(
            format!("timeline.item.duplicate:{}", item.id),
            "timeline_context_menu_action",
            duplicate.rect,
        );
        if duplicate.clicked() {
            actions.push(DeferredItemAction::Duplicate(item.id));
            ui.close();
        }
        transitions::add_transition_menu(ui, project, item, actions);
        if !matches!(item.source, library::model::authoring::SourceRef::Module(_)) {
            let convert = ui.button(format!(
                "{} Convert Source to Node Clip",
                icons::SHARE_NETWORK
            ));
            crate::qa::register_component(
                format!("timeline.item.convert_source_to_node_clip:{}", item.id),
                "timeline_context_menu_action",
                convert.rect,
            );
            if convert
                .on_hover_text(
                    "Create a private Node graph for this clip source and its pre-Transform effects",
                )
                .clicked()
            {
                actions.push(DeferredItemAction::ConvertSourceToNodeClip(item.id));
                ui.close();
            }
        }
        ui.separator();
        if ui.button(format!("{} Delete", icons::TRASH)).clicked() {
            actions.push(DeferredItemAction::Delete(item.id));
            ui.close();
        }
    });

    let (base, accent) = item_colors(project, item);
    let selected = state.selection.contains(AuthoringSelection::Item(item.id));
    let painter = ui.painter().with_clip_rect(content_rect);
    painter.rect_filled(clip_rect, 4.0, base);
    painter.rect_stroke(
        clip_rect,
        4.0,
        Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected { Color32::WHITE } else { accent },
        ),
        StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_size(clip_rect.min, Vec2::new(4.0, clip_rect.height())),
        3.0,
        accent,
    );
    match state.timeline.item_display_mode(item.id, item.track_id) {
        TimelineClipDisplayMode::Content => {
            let evaluation_fps = project
                .timelines
                .get(&state.active_timeline_id)
                .map_or(30.0, |timeline| timeline.fps.to_f64());
            paint_item_content(ItemContentContext {
                ui,
                project,
                item,
                clip_rect,
                viewport_rect: content_rect,
                evaluation_fps,
                waveform,
                media_previews,
            });
        }
        TimelineClipDisplayMode::Keyframes => dope_sheet::paint_clip_keyframe_summary(
            ui,
            project,
            item,
            interval,
            clip_rect,
            content_rect,
            state.active_instance_path.as_ref(),
        ),
    }
    let text_rect = clip_rect.shrink2(Vec2::new(8.0, 0.0));
    painter.text(
        text_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        format!("{} {}", item_icon(item), item.name),
        egui::FontId::proportional(11.5),
        Color32::WHITE,
    );
    if !summary {
        painter.line_segment(
            [clip_rect.left_top(), clip_rect.left_bottom()],
            Stroke::new(2.0, Color32::from_white_alpha(170)),
        );
        painter.line_segment(
            [clip_rect.right_top(), clip_rect.right_bottom()],
            Stroke::new(2.0, Color32::from_white_alpha(170)),
        );
    }
}

fn projected_gesture_for_item(
    gesture: Option<&TimelineItemGesture>,
    item_id: TimelineItemId,
) -> Option<&TimelineItemGesture> {
    gesture.filter(|gesture| gesture.item_id == item_id)
}
fn transport(
    ui: &mut egui::Ui,
    timeline: &library::model::authoring::Timeline,
    state: &mut AuthoringUiState,
    rect: Rect,
    content_rect: Rect,
    row_count: usize,
) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(8.0, 0.0))),
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
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let menu = ui.menu_button(icons::SLIDERS_HORIZONTAL, |ui| {
                        ui.set_min_width(230.0);
                        ui.strong("View Scale");
                        ui.add_space(4.0);
                        let mut time_scale = state.timeline.pixels_per_second;
                        let time_scale_range = viewport::time_scale_range();
                        let time = ui.add(
                            egui::Slider::new(&mut time_scale, time_scale_range.clone())
                                .logarithmic(true)
                                .text("Time scale")
                                .suffix(" px/s"),
                        );
                        crate::qa::register_component_with_metadata(
                            "timeline.view_scale.time",
                            "timeline_scale_slider",
                            time.rect,
                            true,
                            Some(serde_json::json!({
                                "axis": "x",
                                "pixels_per_second": state.timeline.pixels_per_second,
                                "minimum": time_scale_range.start(),
                                "maximum": time_scale_range.end(),
                            })),
                        );
                        let metrics = TimelineRowMetrics::from_view(&state.timeline);
                        let mut row_height = metrics.row_height();
                        let rows = ui.add(
                            egui::Slider::new(
                                &mut row_height,
                                TimelineRowMetrics::minimum_row_height()
                                    ..=TimelineRowMetrics::maximum_row_height(),
                            )
                                .text("Row height")
                                .suffix(" px"),
                        );
                        crate::qa::register_component_with_metadata(
                            "timeline.view_scale.rows",
                            "timeline_scale_slider",
                            rows.rect,
                            true,
                            Some(serde_json::json!({
                                "axis": "y",
                                "vertical_zoom": metrics.vertical_zoom(),
                                "row_height": metrics.row_height(),
                                "wheel_shortcut": "Ctrl/Cmd+Shift+wheel",
                                "minimum": TimelineRowMetrics::minimum_row_height(),
                                "maximum": TimelineRowMetrics::maximum_row_height(),
                            })),
                        );
                        let reset = ui.button("Reset");
                        crate::qa::register_component(
                            "timeline.view_scale.reset",
                            "timeline_scale_reset",
                            reset.rect,
                        );
                        let requested = if reset.clicked() {
                            Some((80.0, 1.0))
                        } else if time.changed() || rows.changed() {
                            Some((time_scale, row_height / metrics.world_row_height()))
                        } else {
                            None
                        };
                        if let Some((time_scale, vertical_zoom)) = requested {
                            viewport::set_view_scale(
                                &mut state.timeline,
                                content_rect,
                                row_count,
                                time_scale,
                                vertical_zoom,
                            );
                            ui.ctx().request_repaint();
                        }
                    });
                    let response = menu.response.on_hover_text(
                        "View Scale - Ctrl/Cmd+wheel changes time; Ctrl/Cmd+Shift+wheel changes rows",
                    );
                    let metrics = TimelineRowMetrics::from_view(&state.timeline);
                    crate::qa::register_component_with_metadata(
                        "timeline.view_scale",
                        "timeline_scale_menu",
                        response.rect,
                        true,
                        Some(serde_json::json!({
                            "pixels_per_second": state.timeline.pixels_per_second,
                            "vertical_zoom": metrics.vertical_zoom(),
                            "row_height": metrics.row_height(),
                            "wheel_shortcut": "Ctrl/Cmd+wheel: time; Ctrl/Cmd+Shift+wheel: rows",
                        })),
                    );
                });
            });
        },
    );
    crate::qa::register_component("timeline.footer", "panel_footer", rect);
}
