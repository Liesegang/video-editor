use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;
use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};

pub(super) fn effector_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Effector;

    let prop = PropertyDefinition::new;
    vec![
        node("effector.transform", "Transform Effector", nc)
            .with_description("Per-character transform (translate, rotate, scale)")
            .with_inputs(vec![inp("shape_in", "Shape In", Shape)])
            .with_outputs(vec![out("shape_out", "Shape Out", Shape)])
            .with_properties(vec![
                prop(
                    "tx",
                    PropertyUiType::Float {
                        min: -1000.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Translate X",
                    PropertyValue::from(0.0),
                ),
                prop(
                    "ty",
                    PropertyUiType::Float {
                        min: -1000.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Translate Y",
                    PropertyValue::from(0.0),
                ),
                prop(
                    "scale_x",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 10.0,
                        step: 0.1,
                        suffix: "".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Scale X",
                    PropertyValue::from(1.0),
                ),
                prop(
                    "scale_y",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 10.0,
                        step: 0.1,
                        suffix: "".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Scale Y",
                    PropertyValue::from(1.0),
                ),
                prop(
                    "rotation",
                    PropertyUiType::Float {
                        min: -360.0,
                        max: 360.0,
                        step: 1.0,
                        suffix: "\u{b0}".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Rotation",
                    PropertyValue::from(0.0),
                ),
            ]),
        node("effector.step_delay", "Step Delay", nc)
            .with_description("Staggered reveal per character")
            .with_inputs(vec![inp("shape_in", "Shape In", Shape)])
            .with_outputs(vec![out("shape_out", "Shape Out", Shape)])
            .with_properties(vec![
                prop(
                    "delay",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 5.0,
                        step: 0.01,
                        suffix: "s".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Delay",
                    PropertyValue::from(0.05),
                ),
                prop(
                    "duration",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 5.0,
                        step: 0.01,
                        suffix: "s".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Duration",
                    PropertyValue::from(0.2),
                ),
                prop(
                    "from_opacity",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "From Opacity",
                    PropertyValue::from(0.0),
                ),
                prop(
                    "to_opacity",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "To Opacity",
                    PropertyValue::from(100.0),
                ),
            ]),
        node("effector.randomize", "Randomize", nc)
            .with_description("Random per-character transform jitter")
            .with_inputs(vec![inp("shape_in", "Shape In", Shape)])
            .with_outputs(vec![out("shape_out", "Shape Out", Shape)])
            .with_properties(vec![
                prop(
                    "seed",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Seed",
                    PropertyValue::from(0.0),
                ),
                prop(
                    "amount",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "Amount",
                    PropertyValue::from(1.0),
                ),
                prop(
                    "translate_range",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 500.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Translate Range",
                    PropertyValue::from(50.0),
                ),
                prop(
                    "rotate_range",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 360.0,
                        step: 1.0,
                        suffix: "\u{b0}".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Rotate Range",
                    PropertyValue::from(15.0),
                ),
                prop(
                    "scale_range",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 5.0,
                        step: 0.1,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Scale Range",
                    PropertyValue::from(0.5),
                ),
            ]),
        node("effector.opacity", "Opacity Effector", nc)
            .with_description("Per-character opacity control")
            .with_inputs(vec![inp("shape_in", "Shape In", Shape)])
            .with_outputs(vec![out("shape_out", "Shape Out", Shape)])
            .with_properties(vec![
                prop(
                    "opacity",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "Opacity",
                    PropertyValue::from(0.0),
                ),
                prop(
                    "mode",
                    PropertyUiType::Dropdown {
                        options: vec!["Set".into(), "Add".into(), "Multiply".into()],
                    },
                    "Mode",
                    PropertyValue::String("Set".into()),
                ),
            ]),
    ]
}
