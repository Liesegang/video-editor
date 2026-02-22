use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn path_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Path;
    vec![
        node("path.offset_path", "Offset Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("offset", "Offset", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.fill_path", "Fill Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("color", "Color", Color),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("path.stroke_path", "Stroke Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("color", "Color", Color),
                inp("width", "Width", Scalar),
            ])
            .with_outputs(vec![out("image", "Image", Image)]),
        node("path.union_path", "Union Path", nc)
            .with_inputs(vec![inp("paths", "Paths", List)])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.trim_path", "Trim Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("start", "Start", Scalar),
                inp("end", "End", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.corner_path", "Corner Path", nc)
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("radius", "Radius", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.discrete_path", "Discrete Path", nc)
            .with_description("Jitter the path (DiscretePathEffect)")
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("segment_length", "Segment Length", Scalar),
                inp("deviation", "Deviation", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
        node("path.simplify_path", "Simplify Path", nc)
            .with_description("Reduce points in path")
            .with_inputs(vec![
                inp("path", "Path", Path),
                inp("tolerance", "Tolerance", Scalar),
            ])
            .with_outputs(vec![out("path", "Path", Path)]),
    ]
}
