use super::{image_filter, inp, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn color_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Color;
    vec![
        image_filter(
            "color.color_correction",
            "Color Correction",
            nc,
            vec![
                inp("lift", "Lift", Color),
                inp("gamma", "Gamma", Color),
                inp("gain", "Gain", Color),
                inp("offset", "Offset", Color),
            ],
        )
        .with_description("Lift, Gamma, Gain 3-way color correction"),
        image_filter(
            "color.brightness_contrast",
            "Brightness Contrast",
            nc,
            vec![
                inp("brightness", "Brightness", Scalar),
                inp("contrast", "Contrast", Scalar),
            ],
        ),
        image_filter(
            "color.hue_saturation",
            "Hue Saturation",
            nc,
            vec![
                inp("hue", "Hue", Scalar),
                inp("saturation", "Saturation", Scalar),
                inp("lightness", "Lightness", Scalar),
            ],
        ),
        image_filter(
            "color.levels",
            "Levels",
            nc,
            vec![
                inp("black_in", "Black In", Scalar),
                inp("white_in", "White In", Scalar),
                inp("gamma", "Gamma", Scalar),
                inp("black_out", "Black Out", Scalar),
                inp("white_out", "White Out", Scalar),
            ],
        ),
        image_filter("color.invert", "Invert", nc, vec![]),
        image_filter(
            "color.hue_shift",
            "Hue Shift",
            nc,
            vec![inp("shift_degrees", "Shift Degrees", Scalar)],
        ),
        image_filter(
            "color.color_map",
            "Color Map",
            nc,
            vec![
                inp("gradient", "Gradient", Gradient),
                inp("gradient_map", "Gradient Map", Image),
            ],
        )
        .with_description("Map luminance to a color gradient"),
    ]
}
