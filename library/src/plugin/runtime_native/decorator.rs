//! Version-negotiated runtime Decorator adapter.

use ruvie_plugin_api::{
    BackplateFitV2, BackplateOffsetV2, BackplateShapeV1, ComponentDescriptorV1,
    DECORATOR_EVALUATE_V1, DECORATOR_EVALUATE_V2, DecoratorEvaluateRequestV1,
    DecoratorEvaluateRequestV2, DecoratorOutputV1, DecoratorOutputV2, DecoratorTargetV1,
    DecoratorTargetV2, InsetsV1, InsetsV2,
};

use super::{RuntimeComponent, color_from_wire, parse_semver_triplet, resolved_config_properties};
use crate::error::LibraryError;
use crate::model::property::PropertyDefinition;
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::{DecoratorPlugin, Plugin};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimeDecoratorProtocol {
    V1,
    V2,
}

impl RuntimeDecoratorProtocol {
    pub(super) fn negotiate(descriptor: &ComponentDescriptorV1) -> Option<Self> {
        if descriptor
            .operations
            .iter()
            .any(|operation| operation == DECORATOR_EVALUATE_V2)
        {
            Some(Self::V2)
        } else if descriptor
            .operations
            .iter()
            .any(|operation| operation == DECORATOR_EVALUATE_V1)
        {
            Some(Self::V1)
        } else {
            None
        }
    }
}

pub(super) struct RuntimeDecoratorPlugin {
    pub(super) component: RuntimeComponent,
    pub(super) definitions: Vec<PropertyDefinition>,
    pub(super) protocol: RuntimeDecoratorProtocol,
}

impl Plugin for RuntimeDecoratorPlugin {
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

impl DecoratorPlugin for RuntimeDecoratorPlugin {
    fn descriptor(
        &self,
    ) -> Result<crate::plugin::OperationDescriptor, crate::plugin::OperationDescriptorError> {
        match self.protocol {
            RuntimeDecoratorProtocol::V1 => crate::plugin::OperationDescriptor::decorator(
                self.id(),
                self.name(),
                self.properties(),
            ),
            RuntimeDecoratorProtocol::V2 => crate::plugin::OperationDescriptor::backplate(
                self.id(),
                self.name(),
                self.properties(),
            ),
        }
    }

    fn properties(&self) -> Vec<PropertyDefinition> {
        self.definitions.clone()
    }

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        _source_id: uuid::Uuid,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> Option<crate::core::ensemble::types::DecoratorConfig> {
        let label = format!("Runtime Decorator '{}'", self.id());
        let properties =
            resolved_config_properties(context, &self.definitions, properties, eval_time, &label)?;
        let (operation, payload) = match self.protocol {
            RuntimeDecoratorProtocol::V1 => (
                DECORATOR_EVALUATE_V1,
                serde_json::to_value(DecoratorEvaluateRequestV1 {
                    time: eval_time,
                    fps: context.evaluation_fps(),
                    properties,
                }),
            ),
            RuntimeDecoratorProtocol::V2 => (
                DECORATOR_EVALUATE_V2,
                serde_json::to_value(DecoratorEvaluateRequestV2 {
                    time: eval_time,
                    fps: context.evaluation_fps(),
                    properties,
                }),
            ),
        };
        let payload = match payload {
            Ok(payload) => payload,
            Err(error) => {
                log::error!("Failed to encode {label}: {error}");
                return None;
            }
        };
        let response = match self.component.invoke(operation, payload) {
            Ok(response) => response,
            Err(error) => {
                log::error!("{label} failed: {error}");
                return None;
            }
        };
        match self.protocol {
            RuntimeDecoratorProtocol::V1 => safe_decorator_config_from_response(response, &label),
            RuntimeDecoratorProtocol::V2 => {
                safe_decorator_config_from_response_v2(response, &label)
            }
        }
    }
}

fn safe_decorator_config_from_response(
    response: serde_json::Value,
    operation_label: &str,
) -> Option<crate::core::ensemble::types::DecoratorConfig> {
    match decorator_config_from_response(response) {
        Ok(output) => output,
        Err(error) => {
            log::error!("{operation_label} returned an invalid config: {error}");
            None
        }
    }
}

pub(super) fn decorator_config_from_response(
    response: serde_json::Value,
) -> Result<Option<crate::core::ensemble::types::DecoratorConfig>, LibraryError> {
    let output = serde_json::from_value(response).map_err(|error| {
        LibraryError::Plugin(format!("Runtime Decorator response is invalid: {error}"))
    })?;
    decorator_config_from_wire(output)
}

fn decorator_config_from_wire(
    output: DecoratorOutputV1,
) -> Result<Option<crate::core::ensemble::types::DecoratorConfig>, LibraryError> {
    use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
    use crate::core::ensemble::types::DecoratorConfig;

    let DecoratorOutputV1::Backplate {
        target,
        shape,
        color,
        padding,
        corner_radius,
    } = output
    else {
        return Ok(None);
    };
    let InsetsV1 {
        top,
        right,
        bottom,
        left,
    } = padding;
    if !valid_backplate_render_geometry((top, right, bottom, left), corner_radius) {
        return Err(LibraryError::Plugin(
            "Runtime Decorator output has invalid Backplate numeric fields".to_string(),
        ));
    }
    Ok(Some(DecoratorConfig::LegacyBackplate {
        target: match target {
            DecoratorTargetV1::Block => BackplateTarget::Block,
            DecoratorTargetV1::Line => BackplateTarget::Line,
            DecoratorTargetV1::Char => BackplateTarget::Char,
        },
        shape: match shape {
            BackplateShapeV1::Rect => BackplateShape::Rect,
            BackplateShapeV1::RoundedRect => BackplateShape::RoundedRect,
            BackplateShapeV1::Circle => BackplateShape::Circle,
        },
        color: color_from_wire(color),
        padding: (top, right, bottom, left),
        corner_radius,
    }))
}

pub(super) fn safe_decorator_config_from_response_v2(
    response: serde_json::Value,
    operation_label: &str,
) -> Option<crate::core::ensemble::types::DecoratorConfig> {
    match decorator_config_from_response_v2(response) {
        Ok(output) => output,
        Err(error) => {
            log::error!("{operation_label} returned an invalid v2 config: {error}");
            None
        }
    }
}

pub(super) fn decorator_config_from_response_v2(
    response: serde_json::Value,
) -> Result<Option<crate::core::ensemble::types::DecoratorConfig>, LibraryError> {
    let output = serde_json::from_value(response).map_err(|error| {
        LibraryError::Plugin(format!("Runtime Decorator v2 response is invalid: {error}"))
    })?;
    decorator_config_from_wire_v2(output)
}

pub(super) fn decorator_config_from_wire_v2(
    output: DecoratorOutputV2,
) -> Result<Option<crate::core::ensemble::types::DecoratorConfig>, LibraryError> {
    use crate::core::ensemble::decorators::{BackplateFit, BackplateTarget};
    use crate::core::ensemble::types::DecoratorConfig;

    let DecoratorOutputV2::Backplate {
        target,
        padding,
        offset,
        fit,
    } = output
    else {
        return Ok(None);
    };
    let InsetsV2 {
        top,
        right,
        bottom,
        left,
    } = padding;
    let BackplateOffsetV2 { x, y } = offset;
    if ![top, right, bottom, left, x, y]
        .into_iter()
        .all(f32::is_finite)
    {
        return Err(LibraryError::Plugin(
            "Runtime Decorator v2 output has invalid Backplate numeric fields".to_string(),
        ));
    }
    Ok(Some(DecoratorConfig::Backplate {
        target: match target {
            DecoratorTargetV2::Block => BackplateTarget::Block,
            DecoratorTargetV2::Line => BackplateTarget::Line,
            DecoratorTargetV2::Char => BackplateTarget::Char,
        },
        padding: (top, right, bottom, left),
        offset: (x, y),
        fit: match fit {
            BackplateFitV2::Stretch => BackplateFit::Stretch,
            BackplateFitV2::Contain => BackplateFit::Contain,
            BackplateFitV2::Cover => BackplateFit::Cover,
        },
    }))
}

fn valid_backplate_render_geometry(padding: (f32, f32, f32, f32), corner_radius: f32) -> bool {
    let (top, right, bottom, left) = padding;
    top.is_finite()
        && right.is_finite()
        && bottom.is_finite()
        && left.is_finite()
        && (left + right).is_finite()
        && (top + bottom).is_finite()
        && backplate_pad_is_finite(
            crate::model::frame::runtime_shape::RuntimeBounds::new(-1.0, -2.0, 3.0, 4.0),
            padding,
        )
        && corner_radius.is_finite()
        && corner_radius >= 0.0
        && (corner_radius * 2.0).is_finite()
}

fn backplate_pad_is_finite(
    bounds: crate::model::frame::runtime_shape::RuntimeBounds,
    padding: (f32, f32, f32, f32),
) -> bool {
    let padded = bounds.pad(padding);
    [
        padded.left,
        padded.top,
        padded.right,
        padded.bottom,
        padded.right - padded.left,
        padded.bottom - padded.top,
    ]
    .into_iter()
    .all(f32::is_finite)
}
