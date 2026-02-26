//! Evaluator for compositing.transform nodes.

use uuid::Uuid;

use crate::error::LibraryError;
use crate::pipeline::context::EvalContext;
use crate::pipeline::evaluator::NodeEvaluator;
use crate::pipeline::output::PinValue;
use crate::project::node::Node;
use crate::rendering::renderer::Renderer;
use crate::runtime::transform::{Position, Scale, Transform};

pub struct TransformEvaluator;

impl NodeEvaluator for TransformEvaluator {
    fn handles(&self) -> &[&str] {
        &["compositing.transform"]
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

        // Pull the upstream image
        let input_value = ctx.pull_input_value(node_id, "image_in")?;
        let input_image = match input_value.into_image() {
            Some(img) => img,
            None => return Ok(PinValue::None),
        };

        // Get transform properties from the graph node
        let graph_node = match ctx.project.get_node(node_id) {
            Some(Node::Graph(gn)) => gn.clone(),
            _ => return Ok(PinValue::Image(input_image)),
        };

        let (px, py) = ctx.resolve_vec2(&graph_node.properties, "position", 0.0, 0.0);
        let (ax, ay) = ctx.resolve_vec2(&graph_node.properties, "anchor", 0.0, 0.0);
        let (sx, sy) = ctx.resolve_vec2(&graph_node.properties, "scale", 100.0, 100.0);
        let rotation = ctx.resolve_number(&graph_node.properties, "rotation", 0.0);
        let opacity = ctx.resolve_number(&graph_node.properties, "opacity", 100.0);

        // Apply region offset and render_scale to map composition-space
        // coordinates to the (potentially smaller/offset) renderer surface.
        let (region_offset_x, region_offset_y) = match &ctx.region {
            Some(r) => (-r.x, -r.y),
            None => (0.0, 0.0),
        };
        let scale_factor = ctx.render_scale;

        let transform = Transform {
            position: Position {
                x: (px + region_offset_x) * scale_factor,
                y: (py + region_offset_y) * scale_factor,
            },
            anchor: Position {
                x: ax * scale_factor,
                y: ay * scale_factor,
            },
            scale: Scale {
                x: sx / 100.0,
                y: sy / 100.0,
            },
            rotation,
            opacity: opacity / 100.0,
        };

        // Apply transform on an offscreen surface
        let output = ctx.renderer.transform_layer(&input_image, &transform)?;
        Ok(PinValue::Image(output))
    }
}
