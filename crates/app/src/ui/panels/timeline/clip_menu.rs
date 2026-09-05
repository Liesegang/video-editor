use egui_phosphor::regular as icons;
use library::editor::TimelineEditorService;
use library::model::authoring::{AuthoringProject, TimelineId, TimelineTrackKind};
use library::plugin::PluginManager;

use crate::state::authoring::{AuthoringSelection, AuthoringUiState};
use crate::ui::clip_creation::{create_basic_clip, BasicClipKind, BasicClipPlacement};

pub(super) fn background_context_menu(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    response: Option<&egui::Response>,
) {
    let Some(response) = response else {
        return;
    };
    response.context_menu(|ui| {
        let new_clip = ui.menu_button(format!("{} New Clip", icons::PLUS), |ui| {
            for (id, label, icon, kind) in [
                ("text", "Text", icons::TEXT_T, BasicClipKind::Text),
                (
                    "rectangle",
                    "Rectangle",
                    icons::SQUARE,
                    BasicClipKind::Rectangle,
                ),
                ("ellipse", "Ellipse", icons::CIRCLE, BasicClipKind::Ellipse),
                ("path", "Path", icons::BEZIER_CURVE, BasicClipKind::Path),
                ("solid", "Solid", icons::PALETTE, BasicClipKind::Solid),
            ] {
                let add = ui.button(format!("{icon} {label}"));
                crate::qa::register_component_with_metadata(
                    format!("timeline.menu.new_clip.{id}"),
                    "timeline_context_menu_action",
                    add.rect,
                    add.enabled(),
                    Some(serde_json::json!({
                        "action": "create_clip",
                        "clip_kind": id,
                        "label": label,
                    })),
                );
                if add.clicked() {
                    match create_basic_clip(
                        project,
                        timeline_id,
                        state,
                        service,
                        plugins,
                        kind,
                        BasicClipPlacement::default(),
                    ) {
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
        crate::qa::register_component(
            "timeline.menu.new_clip",
            "timeline_context_menu_submenu",
            new_clip.response.rect,
        );
        ui.separator();
        let add_track = ui.button(format!("{} Add Track", icons::PLUS));
        crate::qa::register_component(
            "timeline.menu.add_track",
            "timeline_context_menu_action",
            add_track.rect,
        );
        if add_track.clicked() {
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
