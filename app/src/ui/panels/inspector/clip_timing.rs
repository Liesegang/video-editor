use std::sync::{Arc, RwLock};

use egui::Ui;
use library::EditorService;
use library::model::Clip;
use library::model::node::{
    CLIP_DURATION_PROPERTY, CLIP_START_TIME_PROPERTY, CLIP_TIME_STRETCH_PROPERTY,
    CLIP_TRIM_IN_PROPERTY,
};
use library::model::project::Project;
use library::model::property::{PropertyDefinition, PropertyValue};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use crate::action::HistoryManager;
use crate::ui::widgets::property_drag_value::FloatDragValueConfig;

use super::properties;

#[allow(
    clippy::too_many_arguments,
    reason = "property sections share owner, model, UI, timing, and history context"
)]
pub(super) fn render_clip_timing(
    ui: &mut Ui,
    clip: &Clip,
    fps: f64,
    project_service: &EditorService,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<Project>>,
    needs_refresh: &mut bool,
) {
    ui.add_space(5.0);
    ui.heading("Timing");
    ui.separator();

    egui::Grid::new(("clip_timing", clip.id))
        .striped(true)
        .show(ui, |ui| {
            let fps = if fps.is_finite() && fps > 0.0 {
                fps
            } else {
                1.0
            };
            let (
                Some(start_definition),
                Some(duration_definition),
                Some(trim_definition),
                Some(stretch_definition),
            ) = (
                Clip::timing_property_definition(CLIP_START_TIME_PROPERTY),
                Clip::timing_property_definition(CLIP_DURATION_PROPERTY),
                Clip::timing_property_definition(CLIP_TRIM_IN_PROPERTY),
                Clip::timing_property_definition(CLIP_TIME_STRETCH_PROPERTY),
            )
            else {
                log::error!("Clip timing definitions are incomplete");
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Clip timing metadata is incomplete.",
                );
                return;
            };
            let start_frame = clip.start_time.into_inner() * fps;
            let duration_frame = clip.duration.into_inner() * fps;
            let trim_in_frame = clip.trim_in.into_inner() * fps;
            let (
                Some(start_config),
                Some(duration_config),
                Some(trim_config),
                Some(stretch_config),
            ) = (
                inspector_timing_drag_config(start_definition, fps, 0.0),
                inspector_timing_drag_config(duration_definition, fps, start_frame),
                inspector_timing_drag_config(trim_definition, fps, 0.0),
                FloatDragValueConfig::from_definition(stretch_definition),
            )
            else {
                log::error!("Clip timing definitions do not use Float UI metadata");
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Clip timing controls have invalid metadata.",
                );
                return;
            };

            ui.label(format!("{} Frame", start_definition.label()));
            let mut edited_start = start_frame;
            let response = ui.add(start_config.widget(&mut edited_start));
            register_clip_timing_control(
                clip.id,
                start_definition,
                &response,
                clip.start_time.into_inner(),
                start_frame,
                fps,
                "frame",
            );
            if response.changed() {
                if let Err(error) = project_service.update_clip_timing(
                    clip.id,
                    edited_start / fps,
                    clip.duration.into_inner(),
                    clip.trim_in.into_inner(),
                ) {
                    log::error!("Failed to update Clip start: {error}");
                } else {
                    *needs_refresh = true;
                }
            }
            commit_timing_edit(&response, project, history_manager);
            ui.end_row();

            ui.label("Out Frame");
            let mut edited_end = start_frame + duration_frame;
            let response = ui.add(duration_config.widget(&mut edited_end));
            register_clip_timing_control(
                clip.id,
                duration_definition,
                &response,
                clip.duration.into_inner(),
                start_frame + duration_frame,
                fps,
                "out_frame",
            );
            if response.changed() {
                let duration = edited_end / fps - clip.start_time.into_inner();
                if let Err(error) = project_service.update_clip_timing(
                    clip.id,
                    clip.start_time.into_inner(),
                    duration,
                    clip.trim_in.into_inner(),
                ) {
                    log::error!("Failed to update Clip duration: {error}");
                } else {
                    *needs_refresh = true;
                }
            }
            commit_timing_edit(&response, project, history_manager);
            ui.end_row();

            ui.label(format!("{} Frame", trim_definition.label()));
            let mut edited_trim = trim_in_frame;
            let response = ui.add(trim_config.widget(&mut edited_trim));
            register_clip_timing_control(
                clip.id,
                trim_definition,
                &response,
                clip.trim_in.into_inner(),
                trim_in_frame,
                fps,
                "frame",
            );
            if response.changed() {
                if let Err(error) = project_service.update_clip_property(
                    clip.id,
                    trim_definition.name(),
                    PropertyValue::Number(OrderedFloat(edited_trim / fps)),
                ) {
                    log::error!("Failed to update Clip source start: {error}");
                } else {
                    *needs_refresh = true;
                }
            }
            commit_timing_edit(&response, project, history_manager);
            ui.end_row();

            ui.label(stretch_definition.label());
            let mut edited_stretch = clip.time_stretch.into_inner();
            let response = ui.add(stretch_config.widget(&mut edited_stretch));
            register_clip_timing_control(
                clip.id,
                stretch_definition,
                &response,
                clip.time_stretch.into_inner(),
                edited_stretch,
                fps,
                "ratio",
            );
            if response.changed() {
                if let Err(error) = project_service.update_clip_property(
                    clip.id,
                    stretch_definition.name(),
                    PropertyValue::Number(OrderedFloat(edited_stretch)),
                ) {
                    log::error!("Failed to update Clip time stretch: {error}");
                } else {
                    *needs_refresh = true;
                }
            }
            commit_timing_edit(&response, project, history_manager);
            ui.end_row();

            ui.label(duration_definition.label());
            ui.label(format!("{duration_frame:.0} fr"));
            ui.end_row();
        });
}

fn register_clip_timing_control(
    clip_id: Uuid,
    definition: &PropertyDefinition,
    response: &egui::Response,
    value: f64,
    display_value: f64,
    fps: f64,
    display_semantics: &str,
) {
    if !crate::qa::is_enabled() {
        return;
    }

    crate::qa::register_component_with_metadata(
        format!("inspector.property.clip:{clip_id}:{}", definition.name()),
        "inspector_property_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "scope": format!("clip:{clip_id}"),
            "property": definition.name(),
            "control_kind": "float_drag",
            "value": value,
            "display_value": display_value,
            "display_semantics": display_semantics,
            "fps": fps,
            "definition": properties::property_definition_metadata(definition),
        })),
    );
}

pub(super) fn inspector_timing_drag_config(
    definition: &PropertyDefinition,
    fps: f64,
    frame_offset: f64,
) -> Option<FloatDragValueConfig> {
    FloatDragValueConfig::from_definition(definition)
        .map(|config| config.transformed(fps, frame_offset, " fr"))
}

fn commit_timing_edit(
    response: &egui::Response,
    project: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) {
    if !(response.drag_stopped() || response.lost_focus()) {
        return;
    }
    match project.read() {
        Ok(project) => history_manager.push_project_state(project.clone()),
        Err(error) => log::error!("Failed to save Clip timing history: {error}"),
    }
}
