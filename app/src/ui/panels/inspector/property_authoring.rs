use egui::{RichText, Ui};
use egui_phosphor::fill::DIAMOND as ICON_DIAMOND_FILLED;
use egui_phosphor::regular::DIAMOND as ICON_DIAMOND;
use library::model::property::{Property, PropertyDefinition, PropertyValue};

/// The three authoring modes exposed by every editable Inspector property.
///
/// This is deliberately narrower than the persisted evaluator string: a
/// missing third-party evaluator remains visible as unsupported until the user
/// explicitly chooses one of these modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyAuthoringMode {
    Constant,
    Keyframe,
    Expression,
}

impl PropertyAuthoringMode {
    pub fn from_evaluator(evaluator: &str) -> Option<Self> {
        match evaluator {
            "constant" => Some(Self::Constant),
            "keyframe" => Some(Self::Keyframe),
            "expression" => Some(Self::Expression),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Constant => "Constant",
            Self::Keyframe => "Keyframe",
            Self::Expression => "Expression",
        }
    }

    pub const fn qa_key(self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::Keyframe => "keyframe",
            Self::Expression => "expression",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropertyAction {
    Update(String, PropertyValue),
    Commit,
    ToggleKeyframe(String, PropertyValue),
    SetAttribute(String, String, PropertyValue),
    SetMode(String, PropertyAuthoringMode, PropertyValue),
    SetExpressionSource(String, String),
}

/// Renders the label/mode cell and, for Expression properties, the Python
/// source row. The caller renders the normal typed value widget immediately
/// afterwards; in Expression mode that widget edits the authored fallback.
pub fn render_property_authoring(
    ui: &mut Ui,
    definition: &PropertyDefinition,
    property: Option<&Property>,
    authored_value: &PropertyValue,
    current_time: f64,
    qa_scope: &str,
    in_grid: bool,
) -> Vec<PropertyAction> {
    let mut actions = Vec::new();
    let current_mode = property
        .and_then(|property| PropertyAuthoringMode::from_evaluator(property.evaluator.as_str()));

    ui.horizontal(|ui| {
        if property.is_none() {
            ui.label(RichText::new("⚠").color(ui.visuals().warn_fg_color))
                .on_hover_text(format!("Missing authored property '{}'", definition.name()));
        }

        ui.label(definition.label());

        let selected_text = match (current_mode, property) {
            (Some(mode), _) => mode.label().to_string(),
            (None, Some(property)) => format!("Unsupported · {}", property.evaluator),
            (None, None) => "Missing".to_string(),
        };
        let mut selected_mode = current_mode;
        let combo =
            egui::ComboBox::from_id_salt(("inspector_property_mode", qa_scope, definition.name()))
                .selected_text(selected_text)
                .width(92.0)
                .show_ui(ui, |ui| {
                    for mode in [
                        PropertyAuthoringMode::Constant,
                        PropertyAuthoringMode::Keyframe,
                        PropertyAuthoringMode::Expression,
                    ] {
                        let response =
                            ui.selectable_value(&mut selected_mode, Some(mode), mode.label());
                        crate::qa::register_component_with_metadata(
                            format!(
                                "inspector.property_mode.option.{qa_scope}:{}:{}",
                                definition.name(),
                                mode.qa_key(),
                            ),
                            "inspector_property_mode_option",
                            response.rect,
                            response.enabled(),
                            Some(serde_json::json!({
                                "scope": qa_scope,
                                "property": definition.name(),
                                "mode": mode.label(),
                                "mode_key": mode.qa_key(),
                                "selected": current_mode == Some(mode),
                            })),
                        );
                        #[cfg(test)]
                        AUTHORING_TEST_RECTS.with(|rects| {
                            rects
                                .borrow_mut()
                                .insert(format!("option.{}", mode.label()), response.rect);
                        });
                    }
                });
        #[cfg(test)]
        AUTHORING_TEST_RECTS.with(|rects| {
            rects
                .borrow_mut()
                .insert("mode".to_string(), combo.response.rect);
        });
        crate::qa::register_component_with_metadata(
            format!("inspector.property_mode.{qa_scope}:{}", definition.name()),
            "inspector_property_mode",
            combo.response.rect,
            combo.response.enabled(),
            Some(serde_json::json!({
                "scope": qa_scope,
                "property": definition.name(),
                "mode": current_mode.map(PropertyAuthoringMode::label),
                "evaluator": property.map(|property| property.evaluator.as_str()),
            })),
        );
        if selected_mode != current_mode {
            if let Some(mode) = selected_mode {
                actions.push(PropertyAction::SetMode(
                    definition.name().to_string(),
                    mode,
                    authored_value.clone(),
                ));
            }
        }

        if current_mode == Some(PropertyAuthoringMode::Keyframe) {
            let is_on_key = property.is_some_and(|property| {
                property
                    .keyframes()
                    .iter()
                    .any(|key| (key.time.into_inner() - current_time).abs() < 0.001)
            });
            let (icon, color) = if is_on_key {
                (
                    ICON_DIAMOND_FILLED,
                    ui.visuals().widgets.active.text_color(),
                )
            } else {
                (ICON_DIAMOND, ui.visuals().text_color())
            };
            let button = ui
                .add(egui::Button::new(RichText::new(icon).color(color)).frame(false))
                .on_hover_text("Toggle keyframe at current time");
            crate::qa::register_component_with_metadata(
                format!("inspector.keyframe.{qa_scope}:{}", definition.name()),
                "keyframe_control",
                button.rect,
                button.enabled(),
                Some(serde_json::json!({
                    "scope": qa_scope,
                    "property": definition.name(),
                    "is_keyframed": true,
                    "is_on_key": is_on_key,
                    "current_time": current_time,
                })),
            );
            if button.clicked() {
                actions.push(PropertyAction::ToggleKeyframe(
                    definition.name().to_string(),
                    authored_value.clone(),
                ));
            }
        }
    });

    if current_mode == Some(PropertyAuthoringMode::Expression) {
        if !in_grid {
            ui.label(RichText::new("Python Expression").small().strong());
        }
        render_expression_source(ui, definition, property, qa_scope, &mut actions);
        if in_grid {
            ui.end_row();
        }
        ui.label(RichText::new("Authored fallback").small().weak());
    }

    actions
}

fn render_expression_source(
    ui: &mut Ui,
    definition: &PropertyDefinition,
    property: Option<&Property>,
    qa_scope: &str,
    actions: &mut Vec<PropertyAction>,
) {
    let mut source = property
        .and_then(Property::expression_text)
        .unwrap_or_default()
        .to_string();
    ui.vertical(|ui| {
        ui.label(
            RichText::new("value · time/t · frame · fps · resolution · math/helpers")
                .small()
                .weak(),
        );
        let response = ui.add(
            egui::TextEdit::multiline(&mut source)
                .id_salt(("expression_source", qa_scope, definition.name()))
                .code_editor()
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Python expression, e.g. value + sin(time) * 10"),
        );
        crate::qa::register_component_with_metadata(
            format!(
                "inspector.expression_source.{qa_scope}:{}",
                definition.name()
            ),
            "inspector_expression_source",
            response.rect,
            response.enabled(),
            Some(serde_json::json!({
                "scope": qa_scope,
                "property": definition.name(),
                "source": property.and_then(Property::expression_text),
            })),
        );
        if response.changed() {
            actions.push(PropertyAction::SetExpressionSource(
                definition.name().to_string(),
                source,
            ));
        }
        if response.lost_focus() {
            actions.push(PropertyAction::Commit);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::property::PropertyUiType;
    use ordered_float::OrderedFloat;
    use std::error::Error;
    use std::io;

    type TestResult = Result<(), Box<dyn Error>>;

    #[test]
    fn mode_mapping_never_disguises_a_missing_evaluator_as_constant() {
        assert_eq!(
            PropertyAuthoringMode::from_evaluator("constant"),
            Some(PropertyAuthoringMode::Constant)
        );
        assert_eq!(
            PropertyAuthoringMode::from_evaluator("expression"),
            Some(PropertyAuthoringMode::Expression)
        );
        assert_eq!(PropertyAuthoringMode::Expression.qa_key(), "expression");
        assert_eq!(PropertyAuthoringMode::from_evaluator("not-installed"), None);
    }

    #[test]
    fn real_pointer_selection_emits_an_expression_mode_action() -> TestResult {
        AUTHORING_TEST_RECTS.with(|rects| rects.borrow_mut().clear());
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 500.0));
        let definition = PropertyDefinition::new(
            "amount",
            PropertyUiType::Float {
                min: -100.0,
                max: 100.0,
                step: 0.1,
                suffix: String::new(),
                min_hard_limit: false,
                max_hard_limit: false,
            },
            "Amount",
            PropertyValue::Number(OrderedFloat(2.0)),
        );
        let authored_value = PropertyValue::Number(OrderedFloat(2.0));
        let property = Property::constant(authored_value.clone());
        let mut actions = Vec::new();
        let mut frame = 0;
        let mut render = |events: Vec<egui::Event>| {
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        actions.extend(render_property_authoring(
                            ui,
                            &definition,
                            Some(&property),
                            &authored_value,
                            0.0,
                            "node:test",
                            false,
                        ));
                    });
                },
            );
            frame += 1;
            drop(output);
        };

        render(Vec::new());
        render(Vec::new());
        let mode_rect = AUTHORING_TEST_RECTS
            .with(|rects| rects.borrow().get("mode").copied())
            .ok_or_else(|| io::Error::other("Property mode ComboBox was not rendered"))?;
        let mode_pos = mode_rect.center();
        render(vec![
            egui::Event::PointerMoved(mode_pos),
            egui::Event::PointerButton {
                pos: mode_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        render(vec![egui::Event::PointerButton {
            pos: mode_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        render(Vec::new());
        let expression_rect = AUTHORING_TEST_RECTS
            .with(|rects| rects.borrow().get("option.Expression").copied())
            .ok_or_else(|| io::Error::other("Expression mode option was not rendered"))?;
        let expression_pos = expression_rect.center();
        render(vec![
            egui::Event::PointerMoved(expression_pos),
            egui::Event::PointerButton {
                pos: expression_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        render(vec![egui::Event::PointerButton {
            pos: expression_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);

        assert!(actions.iter().any(|action| matches!(
            action,
            PropertyAction::SetMode(
                name,
                PropertyAuthoringMode::Expression,
                PropertyValue::Number(value),
            ) if name == "amount" && *value == OrderedFloat(2.0)
        )));
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    static AUTHORING_TEST_RECTS: std::cell::RefCell<std::collections::HashMap<String, egui::Rect>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}
