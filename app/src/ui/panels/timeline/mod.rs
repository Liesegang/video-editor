pub mod clip_area;
pub mod controls;
pub mod ruler;
pub mod track_list;
pub mod utils;

use egui::Ui;
use library::model::project::project::Project;
use library::EditorService;
use std::sync::{Arc, RwLock};

use crate::command::CommandRegistry;
use crate::{action::HistoryManager, state::context::EditorContext};

// Re-export functions for easier access
pub use clip_area::show_clip_area;
pub use controls::show_timeline_controls;
pub use ruler::show_timeline_ruler;
pub use track_list::show_track_list;

pub fn timeline_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
    registry: &CommandRegistry,
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
    let pixels_per_unit =
        editor_context.timeline.pixels_per_second * editor_context.timeline.h_zoom;
    let scroll_offset_x = editor_context.timeline.scroll_offset.x;

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

                let (clip_area_rect, _) = show_clip_area(
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
                    registry,
                );

                // Draw cursor after all panels are laid out
                let cx = clip_area_rect.min.x - scroll_offset_x
                    + editor_context.timeline.current_time * pixels_per_unit; // Locked to clip area start

                // Constants for Playhead Display
                const RULER_HEIGHT_ESTIMATE: f32 = 28.0; // Approximation of Ruler Height + Gap
                const VISIBILITY_BUFFER_PX: f32 = 20.0; // Extra buffer for visibility check

                // Draw overlay line
                let line_top = clip_area_rect.min.y - RULER_HEIGHT_ESTIMATE;
                let line_bottom = clip_area_rect.max.y;

                // Draw if visible in clip area horizontal range (extended slightly for safety)
                if cx >= clip_area_rect.min.x - VISIBILITY_BUFFER_PX && cx <= clip_area_rect.max.x {
                    // Use a foreground layer to draw over the ruler (which is in a higher panel)
                    // and bypass clipping of the current central panel
                    let painter = ui_content.ctx().layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("timeline_cursor"),
                    ));

                    painter.line_segment(
                        [egui::pos2(cx, line_top), egui::pos2(cx, line_bottom)],
                        egui::Stroke::new(2.0, egui::Color32::RED),
                    );
                }
            },
        );
    });
}
