//! Evaluator for effect nodes (effect.*).

use std::collections::HashMap;

use uuid::Uuid;

use crate::error::LibraryError;
use crate::pipeline::context::EvalContext;
use crate::pipeline::evaluator::NodeEvaluator;
use crate::pipeline::output::PinValue;
use crate::project::node::Node;
use crate::project::property::PropertyValue;
use crate::rendering::renderer::Renderer;

pub struct EffectEvaluator;

impl NodeEvaluator for EffectEvaluator {
    fn handles(&self) -> &[&str] {
        &["effect."]
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

        // Get the graph node to determine effect type and properties
        let graph_node = match ctx.project.get_node(node_id) {
            Some(Node::Graph(gn)) => gn.clone(),
            _ => return Ok(PinValue::Image(input_image)),
        };

        // Extract the effect name from type_id (strip "effect." prefix)
        let effect_name = graph_node
            .type_id
            .strip_prefix("effect.")
            .unwrap_or(&graph_node.type_id);

        // Evaluate all properties
        let mut params: HashMap<String, PropertyValue> = HashMap::new();
        for (key, _prop) in graph_node.properties.iter() {
            let value =
                ctx.resolve_property_value(&graph_node.properties, key, PropertyValue::from(0.0));
            params.insert(key.to_string(), value);
        }

        // Inject u_time
        params.insert(
            "u_time".to_string(),
            PropertyValue::Number(ordered_float::OrderedFloat(ctx.time)),
        );

        // Apply the effect via plugin manager
        let gpu_context = ctx.renderer.get_gpu_context();
        let output =
            ctx.plugin_manager
                .apply_effect(effect_name, &input_image, &params, gpu_context)?;

        Ok(PinValue::Image(output))
    }
}
