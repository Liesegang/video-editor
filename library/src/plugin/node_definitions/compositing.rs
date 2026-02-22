use super::{blend_node, inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn compositing_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Compositing;
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
            .with_outputs(vec![out("image_out", "Image", Image)]),
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
