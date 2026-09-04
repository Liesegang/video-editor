use egui::{Color32, RichText, Ui};
use egui_phosphor::{fill, regular};
use library::model::property::{Keyframe, Property, PropertyValue};

use library::animation::EasingFunction;

const KEYFRAME_TOLERANCE: f64 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PropertyAuthoringMode {
    Constant,
    Keyframe,
    Expression,
}

impl PropertyAuthoringMode {
    pub(crate) fn from_evaluator(evaluator: &str) -> Option<Self> {
        match evaluator {
            "constant" => Some(Self::Constant),
            "keyframe" => Some(Self::Keyframe),
            "expression" => Some(Self::Expression),
            _ => None,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Constant => "Constant",
            Self::Keyframe => "Keyframe",
            Self::Expression => "Expression",
        }
    }

    pub(crate) const fn qa_key(self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::Keyframe => "keyframe",
            Self::Expression => "expression",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PropertyModeAction {
    SetMode(PropertyAuthoringMode),
    ToggleKeyframe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModePresentation {
    mode: Option<PropertyAuthoringMode>,
    key_at_current_time: bool,
    icon: &'static str,
    icon_semantic: &'static str,
    tooltip: &'static str,
    color: Color32,
}

fn mode_presentation(property: Option<&Property>, current_time: f64) -> ModePresentation {
    let mode = property
        .and_then(|property| PropertyAuthoringMode::from_evaluator(property.evaluator.as_str()));
    let key_at_current_time = mode == Some(PropertyAuthoringMode::Keyframe)
        && property
            .is_some_and(|property| property.has_keyframe_at(current_time, KEYFRAME_TOLERANCE));
    match (mode, key_at_current_time) {
        (Some(PropertyAuthoringMode::Constant), _) => ModePresentation {
            mode,
            key_at_current_time: false,
            icon: regular::TIMER,
            icon_semantic: "timer_constant",
            tooltip: "Constant value · Click to change authoring mode",
            color: Color32::from_rgb(185, 190, 201),
        },
        (Some(PropertyAuthoringMode::Keyframe), false) => ModePresentation {
            mode,
            key_at_current_time: false,
            icon: regular::DIAMOND,
            icon_semantic: "diamond_outline_keyframe",
            tooltip: "Keyframe mode · No key at current time · Click for mode and key actions",
            color: Color32::from_rgb(217, 166, 85),
        },
        (Some(PropertyAuthoringMode::Keyframe), true) => ModePresentation {
            mode,
            key_at_current_time: true,
            icon: fill::DIAMOND,
            icon_semantic: "diamond_filled_keyframe",
            tooltip: "Keyframe mode · Key at current time · Click for mode and key actions",
            color: Color32::from_rgb(244, 186, 88),
        },
        (Some(PropertyAuthoringMode::Expression), _) => ModePresentation {
            mode,
            key_at_current_time: false,
            icon: regular::FUNCTION,
            icon_semantic: "function_expression",
            tooltip: "Python Expression · Click to change authoring mode",
            color: Color32::from_rgb(105, 199, 229),
        },
        (None, _) => ModePresentation {
            mode: None,
            key_at_current_time: false,
            icon: regular::QUESTION,
            icon_semantic: "question_unsupported",
            tooltip: "Missing or unsupported property evaluator · Click to choose a supported mode",
            color: Color32::from_rgb(226, 113, 113),
        },
    }
}

/// Shared icon-only authoring-mode control used by the Inspector and inline
/// Node body. Text labels live in its menu and accessibility metadata, never
/// in the compact property row itself.
pub(crate) fn property_mode_control(
    ui: &mut Ui,
    qa_id: &str,
    property: Option<&Property>,
    current_time: f64,
    allow_expression: bool,
) -> (Option<PropertyModeAction>, egui::Response) {
    let presentation = mode_presentation(property, current_time);
    let button = ui
        .push_id(qa_id, |ui| {
            ui.add_sized(
                [24.0, ui.spacing().interact_size.y],
                egui::Button::new(
                    RichText::new(presentation.icon)
                        .color(presentation.color)
                        .strong(),
                )
                .frame(false),
            )
        })
        .inner
        .on_hover_text(presentation.tooltip);
    button.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            button.enabled(),
            presentation.tooltip,
        )
    });
    crate::qa::register_component_with_metadata(
        qa_id,
        "property_mode_control",
        button.rect,
        button.enabled(),
        Some(serde_json::json!({
            "mode": presentation.mode.map(PropertyAuthoringMode::qa_key),
            "mode_label": presentation.mode.map(PropertyAuthoringMode::label),
            "key_at_current_time": presentation.key_at_current_time,
            "icon": {
                "library": "egui_phosphor",
                "semantic": presentation.icon_semantic,
            },
            "current_time": current_time,
            "evaluator": property.map(|property| property.evaluator.as_str()),
        })),
    );
    #[cfg(test)]
    capture_test_rect("mode", button.rect);

    let mut action = None;
    egui::Popup::menu(&button).show(|ui| {
        ui.set_min_width(170.0);
        for mode in [
            PropertyAuthoringMode::Constant,
            PropertyAuthoringMode::Keyframe,
            PropertyAuthoringMode::Expression,
        ]
        .into_iter()
        .filter(|mode| *mode != PropertyAuthoringMode::Expression || allow_expression)
        {
            let (icon, semantic) = match mode {
                PropertyAuthoringMode::Constant => (regular::TIMER, "timer_constant"),
                PropertyAuthoringMode::Keyframe => (regular::DIAMOND, "diamond_outline_keyframe"),
                PropertyAuthoringMode::Expression => (regular::FUNCTION, "function_expression"),
            };
            let option = ui.selectable_label(
                presentation.mode == Some(mode),
                format!("{icon}  {}", mode.label()),
            );
            let option_id = format!("{qa_id}.option:{}", mode.qa_key());
            crate::qa::register_component_with_metadata(
                option_id,
                "property_mode_option",
                option.rect,
                option.enabled(),
                Some(serde_json::json!({
                    "mode": mode.qa_key(),
                    "mode_label": mode.label(),
                    "selected": presentation.mode == Some(mode),
                    "icon": {"library": "egui_phosphor", "semantic": semantic},
                })),
            );
            #[cfg(test)]
            capture_test_rect(&format!("option.{}", mode.label()), option.rect);
            if option.clicked() && presentation.mode != Some(mode) {
                action = Some(PropertyModeAction::SetMode(mode));
                ui.close();
            }
        }

        if presentation.mode == Some(PropertyAuthoringMode::Keyframe) {
            ui.separator();
            let (icon, label, semantic) = if presentation.key_at_current_time {
                (
                    fill::DIAMOND,
                    "Remove key at current time",
                    "diamond_filled_keyframe",
                )
            } else {
                (
                    regular::DIAMOND,
                    "Add key at current time",
                    "diamond_outline_keyframe",
                )
            };
            let toggle = ui.button(format!("{icon}  {label}"));
            crate::qa::register_component_with_metadata(
                format!("{qa_id}.toggle_keyframe"),
                "property_keyframe_toggle",
                toggle.rect,
                toggle.enabled(),
                Some(serde_json::json!({
                    "action": if presentation.key_at_current_time {"remove"} else {"add"},
                    "key_at_current_time": presentation.key_at_current_time,
                    "current_time": current_time,
                    "icon": {"library": "egui_phosphor", "semantic": semantic},
                })),
            );
            #[cfg(test)]
            capture_test_rect("toggle_keyframe", toggle.rect);
            if toggle.clicked() {
                action = Some(PropertyModeAction::ToggleKeyframe);
                ui.close();
            }
        }
    });
    if action.is_some() {
        ui.ctx().request_repaint();
    }
    (action, button)
}

pub(crate) fn property_for_mode(
    current: Option<&Property>,
    mode: PropertyAuthoringMode,
    current_value: PropertyValue,
    current_time: f64,
) -> Result<Property, String> {
    if mode == PropertyAuthoringMode::Expression && !current_value.supports_expression() {
        return Err(
            "Expression evaluation is not available for canonical Color or Path values".to_string(),
        );
    }
    let authored_value = match current {
        Some(property) if property.evaluator == "expression" => property
            .value()
            .cloned()
            .ok_or_else(|| "Expression has no authored typed fallback".to_string())?,
        _ => current_value,
    };
    Ok(match mode {
        PropertyAuthoringMode::Constant => Property::constant(authored_value),
        PropertyAuthoringMode::Keyframe => Property::keyframe(vec![Keyframe::new(
            current_time,
            authored_value,
            EasingFunction::Linear,
        )]),
        PropertyAuthoringMode::Expression => {
            Property::expression("value".to_string(), authored_value)
        }
    })
}

pub(crate) fn toggled_keyframe_property(
    property: &Property,
    current_value: PropertyValue,
    current_time: f64,
) -> Option<Property> {
    if property.evaluator != "keyframe" {
        return None;
    }
    let mut replacement = property.clone();
    if let Some(keyframe_id) = replacement.keyframe_id_at(current_time, KEYFRAME_TOLERANCE) {
        replacement
            .remove_keyframe_by_id(keyframe_id)
            .then_some(replacement)
    } else {
        replacement
            .upsert_keyframe(current_time, current_value, None)
            .then_some(replacement)
    }
}

#[cfg(test)]
thread_local! {
    static PROPERTY_MODE_TEST_RECTS: std::cell::RefCell<std::collections::HashMap<String, egui::Rect>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

#[cfg(test)]
fn capture_test_rect(id: &str, rect: egui::Rect) {
    PROPERTY_MODE_TEST_RECTS.with(|rects| {
        rects.borrow_mut().insert(id.to_string(), rect);
    });
}

#[cfg(test)]
pub(crate) fn reset_property_mode_test_rects() {
    PROPERTY_MODE_TEST_RECTS.with(|rects| rects.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn property_mode_test_rect(id: &str) -> Option<egui::Rect> {
    PROPERTY_MODE_TEST_RECTS.with(|rects| rects.borrow().get(id).copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordered_float::OrderedFloat;

    fn number(value: f64) -> PropertyValue {
        PropertyValue::Number(OrderedFloat(value))
    }

    #[test]
    fn four_mode_icon_states_are_semantic_and_time_aware() {
        let constant = Property::constant(number(1.0));
        let keyframe = Property::keyframe(vec![Keyframe::new(
            2.0,
            number(3.0),
            EasingFunction::Linear,
        )]);
        let expression = Property::expression("value + time".to_string(), number(4.0));

        let constant_state = mode_presentation(Some(&constant), 2.0);
        assert_eq!(constant_state.icon_semantic, "timer_constant");
        let key_away = mode_presentation(Some(&keyframe), 1.0);
        assert_eq!(key_away.icon_semantic, "diamond_outline_keyframe");
        assert!(!key_away.key_at_current_time);
        let key_here = mode_presentation(Some(&keyframe), 2.0);
        assert_eq!(key_here.icon_semantic, "diamond_filled_keyframe");
        assert!(key_here.key_at_current_time);
        let expression_state = mode_presentation(Some(&expression), 2.0);
        assert_eq!(expression_state.icon_semantic, "function_expression");
    }

    #[test]
    fn keyframe_toggle_preserves_mode_until_the_final_key_is_removed() -> Result<(), &'static str> {
        let property = Property::keyframe(vec![
            Keyframe::new(1.0, number(1.0), EasingFunction::Linear),
            Keyframe::new(3.0, number(3.0), EasingFunction::Linear),
        ]);
        let with_middle = toggled_keyframe_property(&property, number(2.0), 2.0)
            .ok_or("could not add middle keyframe")?;
        assert_eq!(with_middle.evaluator, "keyframe");
        assert!(with_middle.has_keyframe_at(2.0, KEYFRAME_TOLERANCE));
        let without_middle = toggled_keyframe_property(&with_middle, number(2.0), 2.0)
            .ok_or("could not remove middle keyframe")?;
        assert_eq!(without_middle.evaluator, "keyframe");
        assert!(!without_middle.has_keyframe_at(2.0, KEYFRAME_TOLERANCE));
        Ok(())
    }

    #[test]
    fn expression_mode_rejects_structured_values_until_the_runtime_supports_them()
    -> Result<(), Box<dyn std::error::Error>> {
        let color = library::model::property::ColorValue::new(
            library::model::property::ColorSpaceRef::srgb(),
            [0.5, 0.25, 1.0, 1.0],
        )?;
        assert!(
            property_for_mode(
                None,
                PropertyAuthoringMode::Expression,
                PropertyValue::ColorValue(color),
                0.0,
            )
            .is_err()
        );
        let path = library::model::path::PathValue::empty(library::model::path::FillRule::NonZero);
        assert!(
            property_for_mode(
                None,
                PropertyAuthoringMode::Expression,
                PropertyValue::Path(path),
                0.0,
            )
            .is_err()
        );
        Ok(())
    }
}
