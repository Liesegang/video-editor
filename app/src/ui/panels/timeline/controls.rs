use egui::Ui;
use egui_phosphor::regular as icons;
use library::core::model::project::Project;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

pub fn show_timeline_controls(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _history_manager: &mut HistoryManager,
    project_service: &ProjectService,
    _project: &Arc<RwLock<Project>>,
) {
    ui.horizontal(|ui| {
        // Play button
        let play_icon_enum = if editor_context.timeline.is_playing {
            icons::PAUSE
        } else {
            icons::PLAY
        };
        if ui
            .add(egui::Button::new(egui::RichText::new(play_icon_enum)))
            .clicked()
        {
            let is_playing = !editor_context.timeline.is_playing;
            editor_context.timeline.is_playing = is_playing;

            if is_playing {
                // Sync audio engine to current timeline time before starting
                project_service.reset_audio_pump(editor_context.timeline.current_time as f64);
                if let Err(e) = project_service
                    .get_audio_service()
                    .get_audio_engine()
                    .play()
                {
                    log::error!("Failed to start audio playback: {}", e);
                }
            } else {
                if let Err(e) = project_service
                    .get_audio_service()
                    .get_audio_engine()
                    .pause()
                {
                    log::error!("Failed to pause audio playback: {}", e);
                }
            }
        }

        // Time display
        let minutes = (editor_context.timeline.current_time / 60.0).floor();
        let seconds = (editor_context.timeline.current_time % 60.0).floor();
        let ms = ((editor_context.timeline.current_time % 1.0) * 100.0).floor();
        let time_text = format!("{:02}:{:02}.{:02}", minutes, seconds, ms);
        ui.label(egui::RichText::new(time_text).monospace());

        // Spacer
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Zoom reset button
            if ui
                .add(egui::Button::new(egui::RichText::new(icons::FRAME_CORNERS)))
                .clicked()
            {
                editor_context.timeline.h_zoom = 1.0;
                editor_context.timeline.v_zoom = 1.0;
            }

            // Zoom information
            let zoom_text = format!("H-Zoom: {:.1}x", editor_context.timeline.h_zoom);
            ui.label(zoom_text);
        });
    });
}
