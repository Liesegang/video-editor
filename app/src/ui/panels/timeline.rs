use egui::Ui;
use std::sync::{Arc, RwLock};
use library::model::project::project::Project;
use library::model::project::Track;
use library::service::project_service::ProjectService;

use crate::{
    action::{
        HistoryManager,
    },
    state::context::EditorContext,
    model::assets::AssetKind,
};

// Helper function to show the timeline ruler
fn show_timeline_ruler(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _project_service: &ProjectService, // Not used here, but keeping signature for consistency
    _project: &Arc<RwLock<Project>>, // Not used here, but keeping signature for consistency
) {
    let (outer_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.available_height()),
        egui::Sense::hover(),
    );

    ui.horizontal(|ui| {
        // Spacer for the track list
        let (spacer_rect, _) = ui
            .allocate_exact_size(egui::vec2(100.0, outer_rect.height()), egui::Sense::hover());
        ui.painter_at(spacer_rect).rect_filled(
            spacer_rect,
            0.0,
            ui.style().visuals.widgets.noninteractive.bg_fill,
        );

        // The actual ruler
        let (rect, _) = ui.allocate_at_least(
            egui::vec2(ui.available_width(), outer_rect.height()),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, ui.style().visuals.widgets.noninteractive.bg_fill);

        let time_scale = editor_context.timeline_pixels_per_second * editor_context.timeline_h_zoom;
        let start_x = rect.min.x + editor_context.timeline_scroll_offset.x;

        // Determine the density of markers based on zoom
        let (major_interval, minor_interval) = if time_scale > 150.0 {
            (1.0, 0.5)
        } else if time_scale > 50.0 {
            (1.0, 1.0) // No minor
        } else if time_scale > 15.0 {
            (5.0, 1.0)
        } else {
            (10.0, 5.0)
        };

        let first_second = (-editor_context.timeline_scroll_offset.x / time_scale).floor() as i32;
        let last_second =
            ((-editor_context.timeline_scroll_offset.x + rect.width()) / time_scale).ceil() as i32;

        for sec in first_second..=last_second {
            let s = sec as f32;
            if s % minor_interval == 0.0 {
                let x = start_x + s * time_scale;
                if x >= rect.min.x && x <= rect.max.x {
                    let is_major = s % major_interval == 0.0;
                    let _height = if is_major {
                        rect.height()
                    } else {
                        rect.height() * 0.5
                    };
                    painter.line_segment(
                        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                        ui.style().visuals.widgets.noninteractive.bg_stroke,
                    );
                    if is_major {
                        painter.text(
                            egui::pos2(x + 2.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!("{}s", s),
                            egui::FontId::monospace(10.0),
                            ui.style().visuals.text_color(),
                        );
                    }
                }
            }
        }
    });
}

// Helper function to show the timeline controls
fn show_timeline_controls(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _history_manager: &mut HistoryManager, // Not used directly here
    _project_service: &ProjectService,
    _project: &Arc<RwLock<Project>>, // Used for undo/redo if implemented directly, or passed to commands
) {
    ui.horizontal(|ui| {
        // Play button
        if ui.button(if editor_context.is_playing { "◼" } else { "▶" }).clicked() {
            editor_context.is_playing = !editor_context.is_playing;
        }

        // Time display
        let minutes = (editor_context.current_time / 60.0).floor();
        let seconds = (editor_context.current_time % 60.0).floor();
        let ms = ((editor_context.current_time % 1.0) * 100.0).floor();
        let time_text = format!("{:02}:{:02}.{:02}", minutes, seconds, ms);
        ui.label(egui::RichText::new(time_text).monospace());

        // Spacer
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Zoom reset button
            if ui.button("1:1").clicked() {
                editor_context.timeline_h_zoom = 1.0;
                editor_context.timeline_v_zoom = 1.0;
            }

            // Zoom information
            let zoom_text = format!("H-Zoom: {:.1}x", editor_context.timeline_h_zoom);
            ui.label(zoom_text);
        });
    });
}


pub fn timeline_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &ProjectService,
    project: &Arc<RwLock<Project>>,
) {
    // Use panels to divide the space correctly
    egui::TopBottomPanel::top("timeline_ruler_panel")
        .exact_height(20.0)
        .show_inside(ui, |ui| {
            show_timeline_ruler(ui, editor_context, project_service, project);
        });

    egui::TopBottomPanel::bottom("timeline_controls_panel")
        .exact_height(40.0)
        .show_inside(ui, |ui| {
            ui.separator();
            show_timeline_controls(ui, editor_context, history_manager, project_service, project);
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        // Main timeline content
        ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
            // --- Track list sidebar ---
            let (track_list_rect, track_list_response) = ui.allocate_exact_size(
                egui::vec2(100.0, ui.available_height()),
                egui::Sense::click_and_drag(),
            );
            let track_list_painter = ui.painter_at(track_list_rect);
            track_list_painter.rect_filled(
                track_list_rect,
                0.0,
                ui.style().visuals.window_fill(),
            ); // Fill entire sidebar background

            let row_height = 30.0;
            let track_spacing = 2.0;

            let mut current_tracks: Vec<Track> = Vec::new();
            if let Ok(proj_read) = project.read() {
                if let Some(comp) = editor_context.get_current_composition(&proj_read) {
                    current_tracks = comp.tracks.clone();
                }
            }
            let num_tracks = current_tracks.len();

            for (i, track) in current_tracks.iter().enumerate() {
                let y = track_list_rect.min.y
                    + (i as f32 * (row_height + track_spacing))
                    + editor_context.timeline_scroll_offset.y;
                let track_label_rect = egui::Rect::from_min_size(
                    egui::pos2(track_list_rect.min.x, y),
                    egui::vec2(track_list_rect.width(), row_height),
                );

                if track_list_rect.intersects(track_label_rect) {
                    let track_interaction_response = ui
                        .interact(
                            track_label_rect,
                            egui::Id::new(track.id).with("track_label_interact"),
                            egui::Sense::click(),
                        )
                        .on_hover_text(format!("Track ID: {}", track.id));
                    if track_interaction_response.clicked() {
                        editor_context.selected_track_id = Some(track.id);
                    }

                    // Draw alternating background for this row
                    track_list_painter.rect_filled(
                        track_label_rect,
                        0.0,
                        if editor_context.selected_track_id == Some(track.id) {
                            egui::Color32::from_rgb(50, 80, 120)
                        } else if i % 2 == 0 {
                            egui::Color32::from_gray(50)
                        } else {
                            egui::Color32::from_gray(60)
                        },
                    );
                    // Draw text label
                    track_list_painter.text(
                        track_label_rect.left_center() + egui::vec2(5.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("Track {}", track.name), // Display track name
                        egui::FontId::monospace(10.0),
                        egui::Color32::GRAY,
                    );
                }
            }

            // Track list right-click context menu
            track_list_response.context_menu(|ui| {
                if let Some(comp_id) = editor_context.selected_composition_id {
                    if ui.button("Add Track").clicked() {
                        // TODO: Replace with a Command
                        project_service
                            .add_track(comp_id, "New Track")
                            .expect("Failed to add track");
                        ui.close_menu();
                    }
                    if let Some(track_id) = editor_context.selected_track_id {
                        if ui.button("Remove Selected Track").clicked() {
                            // TODO: Replace with a Command
                            project_service
                                .remove_track(comp_id, track_id)
                                .expect("Failed to remove track");
                            editor_context.selected_track_id = None;
                            editor_context.selected_entity_id = None;
                            ui.close_menu();
                        }
                    } else {
                        ui.label("Select a track to remove");
                    }
                } else {
                    ui.label("Select a Composition first");
                }
            });

            ui.separator();

            // --- Clip area ---
            let (content_rect, response) =
                ui.allocate_at_least(ui.available_size(), egui::Sense::click_and_drag());

            // --- Interaction ---
            if response.hovered() {
                let scroll_delta = ui.input(|i| i.raw_scroll_delta);
                if ui.input(|i| i.modifiers.ctrl) && scroll_delta.y != 0.0 {
                    let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
                    editor_context.timeline_h_zoom =
                        (editor_context.timeline_h_zoom * zoom_factor).clamp(0.1, 10.0);
                } else if scroll_delta.y != 0.0 {
                    editor_context.timeline_scroll_offset.y -= scroll_delta.y;
                }

                if scroll_delta.x != 0.0 {
                    editor_context.timeline_scroll_offset.x -= scroll_delta.x;
                }
            }
            if response.dragged() {
                editor_context.timeline_scroll_offset.x += response.drag_delta().x;
                editor_context.timeline_scroll_offset.y += response.drag_delta().y;
            }

            // --- Drawing ---
            let painter = ui.painter_at(content_rect);
            let time_scale = editor_context.timeline_pixels_per_second * editor_context.timeline_h_zoom;

            // Constrain scroll offset
            let max_scroll_y =
                (num_tracks as f32 * (row_height + track_spacing)) - content_rect.height();
            editor_context.timeline_scroll_offset.y = editor_context
                .timeline_scroll_offset
                .y
                .clamp(-max_scroll_y.max(0.0), 0.0);
            editor_context.timeline_scroll_offset.x = editor_context.timeline_scroll_offset.x.min(0.0);

            for i in 0..num_tracks {
                let y = content_rect.min.y
                    + (i as f32 * (row_height + track_spacing))
                    + editor_context.timeline_scroll_offset.y;
                let track_rect = egui::Rect::from_min_size(
                    egui::pos2(content_rect.min.x, y),
                    egui::vec2(content_rect.width(), row_height),
                );
                painter.rect_filled(
                    track_rect,
                    0.0,
                    if i % 2 == 0 {
                        egui::Color32::from_gray(50)
                    } else {
                        egui::Color32::from_gray(60)
                    },
                );
            }

            // Logic for adding entity to track on drag-drop
            if ui.input(|i| i.pointer.any_released()) {
                if let Some(asset_index) = editor_context.dragged_asset {
                    if let Some(mouse_pos) = response.hover_pos() {
                        let drop_time = ((mouse_pos.x
                            - content_rect.min.x
                            - editor_context.timeline_scroll_offset.x)
                            / time_scale)
                            .max(0.0);
                        let drop_track_index =
                            ((mouse_pos.y - content_rect.min.y - editor_context.timeline_scroll_offset.y)
                                / (row_height + track_spacing))
                                .floor() as usize;

                        if let Some(comp_id) = editor_context.selected_composition_id {
                            if let Some(track) = current_tracks.get(drop_track_index) {
                                if let Some(asset) = editor_context.assets.get(asset_index) {
                                    // Handle dropping a Composition asset
                                    if let AssetKind::Composition(_nested_comp_id) = asset.kind
                                    {
                                        // TODO: Replace with a Command
                                        if let Err(e) = project_service.add_entity_to_track(
                                            comp_id,
                                            track.id,
                                            &format!("Nested Comp: {}", asset.name),
                                            drop_time as f64,
                                            (drop_time + asset.duration) as f64,
                                        ) {
                                            eprintln!(
                                                "Failed to add nested composition entity: {:?}",
                                                e
                                            );
                                        }
                                    } else {
                                        // TODO: Replace with a Command
                                        if let Err(e) = project_service.add_entity_to_track(
                                            comp_id,
                                            track.id,
                                            &asset.name, // Use asset name as entity type for now
                                            drop_time as f64,
                                            (drop_time + asset.duration) as f64,
                                        ) {
                                            eprintln!("Failed to add entity to track: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let is_dragging_asset = editor_context.dragged_asset.is_some();
            let mut clicked_on_entity = false;

            if !is_dragging_asset && response.drag_stopped() {
                if let Some(pos) = response.interact_pointer_pos() {
                    editor_context.current_time =
                        ((pos.x - content_rect.min.x - editor_context.timeline_scroll_offset.x)
                            / time_scale)
                            .max(0.0);
                }
            }

            // Draw entities (clips) from the Project model
            if let Ok(proj_read) = project.read() {
                if let Some(comp) = editor_context.get_current_composition(&proj_read) {
                    for track in &comp.tracks {
                        let clip_track_index = comp
                            .tracks
                            .iter()
                            .position(|t| t.id == track.id)
                            .map(|idx| idx as f32)
                            .unwrap_or(0.0);

                        for entity in &track.entities {
                            let asset_index = 0; // Temporary: should derive from entity properties
                            let asset = editor_context.assets.get(asset_index);

                            if let Some(a) = asset {
                                let gc = crate::model::ui_types::GuiClip {
                                    id: entity.id,
                                    name: entity.entity_type.clone(),
                                    track_id: track.id,
                                    start_time: entity.start_time as f32,
                                    duration: (entity.end_time - entity.start_time) as f32,
                                    color: a.color,
                                    position: [
                                        entity.properties.get_f32("position_x").unwrap_or(960.0),
                                        entity.properties.get_f32("position_y").unwrap_or(540.0),
                                    ],
                                    scale: entity.properties.get_f32("scale").unwrap_or(100.0),
                                    opacity: entity
                                        .properties
                                        .get_f32("opacity")
                                        .unwrap_or(100.0),
                                    rotation: entity
                                        .properties
                                        .get_f32("rotation")
                                        .unwrap_or(0.0),
                                    asset_index,
                                };

                                let x = content_rect.min.x
                                    + editor_context.timeline_scroll_offset.x
                                    + gc.start_time * time_scale;
                                let y = content_rect.min.y
                                    + editor_context.timeline_scroll_offset.y
                                    + clip_track_index * (row_height + track_spacing);
                                let clip_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, y),
                                    egui::vec2(gc.duration * time_scale, row_height),
                                );

                                let clip_resp = ui.interact(
                                    clip_rect,
                                    egui::Id::new(gc.id),
                                    egui::Sense::click_and_drag(),
                                );
                                if clip_resp.clicked() {
                                    editor_context.selected_entity_id = Some(gc.id);
                                    editor_context.selected_track_id = Some(gc.track_id);
                                    clicked_on_entity = true;
                                }

                                if clip_resp.drag_started() {
                                    editor_context.selected_entity_id = Some(gc.id);
                                    editor_context.selected_track_id = Some(gc.track_id);
                                }
                                if clip_resp.dragged() && editor_context.selected_entity_id == Some(gc.id)
                                {
                                    let dt = clip_resp.drag_delta().x / time_scale;
                                    // TODO: Replace with a Command
                                    // Update entity's start_time in ProjectService
                                    if let Some(comp_id) = editor_context.selected_composition_id {
                                        if let Some(track_id) = editor_context.selected_track_id {
                                            project_service
                                                .with_track_mut(
                                                    comp_id,
                                                    track_id,
                                                    |track_mut| {
                                                        if let Some(entity_mut) = track_mut
                                                            .entities
                                                            .iter_mut()
                                                            .find(|e| e.id == gc.id)
                                                        {
                                                            entity_mut.start_time = (entity_mut
                                                                .start_time
                                                                + dt as f64)
                                                                .max(0.0);
                                                            entity_mut.end_time = (entity_mut
                                                                .end_time
                                                                + dt as f64)
                                                                .max(entity_mut.start_time);
                                                        }
                                                    },
                                                )
                                                .ok();
                                        }
                                    }
                                }

                                let is_sel = editor_context.selected_entity_id == Some(gc.id);
                                let color = gc.color;
                                let transparent_color = egui::Color32::from_rgba_premultiplied(
                                    color.r(),
                                    color.g(),
                                    color.b(),
                                    150,
                                );

                                painter.rect_filled(clip_rect, 4.0, transparent_color);
                                if is_sel {
                                    painter.rect_stroke(
                                        clip_rect,
                                        4.0,
                                        egui::Stroke::new(2.0, egui::Color32::WHITE),
                                    );
                                }
                                painter.text(
                                    clip_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &gc.name,
                                    egui::FontId::default(),
                                    egui::Color32::BLACK,
                                );
                            }
                        }
                    }
                }
            }

            if clicked_on_entity {
                // Selected entity is already set
            } else if response.clicked() && !is_dragging_asset {
                editor_context.selected_entity_id = None;
            }

            let cx = content_rect.min.x
                + editor_context.timeline_scroll_offset.x
                + editor_context.current_time * time_scale;
            if cx > content_rect.min.x && cx < content_rect.max.x {
                painter.line_segment(
                    [
                        egui::pos2(cx, content_rect.min.y),
                        egui::pos2(cx, content_rect.max.y),
                    ],
                    egui::Stroke::new(2.0, egui::Color32::RED),
                );
            }
        });
    });
}
