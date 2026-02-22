use super::{image_filter, inp};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn time_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    vec![image_filter(
        "time.time_shift",
        "Time Shift",
        NodeCategory::Time,
        vec![inp("time_offset", "Time Offset", Scalar)],
    )]
}
