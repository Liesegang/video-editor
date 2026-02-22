use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn text_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Text;
    vec![
        node("text.text", "Text", nc)
            .with_inputs(vec![
                inp("string", "String", String),
                inp("font", "Font", Font),
                inp("size", "Size", Scalar),
                inp("italic", "Italic", Boolean),
                inp("bold", "Bold", Scalar),
                inp("split_mode", "Split Mode", Enum),
                inp("sort_order", "Sort Order", Enum),
            ])
            .with_outputs(vec![
                out("path", "Path", Path),
                out("line_index", "Line Index", List),
                out("char_index", "Char Index", List),
                out("stroke_index", "Stroke Index", List),
            ]),
        node("text.join_strings", "Join Strings", nc)
            .with_inputs(vec![
                inp("strings", "Strings", List),
                inp("separator", "Separator", String),
            ])
            .with_outputs(vec![out("result", "Result", String)]),
        node("text.replace_string", "Replace String", nc)
            .with_inputs(vec![
                inp("source", "Source", String),
                inp("from", "From", String),
                inp("to", "To", String),
            ])
            .with_outputs(vec![out("result", "Result", String)]),
    ]
}
