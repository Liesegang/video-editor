//! Point-field dependency classification and consumer validation.

use std::collections::{HashMap, HashSet};

use crate::model::authoring::{ModuleDefinition, ModulePortAddress};
use crate::model::node::{
    COLOR_RAMP_FACTOR_PORT, ColorContent, ConditionalNodeRole, NUMERIC_LENGTH_CATALOG_ID,
    NUMERIC_LENGTH_INPUT_PORT, NodeContent, POINT_ATTRIBUTE_OUTPUT_PORT,
    POINT_ATTRIBUTE_VALUE_PORT, POINT_OFFSET_INPUT_PORT, POINT_POSITION_INPUT_PORT,
    POINT_SCALE_INPUT_PORT, POINT_SELECTION_INPUT_PORT, POINT_SIZE_PORT, PointNodeRole,
    SELECT_FALSE_INPUT_PORT, SELECT_TRUE_INPUT_PORT,
};
use crate::model::project::{PortDataType, PortDirection};

use super::renderer::supports_point_field_input;
use super::{
    POINT_AGE_OUTPUT_PORT, POINT_NORMALIZED_AGE_OUTPUT_PORT, POINT_POSITION_OUTPUT_PORT,
    POINT_RANDOM_OUTPUT_PORT, address, point_role, single_input_source,
};

/// Reject a varying Point value before the stateless value runtime can mistake
/// it for one frame-wide PropertyValue. Dead editor branches remain harmless.
pub(in crate::core::render_plan) fn validate_point_field_consumers(
    definition: &ModuleDefinition,
    active_nodes: &HashSet<uuid::Uuid>,
) -> Result<(), String> {
    let mut dependencies = PointDependencyResolver::new(definition);
    for connection in &definition.graph.connections {
        if !active_nodes.contains(&connection.to.node_id)
            || !dependencies.depends_on_point(&connection.from)
        {
            continue;
        }
        let target = definition
            .graph
            .nodes
            .get(&connection.to.node_id)
            .ok_or_else(|| "Point field reaches a missing consumer".to_string())?;
        let supported = match target.content() {
            NodeContent::Value(operation) => {
                connection.to.port == operation.primary_input()
                    || connection.to.port == operation.secondary_input()
            }
            NodeContent::Color(ColorContent::ColorRamp) => {
                connection.to.port == COLOR_RAMP_FACTOR_PORT
            }
            NodeContent::NativeOperation(operation) => {
                if let Some(role) = ConditionalNodeRole::from_catalog_id(&operation.catalog_id) {
                    match role {
                        ConditionalNodeRole::Compare(_) => {
                            matches!(connection.to.port.as_str(), "a" | "b")
                        }
                        ConditionalNodeRole::Select(_) => matches!(
                            connection.to.port.as_str(),
                            crate::model::node::CONDITION_INPUT_PORT
                                | SELECT_TRUE_INPUT_PORT
                                | SELECT_FALSE_INPUT_PORT
                        ),
                    }
                } else if operation.catalog_id == NUMERIC_LENGTH_CATALOG_ID {
                    connection.to.port == NUMERIC_LENGTH_INPUT_PORT
                } else {
                    match point_role(target) {
                        Some(PointNodeRole::StoreAttribute(_)) => {
                            connection.to.port == POINT_ATTRIBUTE_VALUE_PORT
                        }
                        Some(PointNodeRole::SetPosition) => matches!(
                            connection.to.port.as_str(),
                            POINT_POSITION_INPUT_PORT
                                | POINT_OFFSET_INPUT_PORT
                                | POINT_SELECTION_INPUT_PORT
                        ),
                        Some(PointNodeRole::SetSize) => matches!(
                            connection.to.port.as_str(),
                            POINT_SIZE_PORT | POINT_SCALE_INPUT_PORT | POINT_SELECTION_INPUT_PORT
                        ),
                        Some(PointNodeRole::Info | PointNodeRole::Grid) => false,
                        None => supports_point_field_input(target, &connection.to.port),
                    }
                }
            }
            _ => false,
        };
        if !supported {
            return Err(format!(
                "Per-Point value {}:{} cannot drive unsupported input {}:{}",
                connection.from.node_id,
                connection.from.port,
                connection.to.node_id,
                connection.to.port
            ));
        }
    }
    Ok(())
}

pub(super) struct PointDependencyResolver<'a> {
    definition: &'a ModuleDefinition,
    memo: HashMap<ModulePortAddress, bool>,
    visiting: HashSet<ModulePortAddress>,
}

impl<'a> PointDependencyResolver<'a> {
    pub(super) fn new(definition: &'a ModuleDefinition) -> Self {
        Self {
            definition,
            memo: HashMap::new(),
            visiting: HashSet::new(),
        }
    }

    pub(super) fn depends_on_point(&mut self, source: &ModulePortAddress) -> bool {
        if let Some(result) = self.memo.get(source) {
            return *result;
        }
        if !self.visiting.insert(source.clone()) {
            return true;
        }
        let result = self.depends_on_point_inner(source);
        self.visiting.remove(source);
        self.memo.insert(source.clone(), result);
        result
    }

    fn depends_on_point_inner(&mut self, source: &ModulePortAddress) -> bool {
        let Ok(port) = self
            .definition
            .graph
            .port_definition(source, PortDirection::Output)
        else {
            return false;
        };
        if !matches!(
            port.data_type,
            PortDataType::Number
                | PortDataType::Boolean
                | PortDataType::Integer
                | PortDataType::Numeric
                | PortDataType::Vec2
                | PortDataType::Vec3
                | PortDataType::Vec4
                | PortDataType::Color
        ) {
            return false;
        }
        let Some(node) = self.definition.graph.nodes.get(&source.node_id).cloned() else {
            return false;
        };
        if point_role(&node).is_some_and(|role| match role {
            PointNodeRole::Info => matches!(
                source.port.as_str(),
                POINT_AGE_OUTPUT_PORT
                    | POINT_NORMALIZED_AGE_OUTPUT_PORT
                    | POINT_RANDOM_OUTPUT_PORT
                    | POINT_POSITION_OUTPUT_PORT
                    | POINT_SIZE_PORT
            ),
            PointNodeRole::StoreAttribute(_) => source.port == POINT_ATTRIBUTE_OUTPUT_PORT,
            PointNodeRole::Grid | PointNodeRole::SetPosition | PointNodeRole::SetSize => false,
        }) {
            return true;
        }
        if node.bypassed
            && let Some(input) = node.bypass_input_for_output(&source.port)
        {
            return single_input_source(self.definition, &address(node.id, input))
                .is_some_and(|source| self.depends_on_point(&source));
        }
        let inputs = self
            .definition
            .graph
            .connections
            .iter()
            .filter(|connection| connection.to.node_id == node.id)
            .map(|connection| connection.from.clone())
            .collect::<Vec<_>>();
        inputs.iter().any(|source| self.depends_on_point(source))
    }
}
