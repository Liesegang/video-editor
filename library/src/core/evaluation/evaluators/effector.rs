//! Evaluator for effector nodes (effector.*).
//!
//! Effectors operate on `ShapeData::Grouped` via `shape_in`/`shape_out` pins,
//! modifying per-element transforms (translate, rotate, scale, opacity).

use uuid::Uuid;

use crate::core::ensemble::effectors::OpacityMode;
use crate::core::evaluation::context::EvalContext;
use crate::core::evaluation::evaluator::NodeEvaluator;
use crate::core::evaluation::output::{PinValue, ShapeData};
use crate::error::LibraryError;
use crate::model::project::node::Node;

pub struct EffectorEvaluator;

impl NodeEvaluator for EffectorEvaluator {
    fn handles(&self) -> &[&str] {
        &["effector."]
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

                let current_time = ctx.time as f32;

                match graph_node.type_id.as_str() {
                    "effector.transform" => {
                        let tx = ctx.resolve_number(&graph_node.properties, "tx", 0.0) as f32;
                        let ty = ctx.resolve_number(&graph_node.properties, "ty", 0.0) as f32;
                        let sx = ctx.resolve_number(&graph_node.properties, "scale_x", 1.0) as f32;
                        let sy = ctx.resolve_number(&graph_node.properties, "scale_y", 1.0) as f32;
                        let r = ctx.resolve_number(&graph_node.properties, "rotation", 0.0) as f32;

                        for group in &mut groups {
                            group.transform.translate.0 += tx;
                            group.transform.translate.1 += ty;
                            group.transform.rotate += r;
                            group.transform.scale.0 *= sx;
                            group.transform.scale.1 *= sy;
                        }
                    }
                    "effector.step_delay" => {
                        let delay =
                            ctx.resolve_number(&graph_node.properties, "delay", 0.05) as f32;
                        let duration =
                            ctx.resolve_number(&graph_node.properties, "duration", 0.3) as f32;
                        let from_opacity =
                            ctx.resolve_number(&graph_node.properties, "from_opacity", 0.0) as f32;
                        let to_opacity =
                            ctx.resolve_number(&graph_node.properties, "to_opacity", 100.0) as f32;

                        for group in &mut groups {
                            let char_start_time = group.index as f32 * delay;
                            let progress = if current_time < char_start_time {
                                0.0
                            } else if duration <= 0.0 || current_time > char_start_time + duration {
                                1.0
                            } else {
                                (current_time - char_start_time) / duration
                            };
                            let opacity = from_opacity + (to_opacity - from_opacity) * progress;
                            group.transform.opacity *= opacity / 100.0;
                        }
                    }
                    "effector.randomize" => {
                        let seed = ctx.resolve_number(&graph_node.properties, "seed", 42.0) as u64;
                        let amount =
                            ctx.resolve_number(&graph_node.properties, "amount", 1.0) as f32;
                        let tr = ctx.resolve_number(&graph_node.properties, "translate_range", 10.0)
                            as f32;
                        let rr =
                            ctx.resolve_number(&graph_node.properties, "rotate_range", 10.0) as f32;

                        for group in &mut groups {
                            let hash =
                                (seed.wrapping_mul(31).wrapping_add(group.index as u64)) as f32;
                            let rand_tx = ((hash * 12.9898).sin() * 43758.5453).fract();
                            let rand_ty = ((hash * 78.233).sin() * 43758.5453).fract();
                            let rand_rot = ((hash * 39.123).sin() * 43758.5453).fract();

                            group.transform.translate.0 += (rand_tx - 0.5) * tr * amount * 2.0;
                            group.transform.translate.1 += (rand_ty - 0.5) * tr * amount * 2.0;
                            group.transform.rotate += (rand_rot - 0.5) * rr * amount * 2.0;
                        }
                    }
                    "effector.opacity" => {
                        let opacity =
                            ctx.resolve_number(&graph_node.properties, "opacity", 100.0) as f32;
                        let mode_str = ctx.resolve_string(&graph_node.properties, "mode", "set");
                        let mode = match mode_str.as_str() {
                            "multiply" | "Multiply" => OpacityMode::Multiply,
                            "add" | "Add" => OpacityMode::Add,
                            _ => OpacityMode::Set,
                        };

                        for group in &mut groups {
                            match mode {
                                OpacityMode::Set => {
                                    group.transform.opacity = opacity / 100.0;
                                }
                                OpacityMode::Multiply => {
                                    group.transform.opacity *= opacity / 100.0;
                                }
                                OpacityMode::Add => {
                                    group.transform.opacity += opacity / 100.0;
                                    group.transform.opacity =
                                        group.transform.opacity.clamp(0.0, 1.0);
                                }
                            }
                        }
                    }
                    _ => {}
                }

                return Ok(PinValue::Shape(ShapeData::Grouped {
                    groups,
                    bounds,
                    lines,
                    font_info,
                }));
            }
            other => return Ok(PinValue::Shape(other)),
        };
    }
}
