//! Descriptor-backed Text Ensemble evaluation for Timeline sources.

use crate::core::ensemble::EnsembleData;
use crate::error::LibraryError;
use crate::model::authoring::{TextEnsembleOperation, text_ensemble_direct_contract_is_compatible};
use crate::model::project::EvalOutput;
use crate::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY,
    PluginManager,
};

use super::frame_values::evaluate_property_map;

pub(super) fn evaluate_text_ensemble(
    plugins: &PluginManager,
    operations: &[TextEnsembleOperation],
    time: f64,
    fps: f64,
    resolution: (u64, u64),
) -> Result<EvalOutput<Option<EnsembleData>>, LibraryError> {
    if operations.is_empty() {
        return Ok(EvalOutput::Produced(None));
    }
    let mut effectors = Vec::new();
    let mut decorators = Vec::new();
    for operation in operations {
        if !text_ensemble_direct_contract_is_compatible(&operation.declared_ports) {
            return Err(LibraryError::Validation(format!(
                "Text Ensemble operation {} requires unsupported media inputs",
                operation.id
            )));
        }
        let descriptor = match plugins.text_ensemble_operation_descriptor(
            &operation.operation.category,
            &operation.operation.component_id,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Text Ensemble operation {} is unavailable: {error}; producing NoOutput",
                    operation.id
                );
                return Ok(EvalOutput::NoOutput);
            }
        };
        if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
            log::warn!(
                "Text Ensemble operation {} descriptor no longer matches its persisted contract; producing NoOutput",
                operation.id
            );
            return Ok(EvalOutput::NoOutput);
        }
        let values = evaluate_property_map(
            &operation.properties,
            time,
            &format!("Text Ensemble operation {}", operation.id),
        )?;
        match (
            operation.operation.category.as_str(),
            operation.operation.operation.as_str(),
        ) {
            (EFFECTOR_CATEGORY, EFFECTOR_APPLY_OPERATION) => {
                match plugins.evaluate_effector_operation_values(
                    &operation.operation.component_id,
                    operation.id,
                    &values,
                    time,
                    fps,
                    resolution,
                ) {
                    EvalOutput::Produced(config) => effectors.push(config),
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                }
            }
            (DECORATOR_CATEGORY, DECORATOR_APPLY_OPERATION) => {
                match plugins.evaluate_text_decorator_operation_values(
                    &operation.operation.component_id,
                    operation.id,
                    &values,
                    time,
                    fps,
                    resolution,
                ) {
                    EvalOutput::Produced(config) => decorators.push(config),
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                }
            }
            _ => {
                return Err(LibraryError::Validation(format!(
                    "Text Ensemble operation {} has unsupported identity {}/{}/{}",
                    operation.id,
                    operation.operation.category,
                    operation.operation.component_id,
                    operation.operation.operation,
                )));
            }
        }
    }
    Ok(EvalOutput::Produced(Some(EnsembleData {
        enabled: true,
        effector_configs: effectors,
        decorator_configs: decorators,
        patches: std::collections::HashMap::new(),
    })))
}
