use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn generator_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Generator;
    vec![
        node("generators.solid_color", "Solid Color", nc)
            .with_inputs(vec![inp("color", "Color", Color)])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("generators.linear_gradient", "Linear Gradient", nc)
            .with_inputs(vec![
                inp("start_point", "Start Point", Vec2),
                inp("end_point", "End Point", Vec2),
                inp("gradient", "Gradient", Gradient),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("generators.radial_gradient", "Radial Gradient", nc)
            .with_inputs(vec![
                inp("center", "Center", Vec2),
                inp("radius", "Radius", Scalar),
                inp("gradient", "Gradient", Gradient),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("generators.construct_gradient", "Construct Gradient", nc)
            .with_description("Create a gradient from color stops")
            .with_inputs(vec![
                inp("colors", "Colors", List),
                inp("positions", "Positions", List),
            ])
            .with_outputs(vec![out("gradient", "Gradient", Gradient)]),
        node("generators.sample_gradient", "Sample Gradient", nc)
            .with_inputs(vec![
                inp("gradient", "Gradient", Gradient),
                inp("time", "Time", Scalar),
            ])
            .with_outputs(vec![out("color", "Color", Color)]),
        node("generators.evaluate_curve", "Evaluate Curve", nc)
            .with_inputs(vec![
                inp("curve", "Curve", Curve),
                inp("time", "Time", Scalar),
            ])
            .with_outputs(vec![out("value", "Value", Scalar)]),
        node("generators.noise", "Noise", nc)
            .with_inputs(vec![
                inp("scale", "Scale", Scalar),
                inp("seed", "Seed", Integer),
                inp("evolution", "Evolution", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
    ]
}
