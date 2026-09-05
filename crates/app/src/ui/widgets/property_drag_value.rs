use library::model::property::{PropertyDefinition, PropertyUiType};

/// Shared numeric commit policy. Pointer capture stays owned by egui's
/// original DragValue; panels do not maintain another gesture state.
pub(crate) fn numeric_edit_finished(response: &egui::Response) -> bool {
    response.drag_stopped()
        || response.lost_focus()
        || (response.has_focus()
            && response
                .ctx
                .input(|input| input.key_pressed(egui::Key::Enter)))
}

#[derive(Clone, Debug, PartialEq)]
pub struct FloatDragValueConfig {
    pub speed: f64,
    pub suffix: String,
    pub hard_min: Option<f64>,
    pub hard_max: Option<f64>,
}

impl FloatDragValueConfig {
    pub fn from_definition(definition: &PropertyDefinition) -> Option<Self> {
        Self::from_ui_type(definition.ui_type())
    }

    pub fn from_ui_type(ui_type: &PropertyUiType) -> Option<Self> {
        let (min, max, step, suffix, min_hard_limit, max_hard_limit) = match ui_type {
            PropertyUiType::Float {
                min,
                max,
                step,
                suffix,
                min_hard_limit,
                max_hard_limit,
            }
            | PropertyUiType::Vec2 {
                min,
                max,
                step,
                suffix,
                min_hard_limit,
                max_hard_limit,
            }
            | PropertyUiType::Vec3 {
                min,
                max,
                step,
                suffix,
                min_hard_limit,
                max_hard_limit,
            }
            | PropertyUiType::Vec4 {
                min,
                max,
                step,
                suffix,
                min_hard_limit,
                max_hard_limit,
            } => (min, max, step, suffix, min_hard_limit, max_hard_limit),
            _ => return None,
        };
        Some(Self {
            speed: *step,
            suffix: suffix.clone(),
            hard_min: min_hard_limit.then_some(*min),
            hard_max: max_hard_limit.then_some(*max),
        })
    }

    /// Map underlying model units into a presentation coordinate. Inspector
    /// uses this for seconds → frames; Node Editor uses the identity map.
    #[cfg(test)]
    pub fn transformed(mut self, scale: f64, offset: f64, suffix: impl Into<String>) -> Self {
        self.speed *= scale;
        self.hard_min = self.hard_min.map(|value| value * scale + offset);
        self.hard_max = self.hard_max.map(|value| value * scale + offset);
        self.suffix = suffix.into();
        self
    }

    pub fn widget<'a>(&self, value: &'a mut f64) -> egui::DragValue<'a> {
        self.widget_with_suffix(value, &self.suffix)
    }

    pub fn widget_without_suffix<'a>(&self, value: &'a mut f64) -> egui::DragValue<'a> {
        self.widget_with_suffix(value, "")
    }

    fn widget_with_suffix<'a>(&self, value: &'a mut f64, suffix: &str) -> egui::DragValue<'a> {
        let mut widget = egui::DragValue::new(value).speed(self.speed).suffix(suffix);
        if self.hard_min.is_some() || self.hard_max.is_some() {
            widget = widget
                .range(
                    self.hard_min.unwrap_or(f64::NEG_INFINITY)
                        ..=self.hard_max.unwrap_or(f64::INFINITY),
                )
                // Loaded data is never silently rewritten merely because a
                // panel rendered. Hard bounds apply once the user edits.
                .clamp_existing_to_range(false);
        }
        widget
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegerDragValueConfig {
    pub suffix: String,
    pub hard_min: Option<i64>,
    pub hard_max: Option<i64>,
}

impl IntegerDragValueConfig {
    pub fn from_ui_type(ui_type: &PropertyUiType) -> Option<Self> {
        let PropertyUiType::Integer {
            min,
            max,
            suffix,
            min_hard_limit,
            max_hard_limit,
        } = ui_type
        else {
            return None;
        };
        Some(Self {
            suffix: suffix.clone(),
            hard_min: min_hard_limit.then_some(*min),
            hard_max: max_hard_limit.then_some(*max),
        })
    }

    pub fn widget<'a>(&self, value: &'a mut i64) -> egui::DragValue<'a> {
        let mut widget = egui::DragValue::new(value).speed(1.0).suffix(&self.suffix);
        if self.hard_min.is_some() || self.hard_max.is_some() {
            widget = widget
                .range(self.hard_min.unwrap_or(i64::MIN)..=self.hard_max.unwrap_or(i64::MAX))
                .clamp_existing_to_range(false);
        }
        widget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_scrub_finishes_once_when_released_outside_its_control() {
        let context = egui::Context::default();
        let mut value = 5.0;
        let mut control = egui::Rect::NOTHING;
        let mut finish_count = 0;
        for phase in 0..6 {
            let point = if phase < 3 {
                control.center()
            } else {
                egui::pos2(control.right() + 50.0, control.center().y)
            };
            let mut events = Vec::new();
            if phase > 0 {
                events.push(egui::Event::PointerMoved(point));
            }
            if phase == 2 || phase == 4 {
                events.push(egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: phase == 2,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 120.0),
                    )),
                    time: Some(phase as f64 * 0.1),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let response = ui.add(egui::DragValue::new(&mut value).speed(0.1));
                        control = response.rect;
                        let finished = numeric_edit_finished(&response);
                        finish_count += usize::from(finished);
                        if phase == 3 {
                            assert!(!finished, "scrub must remain transient while held outside");
                        }
                    });
                },
            ));
        }
        assert!(value > 5.0, "the real numeric widget must edit its draft");
        assert_eq!(finish_count, 1);
    }

    #[test]
    fn hard_bounds_and_unit_transform_come_only_from_property_metadata() {
        let metadata = PropertyUiType::Float {
            min: 0.0,
            max: 10.0,
            step: 0.25,
            suffix: " s".into(),
            min_hard_limit: true,
            max_hard_limit: false,
        };
        let base = FloatDragValueConfig::from_ui_type(&metadata).unwrap();
        assert_eq!(base.speed, 0.25);
        assert_eq!(base.hard_min, Some(0.0));
        assert_eq!(base.hard_max, None, "soft max is not a mutation bound");

        let frames = base.transformed(30.0, 12.0, "fr");
        assert_eq!(frames.speed, 7.5);
        assert_eq!(frames.hard_min, Some(12.0));
        assert_eq!(frames.hard_max, None);
        assert_eq!(frames.suffix, "fr");
    }

    #[test]
    fn vector_components_reuse_float_drag_metadata() {
        for metadata in [
            PropertyUiType::vec2_with_range(-10.0, 20.0, 0.5, " px", true, false),
            PropertyUiType::vec3_with_range(-10.0, 20.0, 0.5, " px", true, false),
            PropertyUiType::vec4_with_range(-10.0, 20.0, 0.5, " px", true, false),
        ] {
            let config = FloatDragValueConfig::from_ui_type(&metadata).unwrap();
            assert_eq!(config.speed, 0.5);
            assert_eq!(config.suffix, " px");
            assert_eq!(config.hard_min, Some(-10.0));
            assert_eq!(config.hard_max, None);
        }
    }
}
