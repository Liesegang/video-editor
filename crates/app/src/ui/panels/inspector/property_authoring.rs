//! Shared Inspector presentation for authored values and Timeline automation.
//!
//! The model-specific panels decide how an action is committed. This module
//! owns the single compact row layout so authored properties, Node Clip
//! parameters, and Effect parameters cannot drift into different controls.

use egui::{Align2, FontId, Response, Sense, TextStyle, Ui};
use library::editor::{AuthoringPropertyOwner, TimelineEditorService};
use library::model::authoring::{
    AttachmentId, AutomationTrack, MediaTime, ModuleInstanceId, PublishedParameterId,
    TimelineItemId,
};
use library::model::property::{Property, PropertyDefinition, PropertyValue};

use crate::ui::widgets::property_mode::{
    property_for_mode, property_mode_control_for_state, PropertyAuthoringMode, PropertyModeAction,
    PropertyModeState,
};
use crate::ui::widgets::property_value_editor::property_value_editor;

const PROPERTY_LABEL_WIDTH: f32 = 112.0;

pub(super) struct PropertyRowSpec<'a> {
    pub(super) control_id: &'a str,
    pub(super) label: &'a str,
    pub(super) definition: Option<&'a PropertyDefinition>,
    pub(super) suffix: &'a str,
    pub(super) speed: f64,
    pub(super) mode_state: PropertyModeState,
    pub(super) allow_expression: bool,
}

pub(super) struct PropertyRowResult {
    pub(super) changed: bool,
    pub(super) finished: bool,
    pub(super) mode_action: Option<PropertyModeAction>,
}

/// Draws the canonical compact Inspector row in production order:
/// left-aligned label, authoring-state icon, then typed value.
pub(super) fn property_row(
    ui: &mut Ui,
    value: &mut PropertyValue,
    spec: PropertyRowSpec<'_>,
) -> PropertyRowResult {
    let mut result = PropertyRowResult {
        changed: false,
        finished: false,
        mode_action: None,
    };
    let row = ui.horizontal(|ui| {
        let _label = property_label(ui, spec.control_id, spec.label);
        let (mode_action, _mode) = property_mode_control_for_state(
            ui,
            &format!("inspector.property_mode:{}", spec.control_id),
            spec.mode_state,
            true,
            spec.allow_expression,
        );
        let value_edit = property_value_editor(
            ui,
            egui::Id::new(("inspector.property", spec.control_id)),
            &format!("inspector.property:{}", spec.control_id),
            value,
            spec.definition,
            spec.suffix,
            spec.speed,
        );
        result.changed = value_edit.changed;
        result.finished = value_edit.finished;
        result.mode_action = mode_action;

        #[cfg(test)]
        {
            capture_test_rect("label", _label.rect);
            capture_test_rect("mode", _mode.rect);
            capture_test_rect("value", value_edit.response.rect);
        }
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.property_row:{}", spec.control_id),
        "inspector_property_row",
        row.response.rect,
        row.response.enabled(),
        Some(serde_json::json!({
            "control_id": spec.control_id,
            "column_order": ["label", "property_mode", "value"],
        })),
    );
    result
}

/// The same typed editor without a mode cell, for controls which do not own
/// Timeline automation (for example a Module Effect's instance-only reset).
pub(super) fn property_control(
    ui: &mut Ui,
    control_id: &str,
    value: &mut PropertyValue,
    definition: Option<&PropertyDefinition>,
    suffix: &str,
    speed: f64,
) -> bool {
    property_value_editor(
        ui,
        egui::Id::new(("inspector.property", control_id)),
        &format!("inspector.property:{control_id}"),
        value,
        definition,
        suffix,
        speed,
    )
    .finished
}

/// Draws and commits no model state. The caller owns the expression draft and
/// submits one authoritative command when this returns true.
pub(super) fn expression_source_editor(
    ui: &mut Ui,
    control_id: &str,
    source: &mut String,
    model_source: &str,
) -> bool {
    ui.horizontal(|ui| {
        ui.add_space(PROPERTY_LABEL_WIDTH + ui.spacing().item_spacing.x);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Python Expression").small().strong());
            let response = ui.add(
                egui::TextEdit::multiline(source)
                    .id_salt(("inspector.expression_source", control_id))
                    .code_editor()
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text("value + sin(time) * 10"),
            );
            crate::qa::register_component_with_metadata(
                format!("inspector.expression_source:{control_id}"),
                "inspector_expression_source",
                response.rect,
                response.enabled(),
                Some(serde_json::json!({
                    "control_id": control_id,
                    "source": source,
                    "variables": ["value", "time", "t", "frame", "fps", "resolution"],
                })),
            );
            let keyboard_commit = response.has_focus()
                && ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::Enter));
            (response.lost_focus() || keyboard_commit) && source != model_source
        })
        .inner
    })
    .inner
}

/// Commits a typed value without discarding its active evaluator. A value edit
/// in Keyframe mode updates the playhead key; an Expression edit updates only
/// the type-defining fallback used by the expression runtime.
pub(super) fn commit_authored_value(
    service: &TimelineEditorService,
    owner: AuthoringPropertyOwner,
    key: &str,
    current: Option<&Property>,
    value: PropertyValue,
    local_time: MediaTime,
) -> Result<(), String> {
    match current.map(|property| property.evaluator.as_str()) {
        Some("keyframe") => service
            .upsert_authored_property_keyframe(owner, key.to_string(), local_time, value, None)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        Some("expression") => {
            let mut property = current
                .cloned()
                .ok_or_else(|| format!("Missing authored Property '{key}'"))?;
            property.properties.insert("value".to_string(), value);
            service
                .set_authored_property(owner, key.to_string(), property)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        _ => service
            .set_authored_property_constant(owner, key.to_string(), value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}

/// Applies the compact mode menu through exact-time service commands. The UI
/// never replaces or mutates an authoritative Keyframe Property directly.
pub(super) fn apply_authored_mode_action(
    service: &TimelineEditorService,
    owner: AuthoringPropertyOwner,
    key: &str,
    current: Option<&Property>,
    current_value: PropertyValue,
    local_time: MediaTime,
    action: PropertyModeAction,
) -> Result<(), String> {
    match action {
        PropertyModeAction::SetMode(PropertyAuthoringMode::Keyframe) => service
            .set_authored_property_keyframe_mode(owner, key.to_string(), local_time, current_value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(mode) => {
            let property =
                property_for_mode(current, mode, current_value, local_time.to_seconds_f64())?;
            service
                .set_authored_property(owner, key.to_string(), property)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        PropertyModeAction::ToggleKeyframe => {
            let current_seconds = local_time.to_seconds_f64();
            if let Some(keyframe_id) =
                current.and_then(|property| property.keyframe_id_at(current_seconds, 0.001))
            {
                service
                    .remove_authored_property_keyframe(owner, key, keyframe_id)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            } else {
                service
                    .upsert_authored_property_keyframe(
                        owner,
                        key.to_string(),
                        local_time,
                        current_value,
                        None,
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        }
    }
}

pub(super) fn commit_expression_source(
    service: &TimelineEditorService,
    owner: AuthoringPropertyOwner,
    key: &str,
    current: Option<&Property>,
    source: String,
) -> Result<(), String> {
    let mut property = current
        .filter(|property| property.evaluator == "expression")
        .cloned()
        .ok_or_else(|| format!("Property '{key}' is not an Expression"))?;
    property
        .properties
        .insert("expression".to_string(), PropertyValue::String(source));
    service
        .set_authored_property(owner, key.to_string(), property)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn commit_module_parameter_value(
    service: &TimelineEditorService,
    item_id: TimelineItemId,
    instance_id: ModuleInstanceId,
    parameter_id: PublishedParameterId,
    automation: Option<&AutomationTrack>,
    value: PropertyValue,
    local_time: MediaTime,
) -> Result<(), String> {
    if automation.is_some() {
        service
            .upsert_module_parameter_keyframe(item_id, parameter_id, local_time, value, None)
            .map(|_| ())
            .map_err(|error| error.to_string())
    } else {
        service
            .set_module_parameter(instance_id, parameter_id, value)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

pub(super) fn apply_module_parameter_mode_action(
    service: &TimelineEditorService,
    item_id: TimelineItemId,
    parameter_id: PublishedParameterId,
    automation: Option<&AutomationTrack>,
    value: PropertyValue,
    local_time: MediaTime,
    action: PropertyModeAction,
) -> Result<(), String> {
    match action {
        PropertyModeAction::SetMode(PropertyAuthoringMode::Constant) => service
            .set_module_parameter_constant(item_id, parameter_id, value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Keyframe) => service
            .upsert_module_parameter_keyframe(item_id, parameter_id, local_time, value, None)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Expression) => {
            Err("Module parameter expressions belong inside the Node Module".to_string())
        }
        PropertyModeAction::ToggleKeyframe => {
            if let Some(keyframe_id) = keyframe_at(automation, local_time) {
                if automation.is_some_and(|track| track.keyframes.len() == 1) {
                    service
                        .set_module_parameter_constant(item_id, parameter_id, value)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                } else {
                    service
                        .remove_module_parameter_keyframe(item_id, parameter_id, keyframe_id)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                }
            } else {
                service
                    .upsert_module_parameter_keyframe(
                        item_id,
                        parameter_id,
                        local_time,
                        value,
                        None,
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        }
    }
}

pub(super) fn commit_builtin_effect_value(
    service: &TimelineEditorService,
    attachment_id: AttachmentId,
    key: &str,
    automation: Option<&AutomationTrack>,
    value: PropertyValue,
    local_time: MediaTime,
) -> Result<(), String> {
    if automation.is_some() {
        service
            .upsert_builtin_effect_parameter_keyframe(attachment_id, key, local_time, value, None)
            .map(|_| ())
            .map_err(|error| error.to_string())
    } else {
        service
            .set_builtin_effect_parameter(attachment_id, key, value)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

pub(super) fn apply_builtin_effect_mode_action(
    service: &TimelineEditorService,
    attachment_id: AttachmentId,
    key: &str,
    automation: Option<&AutomationTrack>,
    value: PropertyValue,
    local_time: MediaTime,
    action: PropertyModeAction,
) -> Result<(), String> {
    match action {
        PropertyModeAction::SetMode(PropertyAuthoringMode::Constant) => service
            .set_builtin_effect_parameter_constant(attachment_id, key, value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Keyframe) => service
            .upsert_builtin_effect_parameter_keyframe(attachment_id, key, local_time, value, None)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Expression) => {
            Err("Effect parameter expressions belong inside a Node Effect".to_string())
        }
        PropertyModeAction::ToggleKeyframe => {
            if let Some(keyframe_id) = keyframe_at(automation, local_time) {
                if automation.is_some_and(|track| track.keyframes.len() == 1) {
                    service
                        .set_builtin_effect_parameter_constant(attachment_id, key, value)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                } else {
                    service
                        .remove_builtin_effect_parameter_keyframe(attachment_id, key, keyframe_id)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                }
            } else {
                service
                    .upsert_builtin_effect_parameter_keyframe(
                        attachment_id,
                        key,
                        local_time,
                        value,
                        None,
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        }
    }
}

fn keyframe_at(
    track: Option<&AutomationTrack>,
    local_time: MediaTime,
) -> Option<library::model::property::KeyframeId> {
    let seconds = local_time.to_seconds_f64();
    track.and_then(|track| {
        track
            .keyframes
            .iter()
            .find(|keyframe| (keyframe.time.to_seconds_f64() - seconds).abs() < 0.001)
            .map(|keyframe| keyframe.id)
    })
}

pub(super) fn property_label(ui: &mut Ui, control_id: &str, text: &str) -> Response {
    let desired_size = egui::vec2(PROPERTY_LABEL_WIDTH, ui.spacing().interact_size.y.max(20.0));
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());
    let font_id: FontId = TextStyle::Body.resolve(ui.style());
    let color = ui.visuals().text_color();
    let clip_rect = rect.intersect(ui.clip_rect());
    ui.painter().with_clip_rect(clip_rect).text(
        property_label_anchor(rect),
        Align2::LEFT_CENTER,
        text,
        font_id,
        color,
    );
    let response = response.on_hover_text(text);
    crate::qa::register_component_with_metadata(
        format!("inspector.property_label:{control_id}"),
        "inspector_property_label",
        rect,
        true,
        Some(serde_json::json!({
            "control_id": control_id,
            "horizontal_alignment": "left",
            "text_anchor": "left_center",
            "text_anchor_x": rect.left(),
        })),
    );
    response
}

fn property_label_anchor(rect: egui::Rect) -> egui::Pos2 {
    rect.left_center()
}

#[cfg(test)]
thread_local! {
    static PROPERTY_ROW_TEST_RECTS: std::cell::RefCell<std::collections::HashMap<String, egui::Rect>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

#[cfg(test)]
fn capture_test_rect(id: &str, rect: egui::Rect) {
    PROPERTY_ROW_TEST_RECTS.with(|rects| {
        rects.borrow_mut().insert(id.to_string(), rect);
    });
}

#[cfg(test)]
fn test_rect(id: &str) -> Option<egui::Rect> {
    PROPERTY_ROW_TEST_RECTS.with(|rects| rects.borrow().get(id).copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{SourceRef, TimelineInterval};
    use library::model::frame::color::Color;
    use ordered_float::OrderedFloat;
    use std::io;

    #[test]
    fn property_row_orders_left_label_then_mode_then_value() -> Result<(), io::Error> {
        PROPERTY_ROW_TEST_RECTS.with(|rects| rects.borrow_mut().clear());
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 300.0));
        let mut value = PropertyValue::Number(OrderedFloat(1.0));
        let _frame_output = context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let _row_result = property_row(
                        ui,
                        &mut value,
                        PropertyRowSpec {
                            control_id: "test:opacity",
                            label: "Opacity",
                            definition: None,
                            suffix: "",
                            speed: 0.1,
                            mode_state: PropertyModeState::constant(0.0),
                            allow_expression: true,
                        },
                    );
                });
            },
        );

        let label = test_rect("label").ok_or_else(|| io::Error::other("missing label"))?;
        let mode = test_rect("mode").ok_or_else(|| io::Error::other("missing mode"))?;
        let value = test_rect("value").ok_or_else(|| io::Error::other("missing value"))?;
        assert_eq!(label.width(), PROPERTY_LABEL_WIDTH);
        assert_eq!(property_label_anchor(label).x, label.left());
        assert!(label.right() <= mode.left());
        assert!(mode.right() <= value.left());
        Ok(())
    }

    #[test]
    fn authored_mode_switches_expression_to_keyframe_and_keyframe_to_constant() {
        let service = TimelineEditorService::create_default("Property modes").expect("service");
        let project = service.snapshot().expect("project");
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        drop(project);
        let (item_id, _) = service
            .add_item(
                track_id,
                "Solid".to_string(),
                SourceRef::Solid {
                    color: Color::white(),
                },
                TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).expect("duration"))
                    .expect("interval"),
                0,
            )
            .expect("item");
        let owner = AuthoringPropertyOwner::Item(item_id);
        service
            .set_authored_property(
                owner,
                "opacity".to_string(),
                Property::expression(
                    "value * 2".to_string(),
                    PropertyValue::Number(OrderedFloat(0.5)),
                ),
            )
            .expect("expression");
        let expression = service.snapshot().expect("expression snapshot");
        let property = expression.items[&item_id]
            .authored_properties
            .get("opacity")
            .expect("property");

        apply_authored_mode_action(
            &service,
            owner,
            "opacity",
            Some(property),
            PropertyValue::Number(OrderedFloat(0.75)),
            MediaTime::new(1, 1).expect("time"),
            PropertyModeAction::SetMode(PropertyAuthoringMode::Keyframe),
        )
        .expect("Keyframe mode");
        let keyed = service.snapshot().expect("keyed");
        let property = keyed.items[&item_id]
            .authored_properties
            .get("opacity")
            .expect("property");
        assert_eq!(property.evaluator, "keyframe");
        assert!(property.has_keyframe_at(1.0, 0.001));
        let keyed_revision = service.revision().expect("revision");

        apply_authored_mode_action(
            &service,
            owner,
            "opacity",
            Some(property),
            PropertyValue::Number(OrderedFloat(0.75)),
            MediaTime::new(1, 1).expect("time"),
            PropertyModeAction::SetMode(PropertyAuthoringMode::Constant),
        )
        .expect("Constant mode");
        assert_eq!(
            service.revision().expect("revision").get(),
            keyed_revision.get() + 1
        );
        let constant = service.snapshot().expect("constant");
        let property = constant.items[&item_id]
            .authored_properties
            .get("opacity")
            .expect("property");
        assert_eq!(property.evaluator, "constant");
        assert_eq!(
            property.value(),
            Some(&PropertyValue::Number(OrderedFloat(0.75)))
        );
    }
}
