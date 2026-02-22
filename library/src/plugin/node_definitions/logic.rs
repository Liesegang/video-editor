use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn logic_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Logic;
    vec![
        node("logic.switch", "Switch", nc)
            .with_inputs(vec![
                inp("condition", "Condition", Boolean),
                inp("true_val", "True", Any),
                inp("false_val", "False", Any),
            ])
            .with_outputs(vec![out("output", "Output", Any)]),
        node("logic.make_list", "Make List", nc)
            .with_description("Create a list from inputs")
            .with_inputs(vec![inp("item", "Item", Any)])
            .with_outputs(vec![out("list", "List", List)]),
        node("logic.get_list_item", "Get List Item", nc)
            .with_inputs(vec![
                inp("list", "List", List),
                inp("index", "Index", Integer),
            ])
            .with_outputs(vec![out("item", "Item", Any)]),
        node("logic.list_length", "List Length", nc)
            .with_inputs(vec![inp("list", "List", List)])
            .with_outputs(vec![out("length", "Length", Integer)]),
    ]
}
