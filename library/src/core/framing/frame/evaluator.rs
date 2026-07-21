//! Shared immutable dependencies and validation boundaries for frame graphs.
//!
//! Typed traversal is intentionally absent from this module. It keeps every
//! graph evaluator attached to the same authoritative Project and centralizes
//! only contracts that all typed domains must enforce.

use std::sync::Arc;

use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::model::project::{
    Composition, EvalOutput, EvalResult, PortAddress, PortDirection, PortMultiplicity, PortOwner,
    Project, ProjectConnection,
};
use crate::plugin::{
    FrameEvaluationContext, PluginManager, PropertyEvaluatorRegistry, ResolvedNodeInputs,
};

/// Evaluates every typed graph domain against one authoritative [`Project`].
///
/// Domain-specific traversal lives in the sibling `*_graph` modules. This
/// type owns only their shared immutable dependencies and graph-contract
/// checks, so those evaluators cannot drift onto copied intermediate models.
pub struct FrameEvaluator<'a> {
    pub(super) project: &'a Project,
    pub(super) composition: &'a Composition,
    pub(super) property_evaluators: Arc<PropertyEvaluatorRegistry>,
    pub(super) plugin_manager: &'a PluginManager,
}

impl<'a> FrameEvaluator<'a> {
    pub fn new(
        project: &'a Project,
        composition: &'a Composition,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
        plugin_manager: &'a PluginManager,
    ) -> Self {
        Self {
            project,
            composition,
            property_evaluators,
            plugin_manager,
        }
    }

    pub(super) fn single_connection_to<'b>(
        &'b self,
        target: &PortAddress,
    ) -> EvalResult<&'b ProjectConnection> {
        let connections = self
            .project
            .connections
            .iter()
            .filter(|connection| &connection.to == target)
            .collect::<Vec<_>>();
        if connections.is_empty() {
            return Ok(EvalOutput::NoOutput);
        }
        let definition = self
            .project
            .port_definition(target, PortDirection::Input)
            .ok_or_else(|| LibraryError::Validation(format!("Missing input port {target:?}")))?;
        if definition.multiplicity != PortMultiplicity::Single || connections.len() != 1 {
            return Err(LibraryError::Validation(format!(
                "Expected one connection to {target:?}, got {}",
                connections.len()
            )));
        }
        let connection = connections[0];
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
        Ok(EvalOutput::Produced(connection))
    }

    pub(super) fn composition_for_owner(&self, owner: PortOwner) -> Option<&Composition> {
        let id = match owner {
            PortOwner::Composition(id) => id,
            PortOwner::Track(id) => self.project.find_composition_for_track(id)?,
            PortOwner::Clip(id) | PortOwner::Node(id) => {
                self.project.find_containing_composition(id)?
            }
        };
        self.project.get_composition(id)
    }

    pub(super) fn context<'b>(
        &'b self,
        composition: &'b Composition,
        inputs: Option<&'b ResolvedNodeInputs>,
    ) -> FrameEvaluationContext<'b> {
        FrameEvaluationContext {
            project: self.project,
            composition,
            property_evaluators: &self.property_evaluators,
            plugin_manager: self.plugin_manager,
            resolved_inputs: inputs,
        }
    }

    pub(super) fn operation_contract_matches(
        &self,
        operation: &crate::model::PluginOperationContent,
    ) -> Result<bool, LibraryError> {
        let descriptor = match self.plugin_manager.operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Unavailable operation {}/{}/{}: {error}; producing NoOutput",
                    operation.category,
                    operation.component_id,
                    operation.operation
                );
                return Ok(false);
            }
        };
        Ok(descriptor.is_execution_compatible_with_ports(&operation.declared_ports))
    }
}

pub(super) fn missing_error(owner: PortOwner) -> LibraryError {
    LibraryError::Project(format!("Graph owner {owner:?} not found"))
}

pub(super) fn cycle_error(owner: PortOwner) -> LibraryError {
    LibraryError::Validation(format!("Evaluation cycle at {owner:?}"))
}

pub(super) fn transparent_background() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}
