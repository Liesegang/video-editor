use std::collections::HashSet;

use super::{EvaluationScope, FrameEvaluator, missing_error};
use crate::model::Node;
use crate::model::frame::runtime_shape::RuntimeShape;
use crate::model::project::{BACKGROUND_SHAPE_INPUT_PORT, EvalOutput, EvalResult, PortOwner};

impl FrameEvaluator<'_> {
    pub(super) fn apply_decorator_to_shape(
        &self,
        node: &Node,
        operation: &crate::model::PluginOperationContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
        if !self.operation_contract_matches(operation)? {
            return Ok(EvalOutput::NoOutput);
        }
        let inputs = self.resolve_node_inputs(node.id, scope, global_time)?;
        if inputs
            .properties
            .values()
            .any(|value| value == &EvalOutput::NoOutput)
        {
            return Ok(EvalOutput::NoOutput);
        }
        let shape = match self.pull_shape_input(node.id, global_time, path)? {
            EvalOutput::Produced(shape) => shape,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let context = self.context(composition, Some(&inputs));
        let config = match self.plugin_manager.evaluate_decorator_operation(
            &context,
            &operation.component_id,
            node.id,
            node.properties(),
            scope.time,
        ) {
            EvalOutput::Produced(config) => config,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        match config {
            crate::core::ensemble::types::DecoratorConfig::LegacyBackplate { .. } => {
                let mut shape = shape;
                shape.push_decorator(config);
                Ok(EvalOutput::Produced(shape))
            }
            crate::core::ensemble::types::DecoratorConfig::Backplate { .. } => {
                let background = match self.pull_shape_input_from_port(
                    node.id,
                    BACKGROUND_SHAPE_INPUT_PORT,
                    global_time,
                    path,
                )? {
                    EvalOutput::Produced(shape) => shape,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                };
                Ok(EvalOutput::Produced(shape.into_backplate_geometry(
                    node.id,
                    background,
                    config,
                    scope.time as f32,
                )?))
            }
        }
    }
}
