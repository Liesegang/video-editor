use super::{image_filter, inp};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn filter_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    vec![
        image_filter(
            "filters.blur",
            "Blur",
            NodeCategory::Filters,
            vec![
                inp("radius_x", "Radius X", Scalar),
                inp("radius_y", "Radius Y", Scalar),
            ],
        )
        .with_description("Gaussian blur"),
        image_filter(
            "filters.glow",
            "Glow",
            NodeCategory::Filters,
            vec![
                inp("threshold", "Threshold", Scalar),
                inp("radius", "Radius", Scalar),
                inp("intensity", "Intensity", Scalar),
            ],
        ),
        image_filter(
            "filters.drop_shadow",
            "Drop Shadow",
            NodeCategory::Filters,
            vec![
                inp("color", "Color", Color),
                inp("distance", "Distance", Scalar),
                inp("angle", "Angle", Scalar),
                inp("softness", "Softness", Scalar),
            ],
        ),
        // Distortion (grouped with filters for convenience)
        image_filter(
            "distortion.displacement_map",
            "Displacement Map",
            NodeCategory::Distortion,
            vec![
                inp("map", "Map", Image),
                inp("scale_x", "Scale X", Scalar),
                inp("scale_y", "Scale Y", Scalar),
            ],
        ),
    ]
}
