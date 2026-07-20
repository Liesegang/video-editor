//! Native whole-Shape spatial transform operation.
//!
//! This operation is the single owner of base position, rotation, scale, and
//! anchor. It is deliberately not an Ensemble Effector: Effectors are optional
//! element/group modulation and never become Preview's absolute placement.

use ordered_float::OrderedFloat;

use crate::model::frame::transform::{Position, Scale, Transform};
use crate::model::property::{
    PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue, Vec2,
};
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::{OperationDescriptor, OperationDescriptorError};

pub fn property_definitions() -> Vec<PropertyDefinition> {
    vec![
        PropertyDefinition::new(
            "position",
            PropertyUiType::Vec2 {
                suffix: "px".to_string(),
            },
            "Position",
            vec2_value(0.0, 0.0),
        ),
        PropertyDefinition::new(
            "rotation",
            PropertyUiType::Float {
                min: -360.0,
                max: 360.0,
                step: 1.0,
                suffix: "deg".to_string(),
                min_hard_limit: false,
                max_hard_limit: false,
            },
            "Rotation",
            PropertyValue::from(0.0),
        ),
        PropertyDefinition::new(
            "scale",
            PropertyUiType::Vec2 {
                suffix: "%".to_string(),
            },
            "Scale",
            vec2_value(100.0, 100.0),
        ),
        PropertyDefinition::new(
            "anchor",
            PropertyUiType::Vec2 {
                suffix: "px".to_string(),
            },
            "Anchor",
            vec2_value(0.0, 0.0),
        ),
    ]
}

pub fn shape_descriptor() -> Result<OperationDescriptor, OperationDescriptorError> {
    OperationDescriptor::shape_transform(property_definitions())
}

pub fn image_descriptor() -> Result<OperationDescriptor, OperationDescriptorError> {
    OperationDescriptor::image_transform(property_definitions())
}

pub fn evaluate_source(
    context: &FrameEvaluationContext<'_>,
    definitions: &[PropertyDefinition],
    properties: &PropertyMap,
    eval_time: f64,
) -> Option<Transform> {
    let evaluated =
        context.evaluate_operation_properties(definitions, properties, eval_time, "Transform")?;
    let position = evaluated.get("position")?.get_as::<Vec2>()?;
    let rotation = evaluated.get("rotation")?.get_as::<f64>()?;
    let scale = evaluated.get("scale")?.get_as::<Vec2>()?;
    let anchor = evaluated.get("anchor")?.get_as::<Vec2>()?;

    let values = [
        position.x.into_inner(),
        position.y.into_inner(),
        rotation,
        scale.x.into_inner(),
        scale.y.into_inner(),
        anchor.x.into_inner(),
        anchor.y.into_inner(),
    ];
    if values.iter().any(|value| !value.is_finite()) {
        log::warn!("Transform evaluated to a non-finite spatial value; producing NoOutput");
        return None;
    }

    Some(Transform {
        position: Position {
            x: position.x.into_inner(),
            y: position.y.into_inner(),
        },
        rotation,
        scale: Scale {
            x: scale.x.into_inner() / 100.0,
            y: scale.y.into_inner() / 100.0,
        },
        anchor: Position {
            x: anchor.x.into_inner(),
            y: anchor.y.into_inner(),
        },
        // Base opacity belongs to Style. Transform never changes alpha.
        opacity: 1.0,
    })
}

pub fn vec2_value(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}
