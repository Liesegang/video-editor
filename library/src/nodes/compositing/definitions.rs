use crate::nodes::{blend_node, inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;
use crate::project::property::{
    PropertyDefinition, PropertyUiType, PropertyValue, Vec2 as PropVec2,
};
use ordered_float::OrderedFloat;

pub(super) fn compositing_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Compositing;
    let prop = PropertyDefinition::new;
    vec![
        node("compositing.transform", "Transform", nc)
            .with_inputs(vec![
                inp("image_in", "Image", Image),
                inp("position", "Position", Vec2),
                inp("rotation", "Rotation", Scalar),
                inp("scale", "Scale", Vec2),
                inp("anchor", "Anchor", Vec2),
                inp("opacity", "Opacity", Scalar),
            ])
            .with_outputs(vec![out("image_out", "Image", Image)])
            .with_properties(vec![
                prop(
                    "position",
                    PropertyUiType::Vec2 {
                        suffix: "".to_string(),
                    },
                    "Position",
                    PropertyValue::Vec2(PropVec2 {
                        x: OrderedFloat(0.0),
                        y: OrderedFloat(0.0),
                    }),
                ),
                prop(
                    "scale",
                    PropertyUiType::Vec2 {
                        suffix: "".to_string(),
                    },
                    "Scale",
                    PropertyValue::Vec2(PropVec2 {
                        x: OrderedFloat(100.0),
                        y: OrderedFloat(100.0),
                    }),
                ),
                prop(
                    "rotation",
                    PropertyUiType::Float {
                        min: -360.0,
                        max: 360.0,
                        step: 1.0,
                        suffix: "°".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Rotation",
                    PropertyValue::Number(OrderedFloat(0.0)),
                ),
                prop(
                    "anchor",
                    PropertyUiType::Vec2 {
                        suffix: "".to_string(),
                    },
                    "Anchor Point",
                    PropertyValue::Vec2(PropVec2 {
                        x: OrderedFloat(0.0),
                        y: OrderedFloat(0.0),
                    }),
                ),
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
                    PropertyValue::Number(OrderedFloat(100.0)),
                ),
            ]),
        node("compositing.composite", "Composite", nc)
            .with_description("Blend N images with individual blend modes")
            .with_inputs(vec![
                inp("layers", "Layers", List),
                inp("blend_modes", "Blend Modes", List),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        // Blend nodes (share identical pin layout)
        blend_node("compositing.normal_blend", "Normal Blend"),
        blend_node("compositing.multiply_blend", "Multiply Blend"),
        blend_node("compositing.screen_blend", "Screen Blend"),
        blend_node("compositing.overlay_blend", "Overlay Blend"),
        node("compositing.mask", "Mask", nc)
            .with_inputs(vec![
                inp("source", "Source", Image),
                inp("mask", "Mask", Image),
                inp("mode", "Mode", Enum),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
    ]
}
