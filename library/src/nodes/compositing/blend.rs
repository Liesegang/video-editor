//! Evaluator for blend nodes (compositing.*_blend).

use uuid::Uuid;

use crate::error::LibraryError;
use crate::pipeline::context::EvalContext;
use crate::pipeline::evaluator::NodeEvaluator;
use crate::pipeline::output::PinValue;
use crate::project::node::Node;
use crate::rendering::renderer::{BlendMode, Renderer};

pub struct BlendEvaluator;

impl NodeEvaluator for BlendEvaluator {
    fn handles(&self) -> &[&str] {
        &[
            "compositing.normal_blend",
            "compositing.multiply_blend",
            "compositing.screen_blend",
            "compositing.overlay_blend",
        ]
    }

    fn evaluate(
        &self,
        node_id: Uuid,
        pin_name: &str,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        if pin_name != "image" {
            return Ok(PinValue::None);
        }

        // Pull foreground and background images
        let bg_value = ctx.pull_input_value(node_id, "background")?;
        let fg_value = ctx.pull_input_value(node_id, "foreground")?;

        let bg_image = match bg_value.into_image() {
            Some(img) => img,
            None => return Ok(PinValue::None),
        };
        let fg_image = match fg_value.into_image() {
            Some(img) => img,
            None => return Ok(PinValue::Image(bg_image)),
        };

        // Get opacity from node properties
        let graph_node = match ctx.project.get_node(node_id) {
            Some(Node::Graph(gn)) => gn.clone(),
            _ => {
                // No graph node: default normal blend at full opacity
                let output =
                    ctx.renderer
                        .blend_images(&bg_image, &fg_image, BlendMode::Normal, 1.0)?;
                return Ok(PinValue::Image(output));
            }
        };

        let opacity = ctx.resolve_number(&graph_node.properties, "opacity", 100.0) / 100.0;

        // Determine blend mode from type_id
        let blend_mode = match graph_node.type_id.as_str() {
            "compositing.multiply_blend" => BlendMode::Multiply,
            "compositing.screen_blend" => BlendMode::Screen,
            "compositing.overlay_blend" => BlendMode::Overlay,
            _ => BlendMode::Normal,
        };

        let output = ctx
            .renderer
            .blend_images(&bg_image, &fg_image, blend_mode, opacity)?;
        Ok(PinValue::Image(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::evaluator::NodeEvaluator;

    #[test]
    fn test_blend_evaluator_handles_types() {
        let evaluator = BlendEvaluator;
        let handles = evaluator.handles();
        assert!(handles.contains(&"compositing.normal_blend"));
        assert!(handles.contains(&"compositing.multiply_blend"));
        assert!(handles.contains(&"compositing.screen_blend"));
        assert!(handles.contains(&"compositing.overlay_blend"));
    }
}
