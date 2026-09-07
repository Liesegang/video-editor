//! Shared Inspector presentation for authored values and Timeline automation.
//!
//! The model-specific panels decide how an action is committed. This module
//! owns the single compact row layout so authored properties, Node Clip
//! parameters, and Effect parameters cannot drift into different controls.

use egui::{Align2, FontId, Response, Sense, TextStyle, Ui};
use library::editor::{AuthoringPropertyOwner, TimelineEditorService};
use library::model::authoring::{AttachmentId, AutomationTrack, MediaTime, ProjectPalette};
use library::model::property::{KeyframeId, Property, PropertyDefinition, PropertyValue};

use crate::state::authoring::{AuthoringUiState, TransientPropertyEdit};
use crate::ui::module_parameter_editor::{
    edit_module_parameter, keyframe_at, ModuleParameterContext, ModuleParameterEditorOutcome,
    ModuleParameterRowInteraction,
};
use library::model::authoring::PublishedParameter;

use crate::ui::widgets::image_collection_editor::ImageCollectionEditorContext;
use crate::ui::widgets::property_mode::{
    property_for_mode, property_mode_control_for_state, PropertyAuthoringMode, PropertyModeAction,
    PropertyModeState,
};
use crate::ui::widgets::property_value_editor::{
    property_value_context_menu, property_value_editor, PropertyValueEditorSpec,
};

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
    pub(super) image_collection: Option<ImageCollectionEditorContext<'a>>,
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
                image_collection: spec.image_collection,
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
        value_edit.response
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
        response: row.inner,
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
            image_collection: None,
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
    project_snapshot: Option<&std::sync::Arc<library::model::authoring::AuthoringProject>>,
    media_previews: Option<&mut crate::ui::media_preview::AuthoringMediaPreviewService>,
) -> egui::Response {
    let local_time = crate::ui::module_parameter_editor::parameter_local_time(context, state);
    let control_id = match &context.owner {
        library::editor::ModuleParameterOwner::Transition(owner) => {
            let transition_id = match owner {
                library::editor::TransitionAutomationOwner::Definition(transition_id)
                | library::editor::TransitionAutomationOwner::Instance { transition_id, .. } => {
                    transition_id
                }
            };
            format!(
                "transition:{transition_id}:module_parameter:{}",
                parameter.id
            )
        }
        library::editor::ModuleParameterOwner::Invocation(_) => {
            format!("module_instance:{}:{}", context.instance.id, parameter.id)
        }
    };
    let mut has_automation = false;
    let mut has_resettable_override = false;
    let image_collection = project_snapshot
        .zip(media_previews)
        .map(|(project, media_previews)| ImageCollectionEditorContext {
            project,
            media_previews,
            library_drag: &mut state.library_drag,
        });
    let outcome = edit_module_parameter(
        &mut state.inspector,
        context,
        parameter,
        local_time,
        |row| {
            has_automation = row.has_automation;
            has_resettable_override = row.has_resettable_override;
            let result = property_row(
                ui,
                row.value,
                &context.project.palette,
                PropertyRowSpec {
                    control_id: &control_id,
                    label: &parameter.name,
                    definition: row.definition,
                    suffix: "",
                    speed: 0.1,
                    mode_state: row.mode_state,
                    allow_keyframe: row.allow_keyframe,
                    keyframe_disabled_reason: row.keyframe_disabled_reason,
                    allow_expression: false,
                    pending_keyframe: row.pending_keyframe,
                    image_collection,
                },
            );
            let mut reset_to_default = false;
            property_value_context_menu(&result.response, |ui| {
                let reset = ui
                    .add_enabled(
                        row.has_resettable_override,
                        egui::Button::new(row.reset_label),
                    )
                    .on_hover_text(row.reset_label);
                let owner = crate::ui::automation_lanes::module_parameter_owner(
                    context.project,
                    &context.owner,
                );
                crate::qa::register_component_with_metadata(
                    format!(
                        "inspector.module_parameter.reset:{}:{}",
                        context.instance.id, parameter.id
                    ),
                    "module_parameter_reset",
                    reset.rect,
                    reset.enabled(),
                    Some(serde_json::json!({
                        "owner": owner.as_ref().map(crate::ui::automation_lanes::owner_metadata),
                        "parameter_id": parameter.id,
                        "label": row.reset_label,
                    })),
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
    super::value_provenance(ui, has_automation, has_resettable_override);
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

    fn secondary_pointer(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn render_context_menu_row(
        context: &egui::Context,
        events: Vec<egui::Event>,
        frame: usize,
        value: &mut PropertyValue,
        menu_opened: &mut bool,
    ) -> egui::Rect {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 300.0));
        let palette = ProjectPalette::default();
        let mut response_rect = None;
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let result = property_row(
                        ui,
                        value,
                        &palette,
                        PropertyRowSpec {
                            control_id: "test:reset",
                            label: "Resettable",
                            definition: None,
                            suffix: "",
                            speed: 0.1,
                            mode_state: PropertyModeState::constant(0.0),
                            allow_keyframe: true,
                            keyframe_disabled_reason: None,
                            allow_expression: false,
                            pending_keyframe: None,
                            image_collection: None,
                        },
                    );
                    response_rect = Some(result.response.rect);
                    property_value_context_menu(&result.response, |ui| {
                        *menu_opened = true;
                        ui.label("Reset to default");
                    });
                });
            },
        ));
        response_rect.expect("property value response")
    }

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
                            image_collection: None,
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
    fn property_row_secondary_click_belongs_to_the_value_control() {
        PROPERTY_ROW_TEST_RECTS.with(|rects| rects.borrow_mut().clear());
        let context = egui::Context::default();
        let mut value = PropertyValue::Number(OrderedFloat(1.0));
        let mut menu_opened = false;
        let response =
            render_context_menu_row(&context, Vec::new(), 0, &mut value, &mut menu_opened);
        let value_rect = test_rect("value").expect("value rect");
        let label_rect = test_rect("label").expect("label rect");
        assert_eq!(response, value_rect);
        assert!(!response.intersects(label_rect));
        assert!(!menu_opened);

        let position = response.center();
        render_context_menu_row(
            &context,
            vec![
                egui::Event::PointerMoved(position),
                secondary_pointer(position, true),
            ],
            1,
            &mut value,
            &mut menu_opened,
        );
        render_context_menu_row(
            &context,
            vec![secondary_pointer(position, false)],
            2,
            &mut value,
            &mut menu_opened,
        );
        render_context_menu_row(&context, Vec::new(), 3, &mut value, &mut menu_opened);
        assert!(menu_opened, "secondary-clicking the value must open Reset");
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
