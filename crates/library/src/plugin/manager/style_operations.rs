//! Shared descriptor validation and execution for graph and Timeline Styles.

use std::collections::HashMap;

use crate::model::project::EvalOutput;
use crate::model::property::PropertyValue;
use crate::plugin::{STYLE_APPLY_OPERATION, STYLE_CATEGORY};

use super::PluginManager;

impl PluginManager {
    /// Execute a Style from values already sampled by an authoring/runtime
    /// boundary. This is the same plugin implementation used by graph Style
    /// Nodes; only property sampling belongs to the caller.
    pub fn evaluate_style_operation_values(
        &self,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &HashMap<String, PropertyValue>,
        eval_time: f64,
        fps: f64,
        resolution: (u64, u64),
    ) -> EvalOutput<crate::model::frame::entity::StyleConfig> {
        let Some(plugin) = self.get_style_plugin(component_id) else {
            log::warn!("Style plugin {component_id} is unavailable; producing NoOutput");
            return EvalOutput::NoOutput;
        };
        let descriptor = match self.operation_descriptor(
            STYLE_CATEGORY,
            component_id,
            STYLE_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Style plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return EvalOutput::NoOutput;
            }
        };
        let Some(properties) = super::validated_operation_values(
            descriptor.properties(),
            properties,
            &format!("Style {component_id}"),
        ) else {
            return EvalOutput::NoOutput;
        };
        let Some(context) = super::evaluated_operation(&properties, eval_time, fps, resolution)
        else {
            log::warn!("Style {component_id} received invalid evaluation context");
            return EvalOutput::NoOutput;
        };
        plugin
            .evaluate_values(&context, source_id)
            .map(EvalOutput::Produced)
            .unwrap_or(EvalOutput::NoOutput)
    }
}
