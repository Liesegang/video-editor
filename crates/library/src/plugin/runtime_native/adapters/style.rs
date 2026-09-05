use ruvie_plugin_api::{
    MAX_STYLE_DASH_INTERVALS_V1, STYLE_EVALUATE_V1, StrokeCapV1, StrokeJoinV1,
    StyleEvaluateRequestV1, StyleOutputV1,
};

use super::super::abi::RuntimeComponent;
use super::super::property_wire::color_from_wire;
use super::{evaluated_config_properties, parse_semver_triplet};
use crate::error::LibraryError;
use crate::model::property::PropertyDefinition;
use crate::plugin::{EvaluatedOperation, Plugin, StylePlugin};
pub(in crate::plugin::runtime_native) struct RuntimeStylePlugin {
    pub(in crate::plugin::runtime_native) component: RuntimeComponent,
    pub(in crate::plugin::runtime_native) definitions: Vec<PropertyDefinition>,
}

impl Plugin for RuntimeStylePlugin {
    fn id(&self) -> &str {
        &self.component.descriptor.id
    }

    fn name(&self) -> String {
        self.component.descriptor.name.clone()
    }

    fn category(&self) -> String {
        self.component.descriptor.group.clone()
    }

    fn version(&self) -> (u32, u32, u32) {
        parse_semver_triplet(&self.component.descriptor.version)
    }

    fn impl_type(&self) -> String {
        "Native ABI v1".to_string()
    }
}

impl StylePlugin for RuntimeStylePlugin {
    fn descriptor(
        &self,
    ) -> Result<crate::plugin::OperationDescriptor, crate::plugin::OperationDescriptorError> {
        crate::plugin::OperationDescriptor::style(self.id(), self.name(), self.definitions.clone())
    }

    fn evaluate_values(
        &self,
        context: &EvaluatedOperation<'_>,
        source_id: uuid::Uuid,
    ) -> Option<crate::model::frame::entity::StyleConfig> {
        let label = format!("Runtime Style '{}'", self.id());
        let properties =
            evaluated_config_properties(&self.definitions, context.properties(), &label)?;
        let payload = match serde_json::to_value(StyleEvaluateRequestV1 {
            time: context.time(),
            fps: context.fps(),
            properties,
        }) {
            Ok(payload) => payload,
            Err(error) => {
                log::error!("Failed to encode {label}: {error}");
                return None;
            }
        };
        let response = match self.component.invoke(STYLE_EVALUATE_V1, payload) {
            Ok(response) => response,
            Err(error) => {
                log::error!("{label} failed: {error}");
                return None;
            }
        };
        safe_style_config_from_response(response, source_id, &label)
    }
}

pub(in crate::plugin::runtime_native) fn safe_style_config_from_response(
    response: serde_json::Value,
    source_id: uuid::Uuid,
    operation_label: &str,
) -> Option<crate::model::frame::entity::StyleConfig> {
    match style_config_from_response(response, source_id) {
        Ok(output) => output,
        Err(error) => {
            log::error!("{operation_label} returned an invalid config: {error}");
            None
        }
    }
}

pub(in crate::plugin::runtime_native) fn style_config_from_response(
    response: serde_json::Value,
    source_id: uuid::Uuid,
) -> Result<Option<crate::model::frame::entity::StyleConfig>, LibraryError> {
    let output = serde_json::from_value(response).map_err(|error| {
        LibraryError::Plugin(format!("Runtime Style response is invalid: {error}"))
    })?;
    style_config_from_wire(output, source_id)
}

pub(in crate::plugin::runtime_native) fn style_config_from_wire(
    output: StyleOutputV1,
    source_id: uuid::Uuid,
) -> Result<Option<crate::model::frame::entity::StyleConfig>, LibraryError> {
    use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType};

    let invalid = |detail: &str| LibraryError::Plugin(format!("Runtime Style output {detail}"));
    let style = match output {
        StyleOutputV1::NoOutput => return Ok(None),
        StyleOutputV1::Fill { color, offset } => {
            if !finite_render_scalar(offset) || !finite_render_scalar(offset * 2.0) {
                return Err(invalid("has an unsafe Fill offset"));
            }
            DrawStyle::Fill {
                color: color_from_wire(color),
                offset,
            }
        }
        StyleOutputV1::Stroke {
            color,
            width,
            offset,
            cap,
            join,
            miter,
            dash_array,
            dash_offset,
        } => {
            if !valid_stroke_render_geometry(width, offset)
                || !finite_render_scalar(miter)
                || !finite_render_scalar(dash_offset)
                || miter < 0.0
                || !valid_stroke_dash_pattern(&dash_array, dash_offset)
            {
                return Err(invalid("has invalid Stroke numeric fields"));
            }
            DrawStyle::Stroke {
                color: color_from_wire(color),
                width,
                offset,
                cap: match cap {
                    StrokeCapV1::Round => CapType::Round,
                    StrokeCapV1::Square => CapType::Square,
                    StrokeCapV1::Butt => CapType::Butt,
                },
                join: match join {
                    StrokeJoinV1::Round => JoinType::Round,
                    StrokeJoinV1::Bevel => JoinType::Bevel,
                    StrokeJoinV1::Miter => JoinType::Miter,
                },
                miter,
                dash_array,
                dash_offset,
            }
        }
    };
    Ok(Some(crate::model::frame::entity::StyleConfig {
        id: source_id,
        style,
    }))
}

pub(in crate::plugin::runtime_native) fn valid_stroke_render_geometry(
    width: f64,
    offset: f64,
) -> bool {
    if width < 0.0 || !finite_render_scalar(width) || !finite_render_scalar(offset) {
        return false;
    }
    let effective_width = (width + offset * 2.0).max(0.0);
    if !finite_render_scalar(effective_width) {
        return false;
    }
    if width <= 0.0 || offset == 0.0 {
        return true;
    }

    // Shape rendering paints a 2*outer radius and may erase a 2*inner radius.
    // This differs from the effective width used by text rendering, so both
    // paths need their exact derived scalars checked before f64 -> f32 casts.
    let half_width = width / 2.0;
    let offset_magnitude = offset.abs();
    let outer_radius = offset_magnitude + half_width;
    let inner_radius = offset_magnitude - half_width;
    finite_render_scalar(half_width)
        && finite_render_scalar(outer_radius)
        && finite_render_scalar(outer_radius * 2.0)
        && (inner_radius <= 0.0
            || (finite_render_scalar(inner_radius) && finite_render_scalar(inner_radius * 2.0)))
}

pub(in crate::plugin::runtime_native) fn valid_stroke_dash_pattern(
    values: &[f64],
    phase: f64,
) -> bool {
    if !finite_render_scalar(phase) {
        return false;
    }
    if values.is_empty() {
        return true;
    }
    if values.len() > MAX_STYLE_DASH_INTERVALS_V1 || !values.len().is_multiple_of(2) {
        return false;
    }

    let mut intervals = Vec::with_capacity(values.len());
    let mut period = 0.0_f32;
    for value in values {
        let interval = *value as f32;
        if !value.is_finite() || !interval.is_finite() || interval <= 0.0 {
            return false;
        }
        period += interval;
        if !period.is_finite() {
            return false;
        }
        intervals.push(interval);
    }
    period > 0.0 && skia_safe::PathEffect::dash(&intervals, phase as f32).is_some()
}

fn finite_render_scalar(value: f64) -> bool {
    value.is_finite() && (value as f32).is_finite()
}
