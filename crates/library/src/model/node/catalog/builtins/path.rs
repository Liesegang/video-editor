use super::super::super::PathOperationContent;
use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, NativeNodeFactory, PortSpec};
use crate::model::project::PortDataType;
use crate::model::project::connection::{PATH_OUTPUT_PORT, PATHS_INPUT_PORT};

const UNION_INPUTS: &[PortSpec] = &[PortSpec::single(
    PATHS_INPUT_PORT,
    "Paths",
    PortDataType::List,
)];
const PATH_OUTPUT: &[PortSpec] = &[PortSpec::single(
    PATH_OUTPUT_PORT,
    "Path",
    PortDataType::Path,
)];

const SPECS: &[DescriptorSpec] = &[DescriptorSpec::implemented(
    DescriptorIdentity::new(
        "native.path.union",
        "Union Path",
        "Path",
        "node_editor.menu.create.path:union",
        &["path", "boolean", "union", "combine", "geometry"],
    ),
    NativeNodeFactory::Path(PathOperationContent::Union),
    UNION_INPUTS,
    PATH_OUTPUT,
)];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
