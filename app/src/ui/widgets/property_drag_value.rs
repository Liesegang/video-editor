use library::model::property::{PropertyDefinition, PropertyUiType};

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
