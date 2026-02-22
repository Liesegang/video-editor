use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn scripting_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    vec![
        node(
            "scripting.expression",
            "Expression",
            NodeCategory::Scripting,
        )
        .with_description("Execute custom Python scripts")
        .with_inputs(vec![
            inp("code", "Code", String),
            inp("inputs", "Inputs", List),
        ])
        .with_outputs(vec![out("result", "Result", Any)]),
    ]
}
