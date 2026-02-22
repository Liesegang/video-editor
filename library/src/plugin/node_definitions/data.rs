use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn data_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Data;
    vec![
        node("data.scalar", "Scalar", nc)
            .with_description("Single numeric value")
            .with_outputs(vec![out("value", "Value", Scalar)]),
        node("data.vector", "Vector", nc)
            .with_description("Generic Vector")
            .with_outputs(vec![out("value", "Value", Vector)]),
        node("data.blank", "Blank", nc)
            .with_description("Empty generic data/image")
            .with_outputs(vec![out("value", "Value", Any)]),
        node("data.vector2", "Vector2", nc)
            .with_description("2D Vector")
            .with_outputs(vec![out("value", "Value", Vec2)]),
        node("data.vector3", "Vector3", nc)
            .with_description("3D Vector")
            .with_outputs(vec![out("value", "Value", Vec3)]),
        node("data.color", "Color", nc)
            .with_description("RGBA Color")
            .with_outputs(vec![out("value", "Value", Color)]),
        node("data.string", "String", nc)
            .with_description("Text string")
            .with_outputs(vec![out("value", "Value", String)]),
        node("data.image", "Image", nc)
            .with_description("Generic Image buffer")
            .with_outputs(vec![out("value", "Value", Image)]),
        node("data.video", "Video", nc)
            .with_description("Video resource")
            .with_inputs(vec![inp("path", "Path", String)])
            .with_outputs(vec![out("output", "Output", Video)]),
        node("data.rgb_image", "RGB Image", nc)
            .with_description("Image in RGB color space")
            .with_outputs(vec![out("output", "Output", Image)]),
        node("data.yuv_image", "YUV Image", nc)
            .with_description("Image in YUV color space")
            .with_outputs(vec![out("output", "Output", Image)]),
        node("data.gradient", "Gradient", nc)
            .with_description("Color gradient (Color Ramp)")
            .with_outputs(vec![out("output", "Output", Gradient)]),
        node("data.curve", "Curve", nc)
            .with_description("1D Value Curve (Profile/Timeline)")
            .with_outputs(vec![out("output", "Output", Curve)]),
        node("data.asset", "Asset", nc)
            .with_inputs(vec![inp("asset_id", "Asset ID", String)])
            .with_outputs(vec![out("output", "Output", Any)]),
    ]
}
