//! Validation of Published values after definition defaults and instance
//! overrides are projected onto their native Node properties.

use std::collections::HashMap;

use crate::model::node::{NodeContent, native_node_descriptor_for_node};
use crate::model::project::PortDirection;
use crate::model::property::{Property, PropertyValue};

use super::{ModuleDefinition, PublishedParameterId, authored_parameter_value_is_compatible};

impl ModuleDefinition {
    /// Validate one concrete Published value against both its public port type
    /// and the canonical native PropertyDefinition behind its target. This is
    /// shared by definition defaults, instance overrides, and every Timeline
    /// automation key so numeric bounds cannot diverge by storage location.
    pub fn validate_parameter_value(
        &self,
        parameter_id: PublishedParameterId,
        value: &PropertyValue,
    ) -> Result<(), String> {
        let parameter = self
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.id == parameter_id)
            .ok_or_else(|| {
                format!(
                    "Module definition {} has no Published parameter {parameter_id}",
                    self.id
                )
            })?;
        if !authored_parameter_value_is_compatible(parameter.data_type, value) {
            return Err(format!(
                "Published parameter '{}' ({parameter_id}) has an incompatible value",
                parameter.name
            ));
        }
        self.graph
            .port_definition(&parameter.target, PortDirection::Input)
            .map_err(|error| {
                format!("Published parameter {parameter_id} has an invalid target: {error}")
            })?;
        let Some(descriptor) = self
            .graph
            .nodes
            .get(&parameter.target.node_id)
            .and_then(native_node_descriptor_for_node)
        else {
            return Ok(());
        };
        if let Some(property_definition) =
            descriptor.property_definition_for_input(&parameter.target.port)
        {
            property_definition.validate_value(value).map_err(|error| {
                format!(
                    "Published parameter '{}' ({parameter_id}) is invalid: {error}",
                    parameter.name
                )
            })?;
        }
        Ok(())
    }

    /// Validate one complete set of instance overrides and then project its
    /// effective values back onto descriptor-backed native Nodes. This keeps
    /// cross-Property invariants valid after Published parameters replace the
    /// Node's authored fallback values.
    pub fn validate_parameter_overrides(
        &self,
        overrides: &HashMap<PublishedParameterId, PropertyValue>,
    ) -> Result<(), String> {
        for parameter_id in overrides.keys() {
            if !self
                .interface
                .parameters
                .iter()
                .any(|parameter| parameter.id == *parameter_id)
            {
                return Err(format!(
                    "Module definition {} has no Published parameter {parameter_id}",
                    self.id
                ));
            }
        }
        for parameter in &self.interface.parameters {
            let value = overrides
                .get(&parameter.id)
                .unwrap_or(&parameter.default_value);
            self.validate_parameter_value(parameter.id, value)?;
        }

        for node in self.graph.nodes.values() {
            if !matches!(node.content(), NodeContent::NativeOperation(_)) {
                continue;
            }
            let Some(descriptor) = native_node_descriptor_for_node(node) else {
                continue;
            };
            let mut effective = node.properties().clone();
            for parameter in self
                .interface
                .parameters
                .iter()
                .filter(|parameter| parameter.target.node_id == node.id)
            {
                let Some(definition) =
                    descriptor.property_definition_for_input(&parameter.target.port)
                else {
                    continue;
                };
                let value = overrides
                    .get(&parameter.id)
                    .unwrap_or(&parameter.default_value);
                effective.set(
                    definition.name().to_string(),
                    Property::constant(value.clone()),
                );
            }
            descriptor.validate_native_properties(&effective)?;
        }
        Ok(())
    }
}
