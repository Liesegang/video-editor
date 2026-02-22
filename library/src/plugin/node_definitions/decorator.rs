use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;
use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};

pub(super) fn decorator_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Decorator;

    let prop = PropertyDefinition::new;
    vec![
        node("decorator.backplate", "Backplate", nc)
            .with_description("Background shape behind text characters/lines/blocks")
            .with_inputs(vec![inp("shape_in", "Shape In", Shape)])
            .with_outputs(vec![out("shape_out", "Shape Out", Shape)])
            .with_properties(vec![
                prop(
                    "target",
                    PropertyUiType::Dropdown {
                        options: vec!["Char".into(), "Line".into(), "Block".into()],
                    },
                    "Target",
                    PropertyValue::String("Block".into()),
                ),
                prop(
                    "shape",
                    PropertyUiType::Dropdown {
                        options: vec!["Rect".into(), "RoundRect".into(), "Circle".into()],
                    },
                    "Shape",
                    PropertyValue::String("Rect".into()),
                ),
                prop(
                    "color",
                    PropertyUiType::Color,
                    "Color",
                    PropertyValue::Color(crate::runtime::color::Color::black()),
                ),
                prop(
                    "padding",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Padding",
                    PropertyValue::from(0.0),
                ),
                prop(
                    "radius",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 50.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Corner Radius",
                    PropertyValue::from(0.0),
                ),
            ]),
    ]
}
