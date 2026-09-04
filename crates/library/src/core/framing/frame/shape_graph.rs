//! Evaluation of Nodes that produce transient Shape output.
//!
//! Generators, Effectors, Path Effects, Decorators, and Shape Transforms stay
//! geometry-only here. Rasterization remains an explicit Style boundary owned
//! by `image_graph`.

use std::collections::HashSet;

use super::evaluator::{FrameEvaluator, cycle_error, missing_error};
use super::scope::EvaluationScope;
use crate::error::LibraryError;
use crate::model::frame::runtime_shape::RuntimeShape;
use crate::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, EvalOutput, EvalResult, PortAddress, PortDataType, PortDirection,
    PortOwner, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use crate::model::{GeneratorContent, Node, NodeContent, PluginOperationContent};
use crate::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY,
    PATH_EFFECT_APPLY_OPERATION, PATH_EFFECT_CATEGORY, TRANSFORM_APPLY_OPERATION,
    TRANSFORM_CATEGORY,
};

impl FrameEvaluator<'_> {
    /// Pull a transient Shape value from an exact output address. Shape values
    /// are never persisted and are cloned only by real graph fan-out.
    pub(super) fn evaluate_shape_output(
        &self,
        source: &PortAddress,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
        let definition = self
            .project
            .port_definition(source, PortDirection::Output)
            .ok_or_else(|| LibraryError::Validation(format!("Missing output port {source:?}")))?;
        if definition.data_type != PortDataType::Shape {
            return Err(LibraryError::Validation(format!(
                "Port {source:?} does not produce Shape"
            )));
        }
        let PortOwner::Node(node_id) = source.owner else {
            return Ok(EvalOutput::NoOutput);
        };
        if source.port != SHAPE_OUTPUT_PORT {
            return Ok(EvalOutput::NoOutput);
        }
        let owner = PortOwner::Node(node_id);
        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| missing_error(owner))?;
        // Disabled is a graph gate. It is checked before cycle detection,
        // scope/Time evaluation, descriptor lookup, properties, or upstream.
        if !node.enabled {
            return Ok(EvalOutput::NoOutput);
        }
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        if node.bypassed {
            let input = node
                .bypass_input_for_output(SHAPE_OUTPUT_PORT)
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Node {} cannot bypass Shape output: no unambiguous same-typed input",
                        node.id
                    ))
                })?;
            let result = self.pull_shape_input_from_port(node.id, input, global_time, path);
            path.remove(&owner);
            return result;
        }
        let result = (|| {
            let scope = match self.scope_for_node(node_id, global_time)? {
                EvalOutput::Produced(scope) => scope,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
            match node.content() {
                NodeContent::Generator(GeneratorContent::Text | GeneratorContent::Shape) => {
                    self.convert_shape_node(node, scope, global_time)
                }
                NodeContent::PluginOperation(operation)
                    if operation.category == EFFECTOR_CATEGORY
                        && operation.operation == EFFECTOR_APPLY_OPERATION =>
                {
                    self.apply_effector_to_shape(node, operation, scope, global_time, path)
                }
                NodeContent::PluginOperation(operation)
                    if operation.category == PATH_EFFECT_CATEGORY
                        && operation.operation == PATH_EFFECT_APPLY_OPERATION =>
                {
                    self.apply_path_effect_to_shape(node, operation, scope, global_time, path)
                }
                NodeContent::PluginOperation(operation)
                    if operation.category == DECORATOR_CATEGORY
                        && operation.operation == DECORATOR_APPLY_OPERATION =>
                {
                    self.apply_decorator_to_shape(node, operation, scope, global_time, path)
                }
                NodeContent::PluginOperation(operation)
                    if operation.category == TRANSFORM_CATEGORY
                        && operation.operation == TRANSFORM_APPLY_OPERATION =>
                {
                    self.apply_root_transform_to_shape(node, operation, scope, global_time, path)
                }
                _ => Ok(EvalOutput::NoOutput),
            }
        })();
        path.remove(&owner);
        result
    }

    fn convert_shape_node(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
    ) -> EvalResult<RuntimeShape> {
        let kind = match node.content() {
            NodeContent::Generator(GeneratorContent::Text) => "text",
            NodeContent::Generator(GeneratorContent::Shape) => "shape",
            _ => return Ok(EvalOutput::NoOutput),
        };
        let inputs = self.resolve_node_inputs(node.id, scope, global_time)?;
        if inputs
            .properties
            .values()
            .any(|value| value == &EvalOutput::NoOutput)
        {
            return Ok(EvalOutput::NoOutput);
        }
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let converter = self
            .plugin_manager
            .get_entity_converter(kind)
            .ok_or_else(|| LibraryError::Plugin(format!("No entity converter for {kind}")))?;
        let context = self.context(composition, Some(&inputs));
        Ok(match converter.convert_shape(&context, node, scope.time) {
            Some(shape) => EvalOutput::Produced(shape),
            None => EvalOutput::NoOutput,
        })
    }

    fn apply_effector_to_shape(
        &self,
        node: &Node,
        operation: &PluginOperationContent,
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
        let mut shape = match self.pull_shape_input(node.id, global_time, path)? {
            EvalOutput::Produced(shape) => shape,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let context = self.context(composition, Some(&inputs));
        let config = match self.plugin_manager.evaluate_effector_operation(
            &context,
            &operation.component_id,
            node.id,
            node.properties(),
            scope.time,
        ) {
            EvalOutput::Produced(config) => config,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        shape.apply_effector(config, scope.time as f32)?;
        Ok(EvalOutput::Produced(shape))
    }

    fn apply_path_effect_to_shape(
        &self,
        node: &Node,
        operation: &PluginOperationContent,
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
        let mut shape = match self.pull_shape_input(node.id, global_time, path)? {
            EvalOutput::Produced(shape) => shape,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let context = self.context(composition, Some(&inputs));
        let effect = match self.plugin_manager.evaluate_path_effect_operation(
            &context,
            &operation.component_id,
            node.properties(),
            scope.time,
        )? {
            EvalOutput::Produced(effect) => effect,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        shape.apply_path_effect(node.id, effect)?;
        Ok(EvalOutput::Produced(shape))
    }

    fn apply_root_transform_to_shape(
        &self,
        node: &Node,
        operation: &PluginOperationContent,
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
        let mut shape = match self.pull_shape_input(node.id, global_time, path)? {
            EvalOutput::Produced(shape) => shape,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let context = self.context(composition, Some(&inputs));
        let transform = match self.plugin_manager.evaluate_transform_operation(
            &context,
            &operation.component_id,
            node.properties(),
            scope.time,
        ) {
            EvalOutput::Produced(transform) => transform,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        shape.set_root_transform(node.id, transform)?;
        Ok(EvalOutput::Produced(shape))
    }

    fn apply_decorator_to_shape(
        &self,
        node: &Node,
        operation: &PluginOperationContent,
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
        // The validated persisted contract distinguishes geometry-only v2
        // from the frozen one-Shape v1 path before plugin code can run.
        let background = if operation.declared_ports.iter().any(|port| {
            port.key == BACKGROUND_SHAPE_INPUT_PORT
                && port.direction == PortDirection::Input
                && port.data_type == PortDataType::Shape
        }) {
            match self.pull_shape_input_from_port(
                node.id,
                BACKGROUND_SHAPE_INPUT_PORT,
                global_time,
                path,
            )? {
                EvalOutput::Produced(shape) => Some(shape),
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            }
        } else {
            None
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
                let Some(background) = background else {
                    return Ok(EvalOutput::NoOutput);
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

    fn pull_shape_input(
        &self,
        node_id: uuid::Uuid,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
        self.pull_shape_input_from_port(node_id, SHAPE_INPUT_PORT, global_time, path)
    }

    fn pull_shape_input_from_port(
        &self,
        node_id: uuid::Uuid,
        port: &str,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
        let target = PortAddress::new(PortOwner::Node(node_id), port);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        self.evaluate_shape_output(&connection.from, global_time, path)
    }
}
