//! Evaluator for style nodes (style.fill, style.stroke).
//!
//! Fill/Stroke nodes receive shape data on `shape_in` and rasterize it
//! with their style properties, producing an image on `image_out`.

use uuid::Uuid;

use crate::error::LibraryError;
use crate::pipeline::context::EvalContext;
use crate::pipeline::evaluator::NodeEvaluator;
use crate::pipeline::output::{PinValue, ShapeData};
use crate::project::node::Node;
use crate::rendering::renderer::Renderer;
use crate::runtime::color::Color;
use crate::runtime::draw_type::{CapType, DrawStyle, JoinType};
use crate::runtime::entity::StyleConfig;
use crate::runtime::transform::Transform;

pub struct StyleEvaluator;

impl NodeEvaluator for StyleEvaluator {
    fn handles(&self) -> &[&str] {
        &["style."]
    }

    fn evaluate(
        &self,
        node_id: Uuid,
        pin_name: &str,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        if pin_name != "image_out" {
            return Ok(PinValue::None);
        }

        // 1. Pull shape data from upstream
        let shape_value = ctx.pull_input_value(node_id, "shape_in")?;
        let shape_data = match shape_value.into_shape() {
            Some(s) => s,
            None => return Ok(PinValue::None),
        };

        // 2. Build StyleConfig from this node's properties
        let style_config = self.build_style_config(node_id, ctx)?;
        let style_config = match style_config {
            Some(s) => s,
            None => return Ok(PinValue::None),
        };

        // 3. Rasterize shape with style
        let identity = Transform::default();
        let output = match shape_data {
            ShapeData::Grouped { groups, .. } => {
                ctx.renderer
                    .rasterize_grouped_shapes(&groups, &[style_config], &identity)?
            }
            ShapeData::Path {
                path_data,
                path_effects,
            } => ctx.renderer.rasterize_shape_layer(
                &path_data,
                &[style_config],
                &path_effects,
                &identity,
            )?,
        };

        Ok(PinValue::Image(output))
    }
}

impl StyleEvaluator {
    /// Build a StyleConfig from the graph node's properties.
    fn build_style_config(
        &self,
        node_id: Uuid,
        ctx: &mut EvalContext,
    ) -> Result<Option<StyleConfig>, LibraryError> {
        let graph_node = match ctx.project.get_node(node_id) {
            Some(Node::Graph(gn)) => gn.clone(),
            _ => return Ok(None),
        };

        let config = match graph_node.type_id.as_str() {
            "style.fill" => {
                let color = ctx.resolve_color(
                    &graph_node.properties,
                    "color",
                    Color {
                        r: 255,
                        g: 255,
                        b: 255,
                        a: 255,
                    },
                );
                let opacity = ctx.resolve_number(&graph_node.properties, "opacity", 100.0);
                let offset = ctx.resolve_number(&graph_node.properties, "offset", 0.0);

                let alpha = ((opacity / 100.0) * 255.0).clamp(0.0, 255.0) as u8;
                StyleConfig {
                    id: node_id,
                    style: DrawStyle::Fill {
                        color: Color {
                            r: color.r,
                            g: color.g,
                            b: color.b,
                            a: alpha,
                        },
                        offset,
                    },
                }
            }
            "style.stroke" => {
                let color = ctx.resolve_color(
                    &graph_node.properties,
                    "color",
                    Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                );
                let width = ctx.resolve_number(&graph_node.properties, "width", 1.0);
                let opacity = ctx.resolve_number(&graph_node.properties, "opacity", 100.0);
                let offset = ctx.resolve_number(&graph_node.properties, "offset", 0.0);
                let join_str = ctx.resolve_string(&graph_node.properties, "join", "round");
                let cap_str = ctx.resolve_string(&graph_node.properties, "cap", "round");
                let miter = ctx.resolve_number(&graph_node.properties, "miter_limit", 4.0);
                let dash_offset = ctx.resolve_number(&graph_node.properties, "dash_offset", 0.0);

                let alpha = ((opacity / 100.0) * 255.0).clamp(0.0, 255.0) as u8;
                let join = match join_str.as_str() {
                    "bevel" => JoinType::Bevel,
                    "miter" => JoinType::Miter,
                    _ => JoinType::Round,
                };
                let cap = match cap_str.as_str() {
                    "square" => CapType::Square,
                    "butt" => CapType::Butt,
                    _ => CapType::Round,
                };

                let dash_array_str = ctx.resolve_string(&graph_node.properties, "dash_array", "");
                let dash_array: Vec<f64> = if dash_array_str.is_empty() {
                    vec![]
                } else {
                    dash_array_str
                        .split(',')
                        .filter_map(|s| s.trim().parse().ok())
                        .collect()
                };

                StyleConfig {
                    id: node_id,
                    style: DrawStyle::Stroke {
                        color: Color {
                            r: color.r,
                            g: color.g,
                            b: color.b,
                            a: alpha,
                        },
                        width,
                        offset,
                        cap,
                        join,
                        miter,
                        dash_array,
                        dash_offset,
                    },
                }
            }
            _ => return Ok(None),
        };

        Ok(Some(config))
    }
}
