use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;
use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};

pub(super) fn style_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Style;

    let prop = PropertyDefinition::new;
    vec![
        node("style.fill", "Fill", nc)
            .with_description("Solid color fill style")
            .with_inputs(vec![inp("shape_in", "Shape In", Shape)])
            .with_outputs(vec![out("image_out", "Image Out", Image)])
            .with_properties(vec![
                prop(
                    "color",
                    PropertyUiType::Color,
                    "Color",
                    PropertyValue::Color(crate::runtime::color::Color::white()),
                ),
                prop(
                    "opacity",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "Opacity",
                    PropertyValue::from(1.0),
                ),
                prop(
                    "offset",
                    PropertyUiType::Float {
                        min: -50.0,
                        max: 50.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Offset",
                    PropertyValue::from(0.0),
                ),
            ]),
        node("style.stroke", "Stroke", nc)
            .with_description("Stroke outline style")
            .with_inputs(vec![inp("shape_in", "Shape In", Shape)])
            .with_outputs(vec![out("image_out", "Image Out", Image)])
            .with_properties(vec![
                prop(
                    "color",
                    PropertyUiType::Color,
                    "Color",
                    PropertyValue::Color(crate::runtime::color::Color::white()),
                ),
                prop(
                    "width",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Width",
                    PropertyValue::from(1.0),
                ),
                prop(
                    "opacity",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "Opacity",
                    PropertyValue::from(1.0),
                ),
                prop(
                    "offset",
                    PropertyUiType::Float {
                        min: -50.0,
                        max: 50.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Offset",
                    PropertyValue::from(0.0),
                ),
                prop(
                    "join",
                    PropertyUiType::Dropdown {
                        options: vec!["Miter".into(), "Round".into(), "Bevel".into()],
                    },
                    "Join",
                    PropertyValue::String("Round".into()),
                ),
                prop(
                    "cap",
                    PropertyUiType::Dropdown {
                        options: vec!["Butt".into(), "Round".into(), "Square".into()],
                    },
                    "Cap",
                    PropertyValue::String("Round".into()),
                ),
                prop(
                    "miter_limit",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 0.1,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Miter Limit",
                    PropertyValue::from(4.0),
                ),
                prop(
                    "dash_array",
                    PropertyUiType::Text,
                    "Dash Array",
                    PropertyValue::String("".into()),
                ),
                prop(
                    "dash_offset",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Dash Offset",
                    PropertyValue::from(0.0),
                ),
            ]),
    ]
}
