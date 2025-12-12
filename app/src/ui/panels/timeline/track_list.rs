use egui::Ui;
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::model::project::Track;
use library::service::project_service::ProjectService;
use log::error;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

pub fn show_track_list(
    ui_content: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
    sidebar_width: f32,
) -> (usize, f32, f32) {
    let row_height = 30.0;
    let track_spacing = 2.0;

    let (track_list_rect, track_list_response) = ui_content.allocate_exact_size(
        egui::vec2(sidebar_width, ui_content.available_height()),
        egui::Sense::click_and_drag(),
    );
    let track_list_painter = ui_content.painter_at(track_list_rect);
    track_list_painter.rect_filled(
        track_list_rect,
        0.0,
        ui_content.style().visuals.window_fill(),
    );

    let mut current_tracks: Vec<Track> = Vec::new();
    let selected_composition_id = editor_context.selection.composition_id;
    // CRITICAL CHANGE: Acquire read lock, clone necessary data, then release the lock.
    if let Some(comp_id) = selected_composition_id {
        // Scope for the read lock to ensure it's dropped before any write attempts
        {
            if let Ok(proj_read) = project.read() {
                if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                    current_tracks = comp.tracks.clone();
                }
            }
        } // `proj_read` lock is dropped here
    }
    let num_tracks = current_tracks.len();

    for (i, track) in current_tracks.iter().enumerate() {
        let y = track_list_rect.min.y
            + (i as f32 * (row_height + track_spacing))
            + editor_context.timeline.scroll_offset.y;
        let track_label_rect = egui::Rect::from_min_size(
            egui::pos2(track_list_rect.min.x, y),
            egui::vec2(track_list_rect.width(), row_height),
        );

        if track_list_rect.intersects(track_label_rect) {
            let track_interaction_response = ui_content
                .interact(
                    track_label_rect,
                    egui::Id::new(track.id).with("track_label_interact"),
                    egui::Sense::click(),
                )
                .on_hover_text(format!("Track ID: {}", track.id));

            track_interaction_response.context_menu(|ui| {
                if let Some(comp_id) = editor_context.selection.composition_id {
                    if ui
                        .button(format!("{} Remove Track", icons::TRASH))
                        .clicked()
                    {
                        if let Err(e) = project_service.remove_track(comp_id, track.id) {
                            error!("Failed to remove track: {:?}", e);
                        } else {
                            // If the removed track was selected, deselect it
                            if editor_context.selection.track_id == Some(track.id) {
                                editor_context.selection.track_id = None;
                                editor_context.selection.entity_id = None;
                            }
                            let current_state = project.read().unwrap().clone();
                            history_manager.push_project_state(current_state);
                            ui.close();
                        }
                    }
                }
            });
            if track_interaction_response.clicked() {
                editor_context.selection.track_id = Some(track.id);
            }

            track_list_painter.rect_filled(
                track_label_rect,
                0.0,
                if editor_context.selection.track_id == Some(track.id) {
                    egui::Color32::from_rgb(50, 80, 120)
                } else if i % 2 == 0 {
                    egui::Color32::from_gray(50)
                } else {
                    egui::Color32::from_gray(60)
                },
            );
            track_list_painter.text(
                track_label_rect.left_center() + egui::vec2(5.0, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("Track {}", track.name),
                egui::FontId::monospace(10.0),
                egui::Color32::GRAY,
            );
        }
    }

    track_list_response.context_menu(|ui_content| {
        if let Some(comp_id) = editor_context.selection.composition_id {
            if ui_content
                .add(egui::Button::new(egui::RichText::new(format!(
                    "{} Add Track",
                    icons::PLUS
                ))))
                .clicked()
            {
                project_service
                    .add_track(comp_id, "New Track")
                    .expect("Failed to add track");
                let current_state = project.read().unwrap().clone();
                history_manager.push_project_state(current_state);
                ui_content.close();
            }
        } else {
            ui_content.label("Select a Composition first");
        }
    });

    (num_tracks, row_height, track_spacing)
}
