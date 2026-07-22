//! Native Shape path-effect operations.
//!
//! Each component owns exactly one authored effect configuration. The
//! evaluated [`PathEffect`] is render-only data appended while a Shape value
//! travels through explicit Shape -> Shape graph wiring.

use crate::error::LibraryError;
use crate::model::frame::draw_type::{PathEffect, TrimPathUnits};
use crate::model::property::{PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue};
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::{OperationDescriptor, OperationDescriptorError, Plugin, PluginCategory};

pub const DASH_PATH_EFFECT_COMPONENT_ID: &str = "dash";
pub const CORNER_PATH_EFFECT_COMPONENT_ID: &str = "corner";
pub const DISCRETE_PATH_EFFECT_COMPONENT_ID: &str = "discrete";
pub const TRIM_PATH_EFFECT_COMPONENT_ID: &str = "trim";

const TRIM_NORMALIZED_MODE: &str = "Normalized (1 = full path)";
const TRIM_LENGTH_MODE: &str = "Length (px)";

pub trait PathEffectPlugin: Plugin {
    fn properties(&self) -> Vec<PropertyDefinition>;

    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::path_effect(self.id(), self.name(), self.properties())
    }

    /// Build one effect from descriptor-validated, fully materialized values.
    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> Result<PathEffect, LibraryError>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::PathEffect
    }
}

fn float_property(
    name: &str,
    label: &str,
    default: f64,
    range: std::ops::RangeInclusive<f64>,
    step: f64,
    suffix: &str,
    hard_limits: (bool, bool),
) -> PropertyDefinition {
    let (min, max) = range.into_inner();
    let (min_hard_limit, max_hard_limit) = hard_limits;
    PropertyDefinition::new(
        name,
        PropertyUiType::Float {
            min,
            max,
            step,
            suffix: suffix.to_string(),
            min_hard_limit,
            max_hard_limit,
        },
        label,
        PropertyValue::from(default),
    )
}

fn required_number(properties: &PropertyMap, key: &str) -> Result<f64, LibraryError> {
    properties.get_f64(key).ok_or_else(|| {
        LibraryError::Validation(format!(
            "Path Effect property {key:?} was not materialized as a number"
        ))
    })
}

fn required_finite_number(properties: &PropertyMap, key: &str) -> Result<f64, LibraryError> {
    let value = required_number(properties, key)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(LibraryError::Validation(format!(
            "Path Effect property {key:?} must be finite"
        )))
    }
}

fn required_integer(properties: &PropertyMap, key: &str) -> Result<i64, LibraryError> {
    properties.get_i64(key).ok_or_else(|| {
        LibraryError::Validation(format!(
            "Path Effect property {key:?} was not materialized as an integer"
        ))
    })
}

fn parse_dash_intervals(value: &str) -> Result<Vec<f64>, LibraryError> {
    let tokens = value
        .split([',', ' '])
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let intervals = tokens
        .iter()
        .map(|token| {
            token.parse::<f64>().map_err(|_| {
                LibraryError::Validation(format!(
                    "Dash Path Effect interval {token:?} is not a number"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if intervals.len() < 2 || intervals.len() % 2 != 0 {
        return Err(LibraryError::Validation(
            "Dash Path Effect requires an even number of at least two intervals".to_string(),
        ));
    }
    if intervals
        .iter()
        .any(|interval| !interval.is_finite() || *interval <= 0.0)
    {
        return Err(LibraryError::Validation(
            "Dash Path Effect intervals must be finite and greater than zero".to_string(),
        ));
    }
    Ok(intervals)
}

pub struct DashPathEffectPlugin;

impl Plugin for DashPathEffectPlugin {
    fn id(&self) -> &'static str {
        DASH_PATH_EFFECT_COMPONENT_ID
    }

    fn name(&self) -> String {
        "Dash".to_string()
    }

    fn category(&self) -> String {
        "Built-in".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PathEffectPlugin for DashPathEffectPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition::new(
                "intervals",
                PropertyUiType::Text,
                "Intervals",
                PropertyValue::String("8 4".to_string()),
            ),
            float_property(
                "phase",
                "Phase",
                0.0,
                0.0..=1000.0,
                1.0,
                "px",
                (false, false),
            ),
        ]
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        properties: &PropertyMap,
        _eval_time: f64,
    ) -> Result<PathEffect, LibraryError> {
        let value = properties.get_string("intervals").ok_or_else(|| {
            LibraryError::Validation(
                "Dash Path Effect intervals were not materialized as text".to_string(),
            )
        })?;
        Ok(PathEffect::Dash {
            intervals: parse_dash_intervals(&value)?,
            phase: required_number(properties, "phase")?,
        })
    }
}

pub struct CornerPathEffectPlugin;

impl Plugin for CornerPathEffectPlugin {
    fn id(&self) -> &'static str {
        CORNER_PATH_EFFECT_COMPONENT_ID
    }

    fn name(&self) -> String {
        "Corner".to_string()
    }

    fn category(&self) -> String {
        "Built-in".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PathEffectPlugin for CornerPathEffectPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![float_property(
            "radius",
            "Radius",
            8.0,
            0.0..=1000.0,
            1.0,
            "px",
            (true, false),
        )]
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        properties: &PropertyMap,
        _eval_time: f64,
    ) -> Result<PathEffect, LibraryError> {
        Ok(PathEffect::Corner {
            radius: required_number(properties, "radius")?,
        })
    }
}

pub struct DiscretePathEffectPlugin;

impl Plugin for DiscretePathEffectPlugin {
    fn id(&self) -> &'static str {
        DISCRETE_PATH_EFFECT_COMPONENT_ID
    }

    fn name(&self) -> String {
        "Discrete".to_string()
    }

    fn category(&self) -> String {
        "Built-in".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PathEffectPlugin for DiscretePathEffectPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            float_property(
                "segment_length",
                "Segment Length",
                8.0,
                0.1..=1000.0,
                1.0,
                "px",
                (true, false),
            ),
            float_property(
                "deviation",
                "Deviation",
                2.0,
                0.0..=1000.0,
                1.0,
                "px",
                (true, false),
            ),
            PropertyDefinition::new(
                "seed",
                PropertyUiType::Integer {
                    min: 0,
                    max: i64::MAX,
                    suffix: String::new(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "Seed",
                PropertyValue::Integer(0),
            ),
        ]
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        properties: &PropertyMap,
        _eval_time: f64,
    ) -> Result<PathEffect, LibraryError> {
        Ok(PathEffect::Discrete {
            seg_length: required_number(properties, "segment_length")?,
            deviation: required_number(properties, "deviation")?,
            seed: required_integer(properties, "seed")? as u64,
        })
    }
}

pub struct TrimPathEffectPlugin;

impl Plugin for TrimPathEffectPlugin {
    fn id(&self) -> &'static str {
        TRIM_PATH_EFFECT_COMPONENT_ID
    }

    fn name(&self) -> String {
        "Trim".to_string()
    }

    fn category(&self) -> String {
        "Built-in".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PathEffectPlugin for TrimPathEffectPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition::new(
                "mode",
                PropertyUiType::Dropdown {
                    options: vec![
                        TRIM_NORMALIZED_MODE.to_string(),
                        TRIM_LENGTH_MODE.to_string(),
                    ],
                },
                "Mode / Units",
                PropertyValue::String(TRIM_NORMALIZED_MODE.to_string()),
            ),
            float_property(
                "start",
                "Start",
                0.0,
                -1000.0..=1000.0,
                0.01,
                "",
                (false, false),
            ),
            float_property(
                "end",
                "End",
                1.0,
                -1000.0..=1000.0,
                0.01,
                "",
                (false, false),
            ),
            float_property(
                "offset",
                "Offset",
                0.0,
                -1000.0..=1000.0,
                0.01,
                "",
                (false, false),
            ),
        ]
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        properties: &PropertyMap,
        _eval_time: f64,
    ) -> Result<PathEffect, LibraryError> {
        let mode = properties.get_string("mode").ok_or_else(|| {
            LibraryError::Validation(
                "Trim Path Effect mode was not materialized as text".to_string(),
            )
        })?;
        let units = match mode.as_str() {
            TRIM_NORMALIZED_MODE => TrimPathUnits::Normalized,
            TRIM_LENGTH_MODE => TrimPathUnits::Length,
            _ => {
                return Err(LibraryError::Validation(format!(
                    "Trim Path Effect mode {mode:?} is not supported"
                )));
            }
        };
        Ok(PathEffect::Trim {
            start: required_finite_number(properties, "start")?,
            end: required_finite_number(properties, "end")?,
            offset: required_finite_number(properties, "offset")?,
            units,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dash_parser_rejects_lossy_or_non_positive_input() {
        assert_eq!(
            parse_dash_intervals("8, 4 2,1").unwrap(),
            [8.0, 4.0, 2.0, 1.0]
        );
        for invalid in ["", "8", "8 nope 4", "8 0", "8 -1", "8 NaN"] {
            assert!(
                parse_dash_intervals(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }
}
