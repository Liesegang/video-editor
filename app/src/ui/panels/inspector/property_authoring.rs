use egui::{RichText, Ui};
use egui_phosphor::regular as icons;
use library::model::property::{Property, PropertyDefinition, PropertyValue};

pub(crate) use crate::ui::widgets::property_mode::PropertyAuthoringMode;
use crate::ui::widgets::property_mode::{property_mode_control, PropertyModeAction};

#[derive(Clone, Debug, PartialEq)]
pub enum PropertyAction {
    Update(String, PropertyValue),
    /// Several fields authored by one logical control and committed by the
    /// model only after every value has validated.
    UpdateGroup(Vec<(String, PropertyValue)>),
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
            ui.label(RichText::new(icons::WARNING).color(ui.visuals().warn_fg_color))
                .on_hover_text(format!("Missing authored property '{}'", definition.name()));
        }

        ui.label(definition.label());
        let qa_id = format!("inspector.property_mode.{qa_scope}:{}", definition.name());
        let allow_expression = definition.ui_type().supports_expression();
        match property_mode_control(ui, &qa_id, property, current_time, allow_expression).0 {
            Some(PropertyModeAction::SetMode(mode)) => {
                actions.push(PropertyAction::SetMode(
                    definition.name().to_string(),
                    mode,
                    authored_value.clone(),
                ));
            }
            Some(PropertyModeAction::ToggleKeyframe) => {
                actions.push(PropertyAction::ToggleKeyframe(
                    definition.name().to_string(),
                    authored_value.clone(),
                ));
            }
            None => {}
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
    use crate::ui::widgets::property_mode::{
        property_mode_test_rect, reset_property_mode_test_rects,
    };
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
        reset_property_mode_test_rects();
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
        let mode_rect = property_mode_test_rect("mode")
            .ok_or_else(|| io::Error::other("Property mode icon button was not rendered"))?;
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
        let expression_rect = property_mode_test_rect("option.Expression")
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

    #[test]
    fn structured_property_menu_does_not_offer_unsupported_expression() -> TestResult {
        reset_property_mode_test_rects();
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 500.0));
        let color = library::model::property::ColorValue::new(
            library::model::property::ColorSpaceRef::srgb(),
            [0.5, 0.25, 1.0, 1.0],
        )?;
        let authored_value = PropertyValue::ColorValue(color.clone());
        let property = Property::constant(authored_value.clone());
        let definition = PropertyDefinition::new(
            "color",
            PropertyUiType::ColorValue,
            "Color",
            PropertyValue::ColorValue(color),
        );
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
                        let _ = render_property_authoring(
                            ui,
                            &definition,
                            Some(&property),
                            &authored_value,
                            0.0,
                            "node:structured",
                            false,
                        );
                    });
                },
            );
            frame += 1;
            drop(output);
        };

        render(Vec::new());
        render(Vec::new());
        let mode_rect = property_mode_test_rect("mode")
            .ok_or_else(|| io::Error::other("Property mode icon button was not rendered"))?;
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
        assert!(property_mode_test_rect("option.Constant").is_some());
        assert!(property_mode_test_rect("option.Keyframe").is_some());
        assert!(property_mode_test_rect("option.Expression").is_none());
        Ok(())
    }
}
