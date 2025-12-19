use crate::state::context::EditorContext;
use egui::Ui;
use library::model::project::project::Project;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};

mod draw;
mod ticks;
mod time_input;

pub fn show_timeline_ruler(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    project_service: &ProjectService,
    project: &Arc<RwLock<Project>>,
    pixels_per_unit: f32,
    scroll_offset_x: f32,
    composition_fps: f64,
) {
    // Get Composition Duration
    let mut current_comp_duration = 300.0; // Default fallback
    if let Some(comp_id) = editor_context.selection.composition_id {
        if let Ok(proj_read) = project.read() {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                current_comp_duration = comp.duration;
            }
        }
    }

    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |h_ui| {
        // Time Input Panel
        let _ = time_input::show_time_input(
            h_ui,
            editor_context,
            composition_fps,
            current_comp_duration,
        );

        h_ui.separator();

        // --- The actual ruler ---
        let (rect, response) = h_ui.allocate_at_least(h_ui.available_size(), egui::Sense::drag());
        let painter = h_ui.painter_at(rect); // Painter for the allocated rect within h_ui
        painter.rect_filled(
            rect,
            0.0,
            h_ui.style().visuals.widgets.noninteractive.bg_fill,
        );

        if response.dragged() && response.dragged_by(egui::PointerButton::Primary) {
            if let Some(pos) = response.interact_pointer_pos() {
                const RULER_INPUT_OFFSET_X: f32 = 0.0;
                let raw_time = ((pos.x - rect.min.x + scroll_offset_x + RULER_INPUT_OFFSET_X)
                    / pixels_per_unit)
                    .max(0.0);
                let snapped = (raw_time * composition_fps as f32).round() / composition_fps as f32;
                let new_time = snapped.min(current_comp_duration as f32);
                editor_context.timeline.current_time = new_time;
                project_service.reset_audio_pump(new_time as f64);
            }
        }

        draw::draw_ruler_marks(
            &painter,
            rect,
            editor_context,
            scroll_offset_x,
            pixels_per_unit,
            composition_fps,
        );
    });
}
