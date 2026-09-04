use std::collections::HashMap;

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind};
use egui_phosphor::regular as icons;
use library::editor::{ModuleItemPlacement, TimelineEditorService};
use library::model::asset::AssetKind;
use library::model::authoring::{
    AuthoringProject, CompositionInstance, DurationPolicy, MediaTime, ModuleDefinitionSharing,
    SourceRef, TimelineId, TimelineInterval, TimelineItemId, TimelineTrackId, TimelineTrackKind,
};

use crate::state::authoring::{
    AuthoringLibraryDrag, AuthoringSelection, AuthoringUiState, TimelineGestureKind,
    TimelineItemGesture,
};
use crate::state::module_node_editor::{ModuleEditorHost, ModuleNodeEditorDocument};

use super::geometry::{next_layer, screen_x_to_seconds, snap_seconds, ROW_GAP, ROW_HEIGHT};
use super::{display_rows, DeferredItemAction, DisplayRow, RowKind};

pub(super) fn handle_wheel_navigation(
    ui: &egui::Ui,
    content_rect: Rect,
    state: &mut AuthoringUiState,
    row_count: usize,
) {
    if state.timeline.item_gesture.is_some() || state.timeline.library_drag.is_some() {
        return;
    }
    let hovered = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|position| content_rect.contains(position));
    if !hovered {
        return;
    }
    let (scroll, modifiers) = ui.input(|input| (input.smooth_scroll_delta, input.modifiers));
    if modifiers.ctrl && scroll.y != 0.0 {
        let old = state.timeline.pixels_per_second;
        state.timeline.pixels_per_second = (old * (scroll.y * 0.002).exp()).clamp(8.0, 2_000.0);
    } else {
        let horizontal = if modifiers.shift { scroll.y } else { scroll.x };
        state.timeline.horizontal_scroll = (state.timeline.horizontal_scroll - horizontal).max(0.0);
        let content_height = row_count as f32 * (ROW_HEIGHT + ROW_GAP);
        let max_vertical = (content_height - content_rect.height()).max(0.0);
        state.timeline.vertical_scroll =
            (state.timeline.vertical_scroll - scroll.y).clamp(0.0, max_vertical);
    }
}

pub(super) fn update_item_projection(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    pointer: Pos2,
    content_top: f32,
) {
    let pixels_per_second = state.timeline.pixels_per_second;
    let active_timeline_id = state.active_timeline_id;
    let Some(dragged_item_id) = state
        .timeline
        .item_gesture
        .as_ref()
        .map(|gesture| gesture.item_id)
    else {
        return;
    };
    let row_target = row_target_at(project, state, pointer.y, content_top, dragged_item_id);
    let Some(gesture) = state.timeline.item_gesture.as_mut() else {
        return;
    };
    let delta_seconds =
        f64::from(pointer.x - gesture.pointer_origin.x) / f64::from(pixels_per_second);
    let frame_seconds = project
        .timelines
        .get(&active_timeline_id)
        .map(|timeline| 1.0 / timeline.fps.to_f64())
        .unwrap_or(1.0 / 30.0);
    match gesture.kind {
        TimelineGestureKind::Move => {
            let seconds =
                (gesture.original_interval.start.to_seconds_f64() + delta_seconds).max(0.0);
            let snapped = snap_seconds(
                project,
                active_timeline_id,
                gesture.item_id,
                seconds,
                frame_seconds,
                pixels_per_second,
            );
            if let Ok(start) = MediaTime::from_seconds_f64(snapped, 1_000_000) {
                gesture.projected_interval.start = start;
            }
            if let Some((track_id, layer, row_item_id)) = row_target {
                gesture.projected_track_id = track_id;
                gesture.projected_layer = layer;
                gesture.projected_row_item_id = row_item_id;
            }
        }
        TimelineGestureKind::TrimStart => {
            let original_end = gesture.original_interval.end().ok();
            let Some(original_end) = original_end else {
                return;
            };
            let latest = original_end.to_seconds_f64() - frame_seconds;
            let seconds = (gesture.original_interval.start.to_seconds_f64() + delta_seconds)
                .clamp(0.0, latest.max(0.0));
            let snapped = (seconds / frame_seconds).round() * frame_seconds;
            if let (Ok(start), Ok(duration)) = (
                MediaTime::from_seconds_f64(snapped, 1_000_000),
                MediaTime::from_seconds_f64(original_end.to_seconds_f64() - snapped, 1_000_000),
            ) {
                gesture.projected_interval = TimelineInterval { start, duration };
            }
        }
        TimelineGestureKind::TrimEnd => {
            let seconds = (gesture.original_interval.duration.to_seconds_f64() + delta_seconds)
                .max(frame_seconds);
            let snapped = (seconds / frame_seconds).round() * frame_seconds;
            if let Ok(duration) = MediaTime::from_seconds_f64(snapped, 1_000_000) {
                gesture.projected_interval.duration = duration;
            }
        }
    }
}

fn row_target_at(
    project: &AuthoringProject,
    state: &AuthoringUiState,
    pointer_y: f32,
    first_row_top: f32,
    dragged_item_id: TimelineItemId,
) -> Option<(TimelineTrackId, i64, Option<TimelineItemId>)> {
    let rows = display_rows(
        project,
        state.active_timeline_id,
        &state.timeline.expanded_tracks,
    );
    let row = ((pointer_y - first_row_top + state.timeline.vertical_scroll)
        / (ROW_HEIGHT + ROW_GAP))
        .floor() as isize;
    let row = usize::try_from(row)
        .ok()
        .and_then(|index| rows.get(index))?;
    match row.kind {
        RowKind::Track { track_id, .. } => Some((track_id, next_layer(project, track_id), None)),
        RowKind::Clip { track_id, item_id } => project.items.get(&item_id).map(|item| {
            (
                track_id,
                row_insertion_layer(item_id, item.layer, dragged_item_id),
                Some(item_id),
            )
        }),
    }
}

fn row_insertion_layer(
    row_item_id: TimelineItemId,
    row_layer: i64,
    dragged_item_id: TimelineItemId,
) -> i64 {
    if row_item_id == dragged_item_id {
        row_layer
    } else {
        row_layer.saturating_add(1)
    }
}

pub(super) fn projected_row_top(
    project: &AuthoringProject,
    state: &AuthoringUiState,
    gesture: &TimelineItemGesture,
    content_top: f32,
) -> Option<f32> {
    let rows = display_rows(
        project,
        state.active_timeline_id,
        &state.timeline.expanded_tracks,
    );
    let target_index = rows.iter().position(|row| match row.kind {
        RowKind::Track { track_id, .. } => {
            gesture.projected_row_item_id.is_none() && track_id == gesture.projected_track_id
        }
        RowKind::Clip { track_id, item_id } => {
            track_id == gesture.projected_track_id && gesture.projected_row_item_id == Some(item_id)
        }
    });
    target_index.map(|index| {
        content_top + index as f32 * (ROW_HEIGHT + ROW_GAP) - state.timeline.vertical_scroll
    })
}

pub(super) fn finish_item_gesture(
    ui: &egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let (released, down, escape) = ui.input(|input| {
        (
            input.pointer.primary_released(),
            input.pointer.primary_down(),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if escape || (state.timeline.item_gesture.is_some() && !down && !released) {
        state.timeline.item_gesture = None;
        return;
    }
    if !released {
        return;
    }
    let Some(gesture) = state.timeline.item_gesture.take() else {
        return;
    };
    if !gesture.changed() {
        return;
    }
    let result = match gesture.kind {
        TimelineGestureKind::Move => service.move_item(
            gesture.item_id,
            gesture.projected_track_id,
            gesture.projected_interval.start,
            gesture.projected_layer,
        ),
        TimelineGestureKind::TrimStart | TimelineGestureKind::TrimEnd => {
            service.trim_item(gesture.item_id, gesture.projected_interval)
        }
    };
    if let Err(error) = result {
        state.error = Some(error.to_string());
    }
}

pub(super) fn handle_library_drop(
    ui: &egui::Ui,
    project: &AuthoringProject,
    timeline_id: TimelineId,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    rows: &[DisplayRow],
    content_rect: Rect,
) {
    let Some(payload) = state.timeline.library_drag else {
        return;
    };
    let pointer = ui.ctx().pointer_latest_pos();
    let over = pointer.is_some_and(|position| content_rect.contains(position));
    if over {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
        ui.painter().rect_stroke(
            content_rect.shrink(2.0),
            0.0,
            Stroke::new(2.0, Color32::from_rgb(86, 177, 255)),
            StrokeKind::Inside,
        );
    }
    let released = ui.input(|input| input.pointer.primary_released());
    let cancelled = ui.input(|input| input.key_pressed(egui::Key::Escape));
    if cancelled || (released && !over) {
        state.timeline.library_drag = None;
        return;
    }
    if !released || !over {
        return;
    }
    state.timeline.library_drag = None;
    let Some(pointer) = pointer else {
        return;
    };
    let seconds = screen_x_to_seconds(pointer.x, content_rect, state);
    let Some((track_id, layer)) = drop_target(project, rows, state, pointer.y, content_rect.top())
    else {
        state.error = Some("Create a Track before placing media".to_string());
        return;
    };
    let start = MediaTime::from_seconds_f64(f64::from(seconds.max(0.0)), 1_000_000)
        .unwrap_or_else(|_| MediaTime::zero());
    let result = place_payload(
        project,
        timeline_id,
        payload,
        track_id,
        layer,
        start,
        service,
    );
    match result {
        Ok(item_id) => {
            state.timeline.expanded_tracks.insert(track_id);
            state.selection.replace(AuthoringSelection::Item(item_id));
            state.status = "Placed clip on Timeline".to_string();
        }
        Err(error) => state.error = Some(error),
    }
}

fn place_payload(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    payload: AuthoringLibraryDrag,
    track_id: TimelineTrackId,
    layer: i64,
    start: MediaTime,
    service: &TimelineEditorService,
) -> Result<TimelineItemId, String> {
    match payload {
        AuthoringLibraryDrag::Asset(asset_id) => {
            let asset = project
                .assets
                .iter()
                .find(|asset| asset.id == asset_id)
                .ok_or_else(|| format!("Missing Asset {asset_id}"))?;
            let duration_seconds = asset.duration.unwrap_or(match asset.kind {
                AssetKind::Image => 5.0,
                _ => 10.0,
            });
            let duration =
                MediaTime::from_seconds_f64(duration_seconds.max(1.0 / 120.0), 1_000_000)?;
            service
                .add_item(
                    track_id,
                    asset.name.clone(),
                    SourceRef::Asset { asset_id },
                    TimelineInterval::new(start, duration)?,
                    layer,
                )
                .map(|(item_id, _)| item_id)
                .map_err(|error| error.to_string())
        }
        AuthoringLibraryDrag::Timeline(nested_id) => {
            if nested_id == timeline_id {
                return Err("A Timeline cannot contain itself".to_string());
            }
            let nested = project
                .timelines
                .get(&nested_id)
                .ok_or_else(|| format!("Missing nested Timeline {nested_id}"))?;
            service
                .add_item(
                    track_id,
                    nested.name.clone(),
                    SourceRef::Composition(CompositionInstance {
                        timeline_id: nested_id,
                        duration_policy: DurationPolicy::Fixed,
                        parameter_overrides: HashMap::new(),
                    }),
                    TimelineInterval::new(start, nested.duration)?,
                    layer,
                )
                .map(|(item_id, _)| item_id)
                .map_err(|error| error.to_string())
        }
        AuthoringLibraryDrag::ModuleDefinition(definition_id) => {
            let definition = project
                .module_definitions
                .get(&definition_id)
                .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
            let output = definition
                .interface
                .media_outputs
                .first()
                .ok_or_else(|| "Node Clip has no published media output".to_string())?;
            service
                .place_module_item(
                    definition_id,
                    module_placement(track_id, definition.name.clone(), output.id, start, layer)?,
                )
                .map(|(item_id, _, _)| item_id)
                .map_err(|error| error.to_string())
        }
        AuthoringLibraryDrag::NewNodeClip => {
            let (definition, output_id) = super::super::image_module_definition(
                "Node Clip",
                ModuleDefinitionSharing::Private,
            );
            service
                .create_private_module_item(
                    definition,
                    module_placement(track_id, "Node Clip".to_string(), output_id, start, layer)?,
                )
                .map(|(item_id, _, _)| item_id)
                .map_err(|error| error.to_string())
        }
    }
}

fn module_placement(
    track_id: TimelineTrackId,
    name: String,
    output_id: library::model::authoring::PublishedMediaOutputId,
    start: MediaTime,
    layer: i64,
) -> Result<ModuleItemPlacement, String> {
    Ok(ModuleItemPlacement {
        track_id,
        name,
        output_id,
        interval: TimelineInterval::new(start, MediaTime::new(5, 1)?)?,
        layer,
        parameter_overrides: HashMap::new(),
        input_bindings: HashMap::new(),
    })
}

fn drop_target(
    project: &AuthoringProject,
    rows: &[DisplayRow],
    state: &AuthoringUiState,
    pointer_y: f32,
    first_row_top: f32,
) -> Option<(TimelineTrackId, i64)> {
    let index = ((pointer_y - first_row_top + state.timeline.vertical_scroll)
        / (ROW_HEIGHT + ROW_GAP))
        .floor()
        .max(0.0) as usize;
    let row = rows.get(index).or_else(|| rows.last())?;
    Some(match row.kind {
        RowKind::Track { track_id, .. } => (track_id, next_layer(project, track_id)),
        RowKind::Clip { track_id, item_id } => (
            track_id,
            project.items.get(&item_id).map_or(0, |item| item.layer + 1),
        ),
    })
}

pub(super) fn run_item_actions(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    actions: Vec<DeferredItemAction>,
) {
    for action in actions {
        let result = match action {
            DeferredItemAction::Split(item_id) => {
                let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
                    continue;
                };
                MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)
                    .map_err(library::LibraryError::Validation)
                    .and_then(|time| service.split_item(item_id, time).map(|_| ()))
            }
            DeferredItemAction::Duplicate(item_id) => {
                let Some(item) = project.items.get(&item_id) else {
                    continue;
                };
                item.interval
                    .end()
                    .map_err(library::LibraryError::Validation)
                    .and_then(|start| {
                        service
                            .duplicate_item(item_id, start, item.layer + 1)
                            .map(|_| ())
                    })
            }
            DeferredItemAction::Delete(item_id) => service.delete_item(item_id).map(|_| ()),
            DeferredItemAction::Open(item_id) => {
                open_item(project, state, item_id);
                Ok(())
            }
        };
        if let Err(error) = result {
            state.error = Some(error.to_string());
        }
    }
}

fn open_item(project: &AuthoringProject, state: &mut AuthoringUiState, item_id: TimelineItemId) {
    let Some(item) = project.items.get(&item_id) else {
        return;
    };
    match &item.source {
        SourceRef::Composition(instance) => {
            state.active_instance_path = state
                .active_instance_path
                .as_ref()
                .map(|path| path.nested(item.id));
            state.active_timeline_id = instance.timeline_id;
            if let Some(timeline) = project.timelines.get(&instance.timeline_id) {
                state
                    .timeline
                    .expanded_tracks
                    .extend(timeline.track_order.iter().copied());
            }
            state
                .selection
                .replace(AuthoringSelection::Timeline(instance.timeline_id));
            state.timeline.current_frame = 0;
            state.timeline.set_playing(false);
            state.preview.auto_fit = true;
        }
        SourceRef::Module(invocation) => {
            let Some(instance) = project.module_instances.get(&invocation.instance_id) else {
                state.error = Some("Node Clip instance is missing".to_string());
                return;
            };
            state
                .node_editor
                .request_document(ModuleNodeEditorDocument::ModuleDefinition {
                    definition_id: instance.definition_id,
                    host: ModuleEditorHost::NodeClip {
                        timeline_item_id: item.id,
                        instance_path: state.active_instance_path.clone(),
                        module_instance_id: instance.id,
                    },
                });
        }
        _ => {}
    }
}

pub(super) fn background_context_menu(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    timeline_id: TimelineId,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    rect: Rect,
) {
    let response = ui.interact(
        rect,
        ui.id().with("timeline-background-menu"),
        Sense::click(),
    );
    response.context_menu(|ui| {
        ui.menu_button(format!("{} New Clip", icons::PLUS), |ui| {
            for (label, icon, kind) in [
                ("Text", icons::TEXT_T, BasicClipKind::Text),
                ("Rectangle", icons::SQUARE, BasicClipKind::Rectangle),
                ("Ellipse", icons::CIRCLE, BasicClipKind::Ellipse),
                ("Solid", icons::PALETTE, BasicClipKind::Solid),
            ] {
                if ui.button(format!("{icon} {label}")).clicked() {
                    match create_basic_clip(project, timeline_id, state, service, kind) {
                        Ok(item_id) => {
                            state.selection.replace(AuthoringSelection::Item(item_id));
                            state.status = format!("Created {label} clip");
                        }
                        Err(error) => state.error = Some(error),
                    }
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui.button(format!("{} Add Track", icons::PLUS)).clicked() {
            match service.add_track(
                timeline_id,
                "Video".to_string(),
                TimelineTrackKind::AudioVisual,
            ) {
                Ok((track_id, _)) => {
                    state.timeline.expanded_tracks.insert(track_id);
                    state.selection.replace(AuthoringSelection::Track(track_id));
                    state.status = "Created Track".to_string();
                }
                Err(error) => {
                    log::error!("Cannot add Timeline Track: {error}");
                    state.error = Some(error.to_string());
                }
            }
            ui.close();
        }
    });
}

#[derive(Clone, Copy)]
enum BasicClipKind {
    Text,
    Rectangle,
    Ellipse,
    Solid,
}

fn create_basic_clip(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    state: &AuthoringUiState,
    service: &TimelineEditorService,
    kind: BasicClipKind,
) -> Result<TimelineItemId, String> {
    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
    let selected_track = state
        .selection
        .primary()
        .and_then(|selection| match selection {
            AuthoringSelection::Track(track_id) => Some(track_id),
            AuthoringSelection::Item(item_id) => {
                project.items.get(&item_id).map(|item| item.track_id)
            }
            _ => None,
        });
    let track_id = selected_track
        .filter(|track_id| {
            project
                .tracks
                .get(track_id)
                .is_some_and(|track| track.timeline_id == timeline_id)
        })
        .or_else(|| timeline.track_order.first().copied())
        .ok_or_else(|| "Add a Track before creating a clip".to_string())?;
    let start = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)?;
    let duration = MediaTime::new(5, 1)?;
    let (name, source) = match kind {
        BasicClipKind::Text => (
            "Text",
            SourceRef::Text {
                text: "Text".to_string(),
            },
        ),
        BasicClipKind::Rectangle => (
            "Rectangle",
            SourceRef::Shape {
                shape: library::model::authoring::ShapeSource {
                    shape_kind: library::model::authoring::ShapeKind::Rectangle,
                    parameters: HashMap::new(),
                },
            },
        ),
        BasicClipKind::Ellipse => (
            "Ellipse",
            SourceRef::Shape {
                shape: library::model::authoring::ShapeSource {
                    shape_kind: library::model::authoring::ShapeKind::Ellipse,
                    parameters: HashMap::new(),
                },
            },
        ),
        BasicClipKind::Solid => (
            "Solid",
            SourceRef::Solid {
                color: library::model::frame::color::Color::black(),
            },
        ),
    };
    service
        .add_item(
            track_id,
            name.to_string(),
            source,
            TimelineInterval::new(start, duration)?,
            next_layer(project, track_id),
        )
        .map(|(item_id, _)| item_id)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::row_insertion_layer;
    use library::model::authoring::TimelineItemId;

    #[test]
    fn horizontal_drag_over_its_own_expanded_row_preserves_layer() {
        let dragged = TimelineItemId::new();
        assert_eq!(row_insertion_layer(dragged, 2, dragged), 2);
    }

    #[test]
    fn dragging_over_another_row_targets_the_gap_above_it() {
        let dragged = TimelineItemId::new();
        let target = TimelineItemId::new();
        assert_eq!(row_insertion_layer(target, 2, dragged), 3);
    }
}
