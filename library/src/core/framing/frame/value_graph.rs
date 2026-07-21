//! Evaluation of typed metadata and numeric value connections.
//!
//! This module owns connected property inputs, bypass routing, and arithmetic
//! Nodes. Timeline inheritance and local-time derivation remain in `scope`.

use std::collections::HashSet;

use uuid::Uuid;

use super::evaluator::{FrameEvaluator, cycle_error, missing_error};
use super::scope::EvaluationScope;
use crate::error::LibraryError;
use crate::model::numeric::evaluate_numeric_binary;
use crate::model::project::{
    DURATION_PORT, EvalOutput, EvalResult, PortAddress, PortDataType, PortDirection, PortOwner,
    RESOLUTION_PORT, TIME_PORT,
};
use crate::model::property::PropertyValue;
use crate::model::{Node, NodeContent, ValueContent};
use crate::plugin::{PropertyEvaluationError, ResolvedNodeInputs, property_name_from_port};

impl FrameEvaluator<'_> {
    pub(super) fn resolve_node_inputs(
        &self,
        node_id: Uuid,
        scope: EvaluationScope,
        global_time: f64,
    ) -> Result<ResolvedNodeInputs, LibraryError> {
        let mut values = ResolvedNodeInputs::from_metadata(scope.as_inputs());
        let owner = PortOwner::Node(node_id);
        let targets = self
            .project
            .connections
            .iter()
            .filter(|connection| connection.to.owner == owner)
            .map(|connection| connection.to.clone())
            .collect::<HashSet<_>>();
        for target in targets {
            let target_definition = self
                .project
                .port_definition(&target, PortDirection::Input)
                .ok_or_else(|| {
                    LibraryError::Validation(format!("Missing input port {target:?}"))
                })?;
            match target_definition.data_type {
                PortDataType::Image | PortDataType::Shape => continue,
                _ => {}
            }
            if matches!(
                target.port.as_str(),
                TIME_PORT | DURATION_PORT | RESOLUTION_PORT
            ) {
                // Authored scope overrides have already been applied by
                // scope_for_owner. Keeping a second copy in the property map
                // both re-evaluates the graph and obscures which Time is
                // authoritative.
                continue;
            }
            let connection = match self.single_connection_to(&target)? {
                EvalOutput::Produced(connection) => connection,
                EvalOutput::NoOutput => continue,
            };
            let value =
                self.resolve_metadata_value(&connection.from, global_time, &mut HashSet::new())?;
            let logical_key = property_name_from_port(&target.port).unwrap_or(&target.port);
            values.properties.insert(logical_key.to_string(), value);
        }
        Ok(values)
    }

    pub(super) fn resolve_metadata_value(
        &self,
        source: &PortAddress,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let source_node = if let PortOwner::Node(node_id) = source.owner {
            let node = self
                .project
                .get_node(node_id)
                .ok_or_else(|| missing_error(source.owner))?;
            if !node.enabled {
                return Ok(EvalOutput::NoOutput);
            }
            Some(node)
        } else {
            None
        };
        let definition = self
            .project
            .port_definition(source, PortDirection::Output)
            .ok_or_else(|| LibraryError::Validation(format!("Missing output port {source:?}")))?;
        if matches!(
            definition.data_type,
            PortDataType::Image | PortDataType::Audio
        ) {
            return Err(LibraryError::Validation(format!(
                "Typed media port {source:?} cannot be resolved as a value"
            )));
        }
        if let Some(node) = source_node
            && node.bypassed
        {
            if !path.insert(source.owner) {
                return Err(cycle_error(source.owner));
            }
            let result = (|| {
                let input = node.bypass_input_for_output(&source.port).ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Node {} cannot bypass output {:?}: no unambiguous same-typed input",
                        node.id, source.port
                    ))
                })?;
                let target = PortAddress::new(source.owner, input);
                let connection = match self.single_connection_to(&target)? {
                    EvalOutput::Produced(connection) => connection,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                };
                self.resolve_metadata_value(&connection.from, global_time, path)
            })();
            path.remove(&source.owner);
            return result;
        }
        if let Some(NodeContent::CompositionInstance(instance)) = source_node.map(Node::content) {
            return match self.composition_instance_target_scope(
                source.owner.id(),
                instance,
                global_time,
                path,
            )? {
                EvalOutput::Produced(scope) => scope
                    .value(&source.port)
                    .map(EvalOutput::Produced)
                    .ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Unsupported Composition Instance metadata output {source:?}"
                        ))
                    }),
                EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
            };
        }
        if let PortOwner::Node(node_id) = source.owner
            && matches!(source_node.map(Node::content), Some(NodeContent::Value(_)))
        {
            return self.evaluate_value_node_output(node_id, &source.port, global_time, path);
        }
        if let Some(NodeContent::PluginOperation(operation)) = source_node.map(Node::content) {
            let descriptor = match self.plugin_manager.operation_descriptor(
                &operation.category,
                &operation.component_id,
                &operation.operation,
            ) {
                Ok(descriptor) => descriptor,
                Err(_) => return Ok(EvalOutput::NoOutput),
            };
            if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
                return Ok(EvalOutput::NoOutput);
            }
        }
        match self.scope_for_owner(source.owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope
                .value(&source.port)
                .map(EvalOutput::Produced)
                .ok_or_else(|| {
                    LibraryError::Validation(format!("Unsupported value output port {source:?}"))
                }),
            EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
        }
    }

    fn evaluate_value_node_output(
        &self,
        node_id: Uuid,
        output_port: &str,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let owner = PortOwner::Node(node_id);
        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| missing_error(owner))?;
        if !node.enabled {
            return Ok(EvalOutput::NoOutput);
        }
        if output_port != crate::model::project::NUMBER_RESULT_OUTPUT_PORT {
            return Err(LibraryError::Validation(format!(
                "Unsupported value output port {owner:?}.{output_port}"
            )));
        }
        let scope = match self.scope_for_owner(owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = match node.content() {
            NodeContent::Value(value) => {
                self.evaluate_numeric_binary_node(node, *value, scope, global_time, path)
            }
            _ => Ok(EvalOutput::NoOutput),
        };
        path.remove(&owner);
        result
    }

    fn evaluate_numeric_binary_node(
        &self,
        node: &Node,
        value: ValueContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let left = match self.resolve_value_input(
            node,
            value.primary_input(),
            None,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let right = match self.resolve_value_input(
            node,
            value.secondary_input(),
            Some(value.secondary_input()),
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        Ok(
            evaluate_numeric_binary(value.numeric_operation(), &left, &right)
                .map_or(EvalOutput::NoOutput, EvalOutput::Produced),
        )
    }

    fn resolve_value_input(
        &self,
        node: &Node,
        port: &str,
        property_fallback: Option<&str>,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let target = PortAddress::new(PortOwner::Node(node.id), port);
        match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => {
                self.resolve_metadata_value(&connection.from, global_time, path)
            }
            EvalOutput::NoOutput => {
                let Some(property_key) = property_fallback else {
                    return Ok(EvalOutput::NoOutput);
                };
                let Some(property) = node.properties().get(property_key) else {
                    return Ok(EvalOutput::NoOutput);
                };
                let composition = self
                    .composition_for_owner(PortOwner::Node(node.id))
                    .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
                let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
                let context = self.context(composition, Some(&inputs));
                let properties = node.properties();
                let value = context.evaluate_property_value(property, properties, scope.time);
                Ok(property_output(value, node.id, property_key))
            }
        }
    }
}

fn property_output(
    result: Result<PropertyValue, PropertyEvaluationError>,
    node_id: Uuid,
    property_key: &str,
) -> EvalOutput<PropertyValue> {
    match result {
        Ok(value) => EvalOutput::Produced(value),
        Err(error) => {
            log::error!("Node '{node_id}' property '{property_key}' produced no output: {error}");
            EvalOutput::NoOutput
        }
    }
}
