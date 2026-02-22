use super::{inp, node, out};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::PinDataType;

pub(super) fn math_nodes() -> Vec<NodeTypeDefinition> {
    use PinDataType::*;
    let nc = NodeCategory::Math;
    vec![
        node("math.add", "Add", nc)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.subtract", "Subtract", nc)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.multiply", "Multiply", nc)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.divide", "Divide", nc)
            .with_inputs(vec![inp("a", "A", Any), inp("b", "B", Any)])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.power", "Power", nc)
            .with_inputs(vec![
                inp("base", "Base", Any),
                inp("exponent", "Exponent", Any),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.clamp", "Clamp", nc)
            .with_inputs(vec![
                inp("value", "Value", Any),
                inp("min", "Min", Any),
                inp("max", "Max", Any),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        node("math.remap", "Remap", nc)
            .with_description("Remap value from input range to output range")
            .with_inputs(vec![
                inp("value", "Value", Any),
                inp("in_min", "In Min", Any),
                inp("in_max", "In Max", Any),
                inp("out_min", "Out Min", Any),
                inp("out_max", "Out Max", Any),
            ])
            .with_outputs(vec![out("result", "Result", Any)]),
        // Vector Math
        node("math.dot_product", "Dot Product", nc)
            .with_inputs(vec![inp("a", "A", Vector), inp("b", "B", Vector)])
            .with_outputs(vec![out("result", "Result", Scalar)]),
        node("math.cross_product", "Cross Product", nc)
            .with_inputs(vec![inp("a", "A", Vector), inp("b", "B", Vector)])
            .with_outputs(vec![out("result", "Result", Vector)]),
        node("math.normalize", "Normalize", nc)
            .with_inputs(vec![inp("vector", "Vector", Vector)])
            .with_outputs(vec![out("result", "Result", Vector)]),
    ]
}
