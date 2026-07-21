use super::super::super::ListContent;
use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, NativeNodeFactory, PortSpec};
use crate::model::project::PortDataType;
use crate::model::project::connection::{
    LIST_INDEX_INPUT_PORT, LIST_INPUT_PORT, LIST_ITEM_OUTPUT_PORT, LIST_ITEMS_INPUT_PORT,
    LIST_LENGTH_OUTPUT_PORT, LIST_OUTPUT_PORT,
};

const MAKE_INPUTS: &[PortSpec] = &[PortSpec::variadic(
    LIST_ITEMS_INPUT_PORT,
    "Item",
    PortDataType::Any,
)];
const LIST_OUTPUT: &[PortSpec] = &[PortSpec::single(
    LIST_OUTPUT_PORT,
    "List",
    PortDataType::List,
)];
const GET_INPUTS: &[PortSpec] = &[
    PortSpec::single(LIST_INPUT_PORT, "List", PortDataType::List),
    PortSpec::single(LIST_INDEX_INPUT_PORT, "Index", PortDataType::Integer),
];
const ITEM_OUTPUT: &[PortSpec] = &[PortSpec::single(
    LIST_ITEM_OUTPUT_PORT,
    "Item",
    PortDataType::Any,
)];
const LENGTH_INPUTS: &[PortSpec] = &[PortSpec::single(
    LIST_INPUT_PORT,
    "List",
    PortDataType::List,
)];
const LENGTH_OUTPUT: &[PortSpec] = &[PortSpec::single(
    LIST_LENGTH_OUTPUT_PORT,
    "Length",
    PortDataType::Integer,
)];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.list.make",
            "Make List",
            "Logic",
            "node_editor.menu.create.list:make",
            &["list", "array", "collect", "ordered", "value"],
        ),
        NativeNodeFactory::List(ListContent::Make),
        MAKE_INPUTS,
        LIST_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.list.get-item",
            "Get List Item",
            "Logic",
            "node_editor.menu.create.list:get_item",
            &["list", "array", "index", "item", "value"],
        ),
        NativeNodeFactory::List(ListContent::GetItem),
        GET_INPUTS,
        ITEM_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.list.length",
            "List Length",
            "Logic",
            "node_editor.menu.create.list:length",
            &["list", "array", "length", "count", "size"],
        ),
        NativeNodeFactory::List(ListContent::Length),
        LENGTH_INPUTS,
        LENGTH_OUTPUT,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
