//! Shared Inspector presentation for authored values and Timeline automation.
//!
//! The model-specific panels decide how an action is committed. This module
//! owns the single compact row layout so authored properties, Node Clip
//! parameters, and Effect parameters cannot drift into different controls.

use egui::{Align2, FontId, Response, Sense, TextStyle, Ui};
use library::editor::{AuthoringPropertyOwner, TimelineEditorService, TransitionAutomationOwner};
use library::model::authoring::{
    AttachmentId, AutomationTrack, MediaTime, ProjectPalette, PublishedParameterId,
};
use library::model::property::{KeyframeId, Property, PropertyDefinition, PropertyValue};

use crate::state::authoring::{AuthoringUiState, TransientPropertyEdit};
use crate::ui::module_parameter_editor::{
    edit_module_parameter, keyframe_at, ModuleParameterContext, ModuleParameterEditorOutcome,
    ModuleParameterRowInteraction,
};
use library::model::authoring::PublishedParameter;

use crate::ui::widgets::property_mode::{
    property_for_mode, property_mode_control_for_state, PropertyAuthoringMode, PropertyModeAction,
    PropertyModeState,
};
use crate::ui::widgets::property_value_editor::{property_value_editor, PropertyValueEditorSpec};

const PROPERTY_LABEL_WIDTH: f32 = 112.0;

pub(super) struct PropertyRowSpec<'a> {
    pub(super) control_id: &'a str,
    pub(super) label: &'a str,
    pub(super) definition: Option<&'a PropertyDefinition>,
    pub(super) suffix: &'a str,
    pub(super) speed: f64,
    pub(super) mode_state: PropertyModeState,
    pub(super) allow_keyframe: bool,
    pub(super) keyframe_disabled_reason: Option<&'a str>,
    pub(super) allow_expression: bool,
    pub(super) pending_keyframe: Option<PendingKeyframeMetadata>,
}

#[derive(Clone, Copy)]
pub(crate) struct PendingKeyframeMetadata {
    pub(crate) insertion_id: KeyframeId,
    pub(crate) local_time: MediaTime,
}

pub(super) struct PropertyRowResult {
    pub(super) response: Response,
    pub(super) changed: bool,
    pub(super) finished: bool,
    pub(super) mode_action: Option<PropertyModeAction>,
}

/// Draws the canonical compact Inspector row in production order:
/// left-aligned label, authoring-state icon, then typed value.
pub(super) fn property_row(
    ui: &mut Ui,
    value: &mut PropertyValue,
    palette: &ProjectPalette,
    spec: PropertyRowSpec<'_>,
) -> PropertyRowResult {
    let mut changed = false;
    let mut finished = false;
    let mut mode_action = None;
    let row = ui.horizontal(|ui| {
        let _label = property_label(ui, spec.control_id, spec.label);
        let (row_mode_action, _mode) = property_mode_control_for_state(
            ui,
            &format!("inspector.property_mode:{}", spec.control_id),
            spec.mode_state,
            spec.allow_keyframe,
            spec.keyframe_disabled_reason,
            spec.allow_expression,
        );
        let value_edit = property_value_editor(
            ui,
            egui::Id::new(("inspector.property", spec.control_id)),
            &format!("inspector.property:{}", spec.control_id),
            value,
            PropertyValueEditorSpec {
                definition: spec.definition,
                fallback_suffix: spec.suffix,
                fallback_speed: spec.speed,
                palette,
            },
        );
        changed = value_edit.changed;
        finished = value_edit.finished;
        mode_action = row_mode_action;

        #[cfg(test)]
        {
            capture_test_rect("label", _label.rect);
            capture_test_rect("mode", _mode.rect);
            capture_test_rect("value", value_edit.response.rect);
        }
    });
    let mut metadata = serde_json::json!({
        "control_id": spec.control_id,
        "column_order": ["label", "property_mode", "value"],
        "allow_keyframe": spec.allow_keyframe,
        "keyframe_disabled_reason": spec.keyframe_disabled_reason,
    });
    if let Some(pending) = spec.pending_keyframe {
        metadata["pending_keyframe_insertion_id"] =
            serde_json::Value::String(pending.insertion_id.to_string());
        metadata["pending_keyframe_time"] = serde_json::json!(pending.local_time.to_seconds_f64());
    }
    crate::qa::register_component_with_metadata(
        format!("inspector.property_row:{}", spec.control_id),
        "inspector_property_row",
        row.response.rect,
        row.response.enabled(),
        Some(metadata),
    );
    PropertyRowResult {
        response: row.response,
        changed,
        finished,
        mode_action,
    }
}

pub(super) fn authored_transient_edit(
    source_revision: Option<library::model::authoring::ProjectRevision>,
    owner: AuthoringPropertyOwner,
    key: &str,
    current: Option<&Property>,
    local_time: MediaTime,
    value: PropertyValue,
) -> Option<TransientPropertyEdit> {
    let source_revision = source_revision?;
    let target = match current.map(|property| property.evaluator.as_str()) {
        Some("keyframe") => library::editor::AuthoringPropertyValueTarget::Keyframe {
            local_time,
            insertion_id: KeyframeId::new(),
        },
        Some("expression") => return None,
        _ => library::editor::AuthoringPropertyValueTarget::Constant,
    };
    Some(TransientPropertyEdit::authored(
        source_revision,
        owner,
        library::editor::AuthoringPropertyValueUpdate {
            key: key.to_string(),
            value,
            target,
        },
    ))
}

pub(crate) fn update_transient_edit(
    slot: &mut Option<TransientPropertyEdit>,
    next: TransientPropertyEdit,
) {
    if let Some(current) = slot {
        current.update(next);
    } else {
        *slot = Some(next);
    }
}

pub(super) fn take_matching_authored_edit(
    slot: &mut Option<TransientPropertyEdit>,
    owner: AuthoringPropertyOwner,
    key: &str,
) -> Option<TransientPropertyEdit> {
    slot.as_ref()
        .is_some_and(|edit| edit.matches(owner, key))
        .then(|| slot.take())
        .flatten()
}

pub(super) fn pending_authored_keyframe(
    edit: Option<&TransientPropertyEdit>,
    owner: AuthoringPropertyOwner,
    key: &str,
) -> Option<PendingKeyframeMetadata> {
    let edit = edit.filter(|edit| edit.matches(owner, key))?;
    let (insertion_id, local_time) = edit.pending_keyframe()?;
    Some(PendingKeyframeMetadata {
        insertion_id,
        local_time,
    })
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
    palette: &ProjectPalette,
) -> bool {
    property_value_editor(
        ui,
        egui::Id::new(("inspector.property", control_id)),
        &format!("inspector.property:{control_id}"),
        value,
        PropertyValueEditorSpec {
            definition,
            fallback_suffix: suffix,
            fallback_speed: speed,
            palette,
        },
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

pub(super) fn commit_transition_parameter_value(
    service: &TimelineEditorService,
    owner: &TransitionAutomationOwner,
    parameter_id: PublishedParameterId,
    automation: Option<&AutomationTrack>,
    value: PropertyValue,
    local_time: MediaTime,
) -> Result<(), String> {
    if automation.is_some() {
        service
            .upsert_transition_parameter_keyframe(owner, parameter_id, local_time, value, None)
            .map(|_| ())
            .map_err(|error| error.to_string())
    } else {
        service
            .set_transition_parameter_constant(owner, parameter_id, value)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

pub(super) fn apply_transition_parameter_mode_action(
    service: &TimelineEditorService,
    owner: &TransitionAutomationOwner,
    parameter_id: PublishedParameterId,
    automation: Option<&AutomationTrack>,
    value: PropertyValue,
    local_time: MediaTime,
    action: PropertyModeAction,
) -> Result<(), String> {
    match action {
        PropertyModeAction::SetMode(PropertyAuthoringMode::Constant) => service
            .set_transition_parameter_constant(owner, parameter_id, value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Keyframe) => service
            .upsert_transition_parameter_keyframe(owner, parameter_id, local_time, value, None)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Expression) => {
            Err("Transition parameter expressions belong inside the Node Module".to_string())
        }
        PropertyModeAction::ToggleKeyframe => {
            if let Some(keyframe_id) = keyframe_at(automation, local_time) {
                if automation.is_some_and(|track| track.keyframes.len() == 1) {
                    service
                        .set_transition_parameter_constant(owner, parameter_id, value)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                } else {
                    service
                        .remove_transition_parameter_keyframe(owner, parameter_id, keyframe_id)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                }
            } else {
                service
                    .upsert_transition_parameter_keyframe(
                        owner,
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

pub(crate) fn property_label(ui: &mut Ui, control_id: &str, text: &str) -> Response {
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

pub(super) fn published_parameter_row(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    context: &ModuleParameterContext<'_>,
    parameter: &PublishedParameter,
) -> egui::Response {
    let local_time = crate::ui::module_parameter_editor::parameter_local_time(context, state);
    let automation = context.invocation.automation_tracks.get(&parameter.id);
    let outcome = edit_module_parameter(
        &mut state.inspector,
        context,
        parameter,
        local_time,
        |row| {
            let result = property_row(
                ui,
                row.value,
                &context.project.palette,
                PropertyRowSpec {
                    control_id: &format!(
                        "module_instance:{}:{}",
                        context.instance.id, parameter.id
                    ),
                    label: &parameter.name,
                    definition: row.definition,
                    suffix: "",
                    speed: 0.1,
                    mode_state: row.mode_state,
                    allow_keyframe: row.allow_keyframe,
                    keyframe_disabled_reason: row.keyframe_disabled_reason,
                    allow_expression: false,
                    pending_keyframe: row.pending_keyframe,
                },
            );
            let mut reset_to_default = false;
            result.response.context_menu(|ui| {
                let reset = ui
                    .add_enabled(
                        context
                            .instance
                            .parameter_overrides
                            .contains_key(&parameter.id),
                        egui::Button::new("Reset base to Module default"),
                    )
                    .on_hover_text("Keep Timeline animation; reset only the base value");
                crate::qa::register_component_with_metadata(
                    format!(
                        "inspector.module_parameter.reset:{}:{}",
                        context.instance.id, parameter.id
                    ),
                    "module_parameter_reset",
                    reset.rect,
                    reset.enabled(),
                    None,
                );
                if reset.clicked() {
                    reset_to_default = true;
                    ui.close();
                }
            });
            ModuleParameterRowInteraction {
                response: result.response,
                changed: result.changed,
                finished: result.finished,
                mode_action: result.mode_action,
                reset_to_default,
            }
        },
    );
    let ModuleParameterEditorOutcome {
        response,
        mode_action,
        error,
    } = outcome;
    if let Some(error) = error {
        state.error = Some(error);
    }
    if let Some(action) = mode_action {
        state.status = format!("{}: {}", parameter.name, super::mode_action_label(action));
    }
    super::value_provenance(
        ui,
        automation.is_some(),
        context
            .instance
            .parameter_overrides
            .contains_key(&parameter.id),
    );
    response
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
    use library::model::authoring::{SourceRef, TimelineInterval, TimelineItemId};
    use library::model::frame::color::Color;
    use ordered_float::OrderedFloat;
    use std::io;

    #[test]
    fn property_row_orders_left_label_then_mode_then_value() -> Result<(), io::Error> {
        PROPERTY_ROW_TEST_RECTS.with(|rects| rects.borrow_mut().clear());
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 300.0));
        let mut value = PropertyValue::Number(OrderedFloat(1.0));
        let palette = ProjectPalette::default();
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
                        &palette,
                        PropertyRowSpec {
                            control_id: "test:opacity",
                            label: "Opacity",
                            definition: None,
                            suffix: "",
                            speed: 0.1,
                            mode_state: PropertyModeState::constant(0.0),
                            allow_keyframe: true,
                            keyframe_disabled_reason: None,
                            allow_expression: true,
                            pending_keyframe: None,
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
    fn repeated_inspector_updates_keep_one_pending_keyframe_identity() {
        let item_id = TimelineItemId::new();
        let owner = AuthoringPropertyOwner::Item(item_id);
        let property = Property {
            evaluator: "keyframe".to_string(),
            properties: Default::default(),
        };
        let local_time = MediaTime::new(3, 2).expect("time");
        let mut slot = authored_transient_edit(
            Some(library::model::authoring::ProjectRevision::initial()),
            owner,
            "position",
            Some(&property),
            local_time,
            PropertyValue::from(10.0),
        );
        let first = pending_authored_keyframe(slot.as_ref(), owner, "position")
            .expect("first pending keyframe");

        update_transient_edit(
            &mut slot,
            authored_transient_edit(
                Some(library::model::authoring::ProjectRevision::initial()),
                owner,
                "position",
                Some(&property),
                local_time,
                PropertyValue::from(25.0),
            )
            .expect("next edit"),
        );
        let updated = pending_authored_keyframe(slot.as_ref(), owner, "position")
            .expect("updated pending keyframe");

        assert_eq!(updated.insertion_id, first.insertion_id);
        assert_eq!(updated.local_time, local_time);
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
