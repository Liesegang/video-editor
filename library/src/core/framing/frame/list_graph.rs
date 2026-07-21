//! Runtime evaluation for heterogeneous List Nodes.

use std::collections::HashSet;

use uuid::Uuid;

use super::evaluator::{FrameEvaluator, cycle_error, missing_error};
use super::scope::EvaluationScope;
use crate::error::LibraryError;
use crate::model::project::connection::{
    LIST_INDEX_INPUT_PORT, LIST_INPUT_PORT, LIST_ITEM_OUTPUT_PORT, LIST_ITEMS_INPUT_PORT,
    LIST_LENGTH_OUTPUT_PORT, LIST_OUTPUT_PORT,
};
use crate::model::project::{
    EvalOutput, EvalResult, PortAddress, PortDirection, PortMultiplicity, PortOwner,
    ProjectConnection,
};
use crate::model::property::PropertyValue;
use crate::model::{ListContent, Node, NodeContent};
use crate::plugin::ResolvedNodeInputs;

impl FrameEvaluator<'_> {
    pub(super) fn evaluate_list_node_output(
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
        let NodeContent::List(operation) = node.content() else {
            return Ok(EvalOutput::NoOutput);
        };
        if !node.enabled || !operation_supports_output(*operation, output_port) {
            return Ok(EvalOutput::NoOutput);
        }
        // List operations have no same-typed, unambiguous passthrough. A
        // forward-loaded invalid bypass flag must remain harmless.
        if node.bypassed {
            return Ok(EvalOutput::NoOutput);
        }
        let scope = match self.scope_for_owner(owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = match operation {
            ListContent::Make => self.evaluate_make_list(node, global_time, path),
            ListContent::GetItem => self.evaluate_get_list_item(node, scope, global_time, path),
            ListContent::Length => self.evaluate_list_length(node, global_time, path),
        };
        path.remove(&owner);
        result
    }

    fn evaluate_make_list(
        &self,
        node: &Node,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let target = PortAddress::new(PortOwner::Node(node.id), LIST_ITEMS_INPUT_PORT);
        let connections = self.ordered_connections_to(&target)?;
        let mut items = Vec::with_capacity(connections.len());
        for connection in connections {
            match self.resolve_metadata_value(&connection.from, global_time, path)? {
                EvalOutput::Produced(value) => items.push(value),
                // A missing element must not silently collapse the sequence
                // and shift every later index.
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            }
        }
        Ok(EvalOutput::Produced(PropertyValue::Array(items)))
    }

    fn evaluate_get_list_item(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let items = match self.resolve_list_input(node, global_time, path)? {
            EvalOutput::Produced(items) => items,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let index = match self.resolve_list_index(node, scope, global_time, path)? {
            EvalOutput::Produced(index) => index,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let Ok(index) = usize::try_from(index) else {
            return Ok(EvalOutput::NoOutput);
        };
        Ok(items
            .get(index)
            .cloned()
            .map_or(EvalOutput::NoOutput, EvalOutput::Produced))
    }

    fn evaluate_list_length(
        &self,
        node: &Node,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let items = match self.resolve_list_input(node, global_time, path)? {
            EvalOutput::Produced(items) => items,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let Ok(length) = i64::try_from(items.len()) else {
            return Ok(EvalOutput::NoOutput);
        };
        Ok(EvalOutput::Produced(PropertyValue::Integer(length)))
    }

    fn resolve_list_input(
        &self,
        node: &Node,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<Vec<PropertyValue>> {
        let target = PortAddress::new(PortOwner::Node(node.id), LIST_INPUT_PORT);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        match self.resolve_metadata_value(&connection.from, global_time, path)? {
            EvalOutput::Produced(PropertyValue::Array(items)) => Ok(EvalOutput::Produced(items)),
            EvalOutput::Produced(_) | EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
        }
    }

    fn resolve_list_index(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<i64> {
        let target = PortAddress::new(PortOwner::Node(node.id), LIST_INDEX_INPUT_PORT);
        let value = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => {
                match self.resolve_metadata_value(&connection.from, global_time, path)? {
                    EvalOutput::Produced(value) => value,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                }
            }
            EvalOutput::NoOutput => {
                let Some(property) = node.properties().get(LIST_INDEX_INPUT_PORT) else {
                    return Ok(EvalOutput::NoOutput);
                };
                let composition = self
                    .composition_for_owner(PortOwner::Node(node.id))
                    .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
                let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
                match self
                    .context(composition, Some(&inputs))
                    .evaluate_property_value(property, node.properties(), scope.time)
                {
                    Ok(value) => value,
                    Err(error) => {
                        log::error!("List Node '{}' index produced no output: {error}", node.id);
                        return Ok(EvalOutput::NoOutput);
                    }
                }
            }
        };
        match value {
            PropertyValue::Integer(index) => Ok(EvalOutput::Produced(index)),
            _ => Ok(EvalOutput::NoOutput),
        }
    }

    fn ordered_connections_to<'a>(
        &'a self,
        target: &PortAddress,
    ) -> Result<Vec<&'a ProjectConnection>, LibraryError> {
        let definition = self
            .project
            .port_definition(target, PortDirection::Input)
            .ok_or_else(|| LibraryError::Validation(format!("Missing input port {target:?}")))?;
        if definition.multiplicity != PortMultiplicity::Variadic {
            return Err(LibraryError::Validation(format!(
                "Expected variadic input port {target:?}"
            )));
        }
        let mut connections = self
            .project
            .connections
            .iter()
            .filter(|connection| &connection.to == target)
            .collect::<Vec<_>>();
        connections.sort_by_key(|connection| (connection.order, connection.id));
        for (expected_order, connection) in connections.iter().enumerate() {
            if connection.order != expected_order as i64 {
                return Err(LibraryError::Validation(format!(
                    "Non-canonical variadic order at {target:?}: expected {expected_order}, got {}",
                    connection.order
                )));
            }
            let errors = self.project.validate_connection(connection);
            if !errors.is_empty() {
                return Err(LibraryError::Validation(
                    errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; "),
                ));
            }
        }
        Ok(connections)
    }
}

fn operation_supports_output(operation: ListContent, output_port: &str) -> bool {
    matches!(
        (operation, output_port),
        (ListContent::Make, LIST_OUTPUT_PORT)
            | (ListContent::GetItem, LIST_ITEM_OUTPUT_PORT)
            | (ListContent::Length, LIST_LENGTH_OUTPUT_PORT)
    )
}
