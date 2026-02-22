use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn image_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Image;
    vec![
        node("image.channel_split", "Channel Split", nc)
            .with_inputs(vec![inp("image", "Image", Image)])
            .with_outputs(vec![
                out("r", "R", Image),
                out("g", "G", Image),
                out("b", "B", Image),
                out("a", "A", Image),
            ]),
        node("image.channel_combine", "Channel Combine", nc)
            .with_inputs(vec![
                inp("r", "R", Image),
                inp("g", "G", Image),
                inp("b", "B", Image),
                inp("a", "A", Image),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("image.rgb_to_yuv", "RGB to YUV", nc)
            .with_inputs(vec![inp("rgb_image", "RGB Image", Image)])
            .with_outputs(vec![out("yuv_image", "YUV Image", Image)]),
        node("image.yuv_to_rgb", "YUV to RGB", nc)
            .with_inputs(vec![inp("yuv_image", "YUV Image", Image)])
            .with_outputs(vec![out("rgb_image", "RGB Image", Image)]),
    ]
}
