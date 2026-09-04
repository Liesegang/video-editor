use std::collections::{HashMap, HashSet};

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind};
use egui_phosphor::regular as icons;
use library::editor::{ModuleItemPlacement, TimelineEditorService};
use library::model::asset::AssetKind;
use library::model::authoring::{
    ordered_track_item_ids, track_item_ids_after_placement, AuthoringProject, CompositionInstance,
    DurationPolicy, MediaTime, ModuleDefinition, ModuleDefinitionSharing, SourceRef, TimelineId,
    TimelineInterval, TimelineItemId, TimelineTrackId, TimelineTrackKind, TransitionProcessor,
};
use library::plugin::PluginManager;

use crate::state::authoring::{
    AuthoringLibraryDrag, AuthoringSelection, AuthoringUiState, AutomationLaneId,
    TimelineGestureKind, TimelineItemGesture,
};
use crate::state::node_editor::{ModuleEditorHost, NodeEditorDocument};
use crate::ui::panels::node_editor::open_transition_document;

use super::geometry::{next_layer, snap_seconds, TimelineRowMetrics};
use super::rows::property_row_items;
use super::viewport::screen_x_to_seconds;
use super::{display_rows, DeferredItemAction, DisplayRow, RowKind};

/// Pure row mapping for a pending item move. The authoritative Project stays
/// untouched until release; every visible Track header, expanded Clip row,
/// and collapsed summary derives its Y position from this one projection.
#[derive(Debug, Default)]
pub(super) struct TimelineRowProjection {
    track_rows: HashMap<TimelineTrackId, usize>,
    track_items: HashMap<TimelineTrackId, Vec<TimelineItemId>>,
    item_rows: HashMap<TimelineItemId, usize>,
    property_rows: HashMap<(TimelineItemId, AutomationLaneId), usize>,
    visible_row_count: usize,
}

impl TimelineRowProjection {
    pub(super) fn row_for_track(&self, track_id: TimelineTrackId) -> Option<usize> {
        self.track_rows.get(&track_id).copied()
    }

    pub(super) fn row_for_item(&self, item_id: TimelineItemId) -> Option<usize> {
        self.item_rows.get(&item_id).copied()
    }

    pub(super) fn items_for_track(&self, track_id: TimelineTrackId) -> Option<&[TimelineItemId]> {
        self.track_items.get(&track_id).map(Vec::as_slice)
    }

    pub(super) fn row_for_property(
        &self,
        item_id: TimelineItemId,
        lane: &AutomationLaneId,
    ) -> Option<usize> {
        self.property_rows.get(&(item_id, lane.clone())).copied()
    }

    pub(super) const fn visible_row_count(&self) -> usize {
        self.visible_row_count
    }

    pub(super) fn ordered_item_rows(&self) -> Vec<(TimelineItemId, usize)> {
        let mut rows = self
            .item_rows
            .iter()
            .map(|(item_id, row)| (*item_id, *row))
            .collect::<Vec<_>>();
        rows.sort_by_key(|(item_id, row)| (*row, *item_id));
        rows
    }
}

pub(super) fn timeline_row_projection(
    project: &AuthoringProject,
    rows: &[DisplayRow],
    expanded_tracks: &HashSet<TimelineTrackId>,
    expanded_items: &HashSet<TimelineItemId>,
    gesture: Option<&TimelineItemGesture>,
) -> Option<TimelineRowProjection> {
    let gesture = gesture.filter(|gesture| gesture.kind == TimelineGestureKind::Move)?;
    project.items.get(&gesture.item_id)?;
    if !rows.iter().any(|row| {
        matches!(
            row.kind,
            RowKind::Track { track_id, .. } if track_id == gesture.projected_track_id
        )
    }) {
        return None;
    }

    let mut projection = TimelineRowProjection::default();
    let mut visible_row = 0;
    for row in rows {
        let RowKind::Track { track_id, .. } = row.kind else {
            continue;
        };
        projection.track_rows.insert(track_id, visible_row);
        let track_row = visible_row;
        visible_row += 1;

        let item_ids = if track_id == gesture.projected_track_id {
            track_item_ids_after_placement(
                project,
                track_id,
                gesture.item_id,
                gesture.projected_layer,
            )
        } else {
            ordered_track_item_ids(
                project,
                track_id,
                (track_id == gesture.original_track_id).then_some(gesture.item_id),
            )
        };
        projection.track_items.insert(track_id, item_ids.clone());

        if expanded_tracks.contains(&track_id) {
            for item_id in item_ids.into_iter().rev() {
                projection.item_rows.insert(item_id, visible_row);
                visible_row += 1;
                if expanded_items.contains(&item_id) {
                    for lane in rows.iter().filter_map(|row| match &row.kind {
                        RowKind::Property {
                            item_id: owner,
                            lane,
                        } if *owner == item_id => Some(lane.clone()),
                        _ => None,
                    }) {
                        projection
                            .property_rows
                            .insert((item_id, lane), visible_row);
                        visible_row += 1;
                    }
                }
            }
        } else {
            for item_id in item_ids {
                projection.item_rows.insert(item_id, track_row);
            }
        }
    }
    projection.visible_row_count = visible_row;
    Some(projection)
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
            if let Some((track_id, layer)) = row_target {
                gesture.projected_track_id = track_id;
                gesture.projected_layer = layer;
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
) -> Option<(TimelineTrackId, i64)> {
    let property_items = property_row_items(project, state.active_timeline_id, &state.timeline);
    let rows = display_rows(
        project,
        state.active_timeline_id,
        &state.timeline.expanded_tracks,
        &property_items,
        state.active_instance_path.as_ref(),
    );
    let metrics = TimelineRowMetrics::from_view(&state.timeline);
    let row = metrics
        .row_index_at(pointer_y, first_row_top, state.timeline.vertical_scroll)
        .and_then(|index| rows.get(index))?;
    let track_id = match row.kind {
        RowKind::Track { track_id, .. } | RowKind::Clip { track_id, .. } => track_id,
        RowKind::Property { .. } => return None,
    };
    if !state.timeline.expanded_tracks.contains(&track_id) {
        return Some((track_id, next_layer(project, track_id)));
    }

    let item_count = ordered_track_item_ids(project, track_id, None).len();
    let markers = clip_insertion_markers(
        &rows,
        track_id,
        item_count,
        first_row_top,
        state.timeline.vertical_scroll,
        metrics,
    );
    let insertion_slot = nearest_clip_insertion_slot(pointer_y, &markers)?;
    let destination_layer =
        destination_layer_for_insertion_slot(project, dragged_item_id, track_id, insertion_slot)?;
    Some((track_id, destination_layer))
}

fn clip_insertion_markers(
    rows: &[DisplayRow],
    track_id: TimelineTrackId,
    item_count: usize,
    first_row_top: f32,
    vertical_scroll: f32,
    metrics: TimelineRowMetrics,
) -> Vec<(usize, f32)> {
    let Some(header_index) = rows
        .iter()
        .position(|row| matches!(row.kind, RowKind::Track { track_id: id, .. } if id == track_id))
    else {
        return Vec::new();
    };
    let clip_rows = rows
        .iter()
        .enumerate()
        .skip(header_index + 1)
        .take_while(|(_, row)| !matches!(row.kind, RowKind::Track { .. }))
        .filter_map(|(index, row)| {
            matches!(row.kind, RowKind::Clip { track_id: id, .. } if id == track_id)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if clip_rows.len() != item_count {
        return Vec::new();
    }
    let track_end = rows
        .iter()
        .enumerate()
        .skip(header_index + 1)
        .find_map(|(index, row)| matches!(row.kind, RowKind::Track { .. }).then_some(index))
        .unwrap_or(rows.len());
    let mut markers = Vec::with_capacity(item_count + 1);
    if let Some(first) = clip_rows.first().copied() {
        markers.push((
            item_count,
            metrics.boundary_y(first, first_row_top, vertical_scroll),
        ));
    }
    for (visual_index, _) in clip_rows.iter().enumerate() {
        let boundary = clip_rows
            .get(visual_index + 1)
            .copied()
            .unwrap_or(track_end);
        markers.push((
            item_count - visual_index - 1,
            metrics.boundary_y(boundary, first_row_top, vertical_scroll),
        ));
    }
    markers
}

fn nearest_clip_insertion_slot(pointer_y: f32, markers: &[(usize, f32)]) -> Option<usize> {
    markers
        .iter()
        .min_by(|(_, left_y), (_, right_y)| {
            (pointer_y - *left_y)
                .abs()
                .total_cmp(&(pointer_y - *right_y).abs())
        })
        .map(|(slot, _)| *slot)
}

fn destination_layer_for_insertion_slot(
    project: &AuthoringProject,
    dragged_item_id: TimelineItemId,
    target_track_id: TimelineTrackId,
    insertion_slot: usize,
) -> Option<i64> {
    let target_items = ordered_track_item_ids(project, target_track_id, None);
    let source_track_id = project.items.get(&dragged_item_id)?.track_id;
    let destination = if source_track_id == target_track_id {
        let source_index = target_items
            .iter()
            .position(|item_id| *item_id == dragged_item_id)?;
        destination_index_after_removal(source_index, insertion_slot, target_items.len())?
    } else {
        insertion_slot.min(target_items.len())
    };
    i64::try_from(destination).ok()
}

fn destination_index_after_removal(
    source_index: usize,
    insertion_slot: usize,
    item_count: usize,
) -> Option<usize> {
    if item_count == 0 || source_index >= item_count || insertion_slot > item_count {
        return None;
    }
    Some(if insertion_slot > source_index {
        insertion_slot - 1
    } else {
        insertion_slot
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
        ui.ctx().request_repaint();
        return;
    }
    if !released {
        return;
    }
    let Some(gesture) = state.timeline.item_gesture.take() else {
        return;
    };
    ui.ctx().request_repaint();
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
    let target = pointer
        .filter(|position| content_rect.contains(*position))
        .and_then(|position| drop_target(project, rows, state, position.y, content_rect.top()));
    let over = target.is_some();
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
    let seconds = screen_x_to_seconds(pointer.x, content_rect, &state.timeline);
    let Some((track_id, layer)) = target else {
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
                        transition_module_overrides: Vec::new(),
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
                .outputs()
                .next()
                .ok_or_else(|| "Node Clip has no dedicated Output terminal".to_string())?;
            service
                .place_module_item(
                    definition_id,
                    module_placement(track_id, definition.name.clone(), output.id, start, layer)?,
                )
                .map(|(item_id, _, _)| item_id)
                .map_err(|error| error.to_string())
        }
        AuthoringLibraryDrag::NewNodeClip => {
            let (definition, output_id) =
                ModuleDefinition::new_image("Node Clip", ModuleDefinitionSharing::Private);
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
    output_id: library::model::authoring::ModuleOutputId,
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
    let metrics = TimelineRowMetrics::from_view(&state.timeline);
    let index = metrics
        .row_index_at(pointer_y, first_row_top, state.timeline.vertical_scroll)
        .unwrap_or_default();
    let row = rows.get(index).or_else(|| rows.last())?;
    match row.kind {
        RowKind::Track { track_id, .. } => Some((track_id, next_layer(project, track_id))),
        RowKind::Clip { track_id, item_id } => Some((
            track_id,
            project.items.get(&item_id).map_or(0, |item| item.layer + 1),
        )),
        RowKind::Property { .. } => None,
    }
}

pub(super) fn run_item_actions(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
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
                super::documents::open_item(project, state, item_id);
                Ok(())
            }
            DeferredItemAction::ConvertSourceToNodeClip(item_id) => service
                .convert_source_to_node_clip(plugins, item_id)
                .map(|conversion| {
                    state.node_editor.request_document(
                        NodeEditorDocument::ModuleDefinition {
                            definition_id: conversion.definition_id,
                            host: ModuleEditorHost::NodeClip {
                                timeline_item_id: item_id,
                                instance_path: state.active_instance_path.clone(),
                                module_instance_id: conversion.instance_id,
                            },
                        },
                    );
                    state.status = if conversion.retained_post_transform_effects == 0 {
                        format!(
                            "Converted source to Node Clip with {} pre-Transform effect(s)",
                            conversion.moved_pre_transform_effects
                        )
                    } else {
                        format!(
                            "Converted source to Node Clip; moved {} pre-Transform effect(s), kept {} post-Transform effect(s) outside",
                            conversion.moved_pre_transform_effects,
                            conversion.retained_post_transform_effects
                        )
                    };
                }),
            DeferredItemAction::AddTransition(candidate) => {
                super::transitions::add_creation_candidate(service, candidate).map(|()| {
                    state.status = "Added Timeline transition".to_string();
                })
            }
            DeferredItemAction::RemoveTransition(transition_id) => {
                service.remove_transition(transition_id).map(|_| {
                    state.status = "Removed Timeline transition".to_string();
                })
            }
            DeferredItemAction::EditTransitionLogic(transition_id) => {
                open_transition_document(project, state, service, transition_id)
            }
            DeferredItemAction::AssignTransitionModule {
                transition_id,
                definition_id,
            } => service
                .assign_transition_module(transition_id, definition_id)
                .map(|_| {
                    state.status = "Applied reusable Transition Module".to_string();
                }),
            DeferredItemAction::AssignBuiltinTransition(transition_id) => project
                .transitions
                .get(&transition_id)
                .ok_or_else(|| {
                    library::LibraryError::Validation(format!(
                        "Missing Transition {transition_id}"
                    ))
                })
                .and_then(|transition| {
                    let processor = match transition.processor.contract.media_type {
                        library::model::authoring::TransitionMediaType::Image => {
                            TransitionProcessor::cross_dissolve()
                        }
                        library::model::authoring::TransitionMediaType::Audio => {
                            TransitionProcessor::audio_crossfade()
                        }
                    };
                    service.assign_transition_operation(transition_id, processor)
                })
                .map(|_| {
                    state.status = "Applied built-in Transition processor".to_string();
                }),
        };
        if let Err(error) = result {
            state.error = Some(error.to_string());
        }
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
                ("Path", icons::BEZIER_CURVE, BasicClipKind::Path),
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
    Path,
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
                ensemble_operations: Vec::new(),
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
        BasicClipKind::Path => {
            let path = library::model::path::PathValue::new(
                library::model::path::FillRule::NonZero,
                vec![library::model::path::PathContour::new(
                    library::model::path::PathPoint::new(0.0, 0.0),
                    vec![
                        library::model::path::PathSegment::line(
                            library::model::path::PathPoint::new(160.0, 0.0),
                        ),
                        library::model::path::PathSegment::line(
                            library::model::path::PathPoint::new(160.0, 90.0),
                        ),
                        library::model::path::PathSegment::line(
                            library::model::path::PathPoint::new(0.0, 90.0),
                        ),
                    ],
                    true,
                )],
            )
            .map_err(|error| error.to_string())?;
            (
                "Path",
                SourceRef::Shape {
                    shape: library::model::authoring::ShapeSource {
                        shape_kind: library::model::authoring::ShapeKind::Path,
                        parameters: HashMap::from([
                            (
                                "path".to_string(),
                                library::model::property::PropertyValue::Path(path),
                            ),
                            (
                                "width".to_string(),
                                library::model::property::PropertyValue::from(160.0),
                            ),
                            (
                                "height".to_string(),
                                library::model::property::PropertyValue::from(90.0),
                            ),
                        ]),
                    },
                },
            )
        }
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
    use crate::state::authoring::AuthoringUiState;

    use super::super::display_rows;
    use super::super::geometry::TimelineRowMetrics;
    use super::super::tests::fixture;
    use super::{destination_index_after_removal, drop_target};

    #[test]
    fn removal_adjustment_keeps_adjacent_slots_stable() {
        assert_eq!(destination_index_after_removal(2, 0, 4), Some(0));
        assert_eq!(destination_index_after_removal(0, 3, 4), Some(2));
        assert_eq!(destination_index_after_removal(1, 1, 4), Some(1));
        assert_eq!(destination_index_after_removal(1, 2, 4), Some(1));
        assert_eq!(destination_index_after_removal(4, 0, 4), None);
    }

    #[test]
    fn drop_hit_uses_scaled_rows_and_vertical_scroll() {
        let (project, track_id, _) = fixture();
        let mut state = AuthoringUiState::new(project.root_timeline_id);
        state.timeline.expanded_tracks.insert(track_id);
        state.timeline.vertical_zoom = 2.25;
        state.timeline.vertical_scroll = 47.0;
        let rows = display_rows(
            &project,
            project.root_timeline_id,
            &state.timeline.expanded_tracks,
            &state.timeline.expanded_items,
            state.active_instance_path.as_ref(),
        );
        let metrics = TimelineRowMetrics::from_view(&state.timeline);
        let first_row_top = 180.0;

        for (row_index, row) in rows.iter().enumerate().skip(1) {
            let super::super::RowKind::Clip { item_id, .. } = row.kind else {
                panic!("expanded child must be a clip row");
            };
            let pointer_y =
                first_row_top + row_index as f32 * metrics.stride() + metrics.row_height() / 2.0
                    - state.timeline.vertical_scroll;
            assert_eq!(
                drop_target(&project, &rows, &state, pointer_y, first_row_top),
                Some((track_id, project.items[&item_id].layer + 1))
            );
        }
    }
}
