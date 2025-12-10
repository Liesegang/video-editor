pub mod clip_area;
pub mod controls;
pub mod ruler;
pub mod track_list;

use egui::Ui;
use library::model::project::project::Project;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};

use crate::{
    action::HistoryManager, model::ui_types::TimelineDisplayMode, state::context::EditorContext,
};

// Re-export functions for easier access
pub use clip_area::show_clip_area;
pub use controls::show_timeline_controls;
pub use ruler::show_timeline_ruler;
pub use track_list::show_track_list;

pub fn timeline_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
) {
    let current_composition_fps: f64;
    // CRITICAL CHANGE: Scope the read lock to only where `project_lock` is needed.
    {
        let project_lock = project.read().unwrap();
        current_composition_fps = editor_context
            .get_current_composition(&project_lock)
            .map(|c| c.fps)
            .unwrap_or(30.0); // Default to 30.0 FPS if no composition is selected or loaded
    } // `project_lock` is dropped here, releasing the read lock.

    const TRACK_LIST_SIDEBAR_WIDTH: f32 = 100.0;
    let pixels_per_unit = match editor_context.timeline_display_mode {
        TimelineDisplayMode::Seconds => {
            editor_context.timeline_pixels_per_second * editor_context.timeline_h_zoom
        }
        TimelineDisplayMode::Frames | TimelineDisplayMode::SecondsAndFrames => {
            (editor_context.timeline_pixels_per_second / current_composition_fps as f32) // Use the extracted fps
                * editor_context.timeline_h_zoom
        }
    };
    let scroll_offset_x = editor_context.timeline_scroll_offset.x;

    egui::TopBottomPanel::top("timeline_ruler_panel")
        .exact_height(20.0)
        .show_inside(ui, |ui| {
            show_timeline_ruler(
                ui,
                editor_context,
                project_service,
                project,
                pixels_per_unit,
                scroll_offset_x,
                current_composition_fps,
            );
        });

    egui::TopBottomPanel::bottom("timeline_controls_panel")
        .exact_height(40.0)
        .show_inside(ui, |ui| {
            ui.separator();
            show_timeline_controls(
                ui,
                editor_context,
                history_manager,
                project_service,
                project,
            );
        });

    let mut central_panel_rect = egui::Rect::NOTHING;
    egui::CentralPanel::default().show_inside(ui, |ui| {
        central_panel_rect = ui.available_rect_before_wrap();
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::TOP),
            |ui_content| {
                let (num_tracks, row_height, track_spacing) = show_track_list(
                    ui_content,
                    editor_context,
                    history_manager,
                    project_service,
                    project,
                    TRACK_LIST_SIDEBAR_WIDTH,
                );

                ui_content.separator();

                show_clip_area(
                    ui_content,
                    editor_context,
                    history_manager,
                    project_service,
                    project,
                    pixels_per_unit,
                    num_tracks,
                    row_height,
                    track_spacing,
                    current_composition_fps,
                );
            },
        );
    });

    // Draw cursor after all panels are laid out
    let cx = central_panel_rect.min.x
      + TRACK_LIST_SIDEBAR_WIDTH // Add the width of the track list sidebar
      - scroll_offset_x
        + editor_context.current_time * pixels_per_unit
        + 24.0; // This offset might need to be adjusted

    let full_timeline_area = ui.available_rect_before_wrap();

    let ruler_y_min = ui.clip_rect().min.y;
    let ruler_y_max = ruler_y_min + 20.0;

    let central_panel_y_min = central_panel_rect.min.y;
    let central_panel_y_max = central_panel_rect.max.y;

    if cx >= full_timeline_area.min.x + 132.0 && cx <= full_timeline_area.max.x {
        ui.painter().line_segment(
            [
                egui::pos2(cx, ruler_y_min),
                egui::pos2(cx, ruler_y_max + 12.0),
            ],
            egui::Stroke::new(2.0, egui::Color32::RED),
        );
    }

    if cx >= central_panel_rect.min.x + TRACK_LIST_SIDEBAR_WIDTH + 24.0
        && cx <= central_panel_rect.max.x
    {
        ui.painter().line_segment(
            [
                egui::pos2(cx, central_panel_y_min - 12.0),
                egui::pos2(cx, central_panel_y_max + 8.0),
            ],
            egui::Stroke::new(2.0, egui::Color32::RED),
        );
    }
}
