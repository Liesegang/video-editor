//! Evaluator for decorator nodes (decorator.*).
//!
//! Decorators operate on `ShapeData::Grouped` via `shape_in`/`shape_out` pins,
//! adding decoration shapes (backplates, etc.) to each element's `decorations` list.

use uuid::Uuid;

use super::svg_builder::build_rect_svg;
use crate::error::LibraryError;
use crate::pipeline::context::EvalContext;
use crate::pipeline::evaluator::NodeEvaluator;
use crate::pipeline::output::{DecorationShape, PinValue, ShapeData};
use crate::pipeline::processing::ensemble::decorators::{BackplateShape, BackplateTarget};
use crate::project::node::Node;
use crate::runtime::color::Color;

pub struct DecoratorEvaluator;

impl NodeEvaluator for DecoratorEvaluator {
    fn handles(&self) -> &[&str] {
        &["decorator."]
    }

    fn evaluate(
        &self,
        node_id: Uuid,
        pin_name: &str,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        if pin_name != "shape_out" {
            return Ok(PinValue::None);
        }

        // Pull upstream shape data
        let shape_value = ctx.pull_input_value(node_id, "shape_in")?;
        let shape_data = match shape_value.into_shape() {
            Some(s) => s,
            None => return Ok(PinValue::None),
        };

        // Only operate on Grouped shapes; pass Path through unchanged
        match shape_data {
            ShapeData::Grouped {
                mut groups,
                bounds,
                lines,
                font_info,
            } => {
                let graph_node = match ctx.project.get_node(node_id) {
                    Some(Node::Graph(gn)) => gn.clone(),
                    _ => {
                        return Ok(PinValue::Shape(ShapeData::Grouped {
                            groups,
                            bounds,
                            lines,
                            font_info,
                        }));
                    }
                };

                match graph_node.type_id.as_str() {
                    "decorator.backplate" => {
                        let target_str =
                            ctx.resolve_string(&graph_node.properties, "target", "char");
                        let shape_str = ctx.resolve_string(&graph_node.properties, "shape", "rect");
                        let color = ctx.resolve_color(
                            &graph_node.properties,
                            "color",
                            Color {
                                r: 0,
                                g: 0,
                                b: 0,
                                a: 128,
                            },
                        );
                        let padding =
                            ctx.resolve_number(&graph_node.properties, "padding", 4.0) as f32;
                        let radius =
                            ctx.resolve_number(&graph_node.properties, "radius", 0.0) as f32;

                        let target = match target_str.as_str() {
                            "line" | "Line" => BackplateTarget::Line,
                            "block" | "Block" => BackplateTarget::Block,
                            "parts" | "Parts" => BackplateTarget::Parts,
                            _ => BackplateTarget::Char,
                        };
                        let backplate_shape = match shape_str.as_str() {
                            "rounded_rect" | "RoundRect" => BackplateShape::RoundedRect,
                            "circle" | "Circle" => BackplateShape::Circle,
                            _ => BackplateShape::Rect,
                        };

                        match target {
                            BackplateTarget::Char => {
                                // Add backplate behind each character
                                for group in &mut groups {
                                    let rect_path = build_rect_svg(
                                        group.base_position.0 - padding,
                                        group.base_position.1 - padding,
                                        group.bounds.2 + padding * 2.0,
                                        group.bounds.3 + padding * 2.0,
                                        &backplate_shape,
                                        radius,
                                    );
                                    group.decorations.push(DecorationShape {
                                        path: rect_path,
                                        color: color.clone(),
                                        behind: true,
                                    });
                                }
                            }
                            BackplateTarget::Line => {
                                // Add backplate behind each line
                                for line_info in &lines {
                                    let rect_path = build_rect_svg(
                                        line_info.bounds.0 - padding,
                                        line_info.bounds.1 - padding,
                                        line_info.bounds.2 + padding * 2.0,
                                        line_info.bounds.3 + padding * 2.0,
                                        &backplate_shape,
                                        radius,
                                    );
                                    // Add to first group of the line
                                    if let Some(first_idx) = line_info.group_range.clone().next() {
                                        if let Some(group) = groups.get_mut(first_idx) {
                                            group.decorations.push(DecorationShape {
                                                path: rect_path,
                                                color: color.clone(),
                                                behind: true,
                                            });
                                        }
                                    }
                                }
                            }
                            BackplateTarget::Block | BackplateTarget::Parts => {
                                // Add backplate behind entire text block
                                let rect_path = build_rect_svg(
                                    bounds.0 - padding,
                                    bounds.1 - padding,
                                    bounds.2 + padding * 2.0,
                                    bounds.3 + padding * 2.0,
                                    &backplate_shape,
                                    radius,
                                );
                                if let Some(first_group) = groups.first_mut() {
                                    first_group.decorations.push(DecorationShape {
                                        path: rect_path,
                                        color: color.clone(),
                                        behind: true,
                                    });
                                }
                            }
                        }
                    }
                    _ => {}
                }

                Ok(PinValue::Shape(ShapeData::Grouped {
                    groups,
                    bounds,
                    lines,
                    font_info,
                }))
            }
            other => Ok(PinValue::Shape(other)),
        }
    }
}
