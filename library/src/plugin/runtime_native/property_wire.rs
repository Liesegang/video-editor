use ordered_float::OrderedFloat;
use ruvie_plugin_api::{
    ColorV1, ComponentDescriptorV1, PROPERTY_VALUE_BOOLEAN_V1, PROPERTY_VALUE_COLOR_V1,
    PROPERTY_VALUE_INTEGER_V1, PROPERTY_VALUE_NUMBER_V1, PROPERTY_VALUE_STRING_V1,
    PROPERTY_VALUE_VEC2_V1, PROPERTY_VALUE_VEC3_V1, PROPERTY_VALUE_VEC4_V1, PropertyValueV1,
    RuvieBytesView, RuviePropertyValueViewV1,
};

use crate::error::LibraryError;
use crate::model::property::{PropertyValue, Vec2, Vec3, Vec4};
pub(super) fn empty_bytes_view() -> RuvieBytesView {
    RuvieBytesView {
        ptr: std::ptr::null(),
        len: 0,
    }
}

pub(super) fn property_views(
    values: &[(String, PropertyValue)],
) -> Result<Vec<RuviePropertyValueViewV1>, LibraryError> {
    values
        .iter()
        .map(|(name, value)| {
            let mut view = RuviePropertyValueViewV1 {
                name: RuvieBytesView::from_slice(name.as_bytes()),
                value_type: 0,
                number: 0.0,
                integer: 0,
                bytes: empty_bytes_view(),
                vector: [0.0; 4],
                color: [0; 4],
            };
            match value {
                PropertyValue::Number(value) if value.is_finite() => {
                    view.value_type = PROPERTY_VALUE_NUMBER_V1;
                    view.number = value.into_inner();
                }
                PropertyValue::Integer(value) => {
                    view.value_type = PROPERTY_VALUE_INTEGER_V1;
                    view.integer = *value;
                }
                PropertyValue::String(value) => {
                    view.value_type = PROPERTY_VALUE_STRING_V1;
                    view.bytes = RuvieBytesView::from_slice(value.as_bytes());
                }
                PropertyValue::Boolean(value) => {
                    view.value_type = PROPERTY_VALUE_BOOLEAN_V1;
                    view.integer = i64::from(*value);
                }
                PropertyValue::Vec2(value) if value.x.is_finite() && value.y.is_finite() => {
                    view.value_type = PROPERTY_VALUE_VEC2_V1;
                    view.vector[..2].copy_from_slice(&[value.x.into_inner(), value.y.into_inner()]);
                }
                PropertyValue::Vec3(value)
                    if value.x.is_finite() && value.y.is_finite() && value.z.is_finite() =>
                {
                    view.value_type = PROPERTY_VALUE_VEC3_V1;
                    view.vector[..3].copy_from_slice(&[
                        value.x.into_inner(),
                        value.y.into_inner(),
                        value.z.into_inner(),
                    ]);
                }
                PropertyValue::Vec4(value)
                    if value.x.is_finite()
                        && value.y.is_finite()
                        && value.z.is_finite()
                        && value.w.is_finite() =>
                {
                    view.value_type = PROPERTY_VALUE_VEC4_V1;
                    view.vector.copy_from_slice(&[
                        value.x.into_inner(),
                        value.y.into_inner(),
                        value.z.into_inner(),
                        value.w.into_inner(),
                    ]);
                }
                PropertyValue::Color(value) => {
                    view.value_type = PROPERTY_VALUE_COLOR_V1;
                    view.color = [value.r, value.g, value.b, value.a];
                }
                PropertyValue::Number(_)
                | PropertyValue::Vec2(_)
                | PropertyValue::Vec3(_)
                | PropertyValue::Vec4(_) => {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Effect property {name:?} contains a non-finite value"
                    )));
                }
                PropertyValue::Array(_) | PropertyValue::Map(_) => {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Effect property {name:?} uses an unsupported aggregate value"
                    )));
                }
            }
            Ok(view)
        })
        .collect()
}

pub(super) fn color_from_wire(color: ColorV1) -> crate::model::frame::color::Color {
    crate::model::frame::color::Color {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

pub(super) fn property_value_to_wire(
    value: &PropertyValue,
) -> Result<PropertyValueV1, &'static str> {
    match value {
        PropertyValue::Number(value) if value.is_finite() => Ok(PropertyValueV1::Number {
            value: value.into_inner(),
        }),
        PropertyValue::Number(_) => Err("number must be finite"),
        PropertyValue::Integer(value) => Ok(PropertyValueV1::Integer { value: *value }),
        PropertyValue::String(value) => Ok(PropertyValueV1::String {
            value: value.clone(),
        }),
        PropertyValue::Boolean(value) => Ok(PropertyValueV1::Boolean { value: *value }),
        PropertyValue::Vec2(value) if value.x.is_finite() && value.y.is_finite() => {
            Ok(PropertyValueV1::Vec2 {
                x: value.x.into_inner(),
                y: value.y.into_inner(),
            })
        }
        PropertyValue::Vec3(value)
            if value.x.is_finite() && value.y.is_finite() && value.z.is_finite() =>
        {
            Ok(PropertyValueV1::Vec3 {
                x: value.x.into_inner(),
                y: value.y.into_inner(),
                z: value.z.into_inner(),
            })
        }
        PropertyValue::Vec4(value)
            if value.x.is_finite()
                && value.y.is_finite()
                && value.z.is_finite()
                && value.w.is_finite() =>
        {
            Ok(PropertyValueV1::Vec4 {
                x: value.x.into_inner(),
                y: value.y.into_inner(),
                z: value.z.into_inner(),
                w: value.w.into_inner(),
            })
        }
        PropertyValue::Vec2(_) | PropertyValue::Vec3(_) | PropertyValue::Vec4(_) => {
            Err("vector components must be finite")
        }
        PropertyValue::Color(value) => Ok(PropertyValueV1::Color {
            r: value.r,
            g: value.g,
            b: value.b,
            a: value.a,
        }),
        PropertyValue::Array(_) | PropertyValue::Map(_) => {
            Err("array and map values are not supported by ABI v1")
        }
    }
}

pub(super) fn property_value_from_wire(
    value: &PropertyValueV1,
) -> Result<PropertyValue, LibraryError> {
    let non_finite =
        || LibraryError::Plugin("Runtime property value contains a non-finite number".to_string());
    match value {
        PropertyValueV1::Number { value } => value
            .is_finite()
            .then_some(PropertyValue::Number(OrderedFloat(*value)))
            .ok_or_else(non_finite),
        PropertyValueV1::Integer { value } => Ok(PropertyValue::Integer(*value)),
        PropertyValueV1::String { value } => Ok(PropertyValue::String(value.clone())),
        PropertyValueV1::Boolean { value } => Ok(PropertyValue::Boolean(*value)),
        PropertyValueV1::Vec2 { x, y } => {
            if !x.is_finite() || !y.is_finite() {
                return Err(non_finite());
            }
            Ok(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(*x),
                y: OrderedFloat(*y),
            }))
        }
        PropertyValueV1::Vec3 { x, y, z } => {
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return Err(non_finite());
            }
            Ok(PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(*x),
                y: OrderedFloat(*y),
                z: OrderedFloat(*z),
            }))
        }
        PropertyValueV1::Vec4 { x, y, z, w } => {
            if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
                return Err(non_finite());
            }
            Ok(PropertyValue::Vec4(Vec4 {
                x: OrderedFloat(*x),
                y: OrderedFloat(*y),
                z: OrderedFloat(*z),
                w: OrderedFloat(*w),
            }))
        }
        PropertyValueV1::Color { r, g, b, a } => {
            Ok(PropertyValue::Color(crate::model::frame::color::Color {
                r: *r,
                g: *g,
                b: *b,
                a: *a,
            }))
        }
    }
}

pub(super) fn property_output_default(
    component: &ComponentDescriptorV1,
) -> Result<PropertyValue, LibraryError> {
    let value = component.output_default.as_ref().ok_or_else(|| {
        LibraryError::Plugin(format!(
            "Runtime property '{}' must declare output_default",
            component.id
        ))
    })?;
    property_value_from_wire(value).map_err(|error| {
        LibraryError::Plugin(format!(
            "Runtime property '{}' has an invalid output_default: {error}",
            component.id
        ))
    })
}
