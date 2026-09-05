//! Descriptor-backed appearance evaluation for direct Timeline sources.

use crate::error::LibraryError;
use crate::model::authoring::{AppearanceOperation, appearance_direct_contract_is_compatible};
use crate::model::frame::entity::StyleConfig;
use crate::model::project::EvalOutput;
use crate::plugin::{PluginManager, STYLE_APPLY_OPERATION, STYLE_CATEGORY};

use super::frame_values::evaluate_property_map;

pub(super) fn evaluate_appearance(
    plugins: &PluginManager,
    operations: &[AppearanceOperation],
    time: f64,
    fps: f64,
    resolution: (u64, u64),
) -> Result<EvalOutput<Vec<StyleConfig>>, LibraryError> {
    let mut styles = Vec::with_capacity(operations.len());
    for operation in operations {
        if operation.operation.category != STYLE_CATEGORY
            || operation.operation.operation != STYLE_APPLY_OPERATION
            || !appearance_direct_contract_is_compatible(&operation.declared_ports)
        {
            return Err(LibraryError::Validation(format!(
                "Appearance operation {} has an unsupported contract",
                operation.id
            )));
        }
        let descriptor = match plugins.operation_descriptor(
            STYLE_CATEGORY,
            &operation.operation.component_id,
            STYLE_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Appearance operation {} is unavailable: {error}; producing NoOutput",
                    operation.id
                );
                return Ok(EvalOutput::NoOutput);
            }
        };
        if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
            log::warn!(
                "Appearance operation {} descriptor no longer matches its persisted contract; producing NoOutput",
                operation.id
            );
            return Ok(EvalOutput::NoOutput);
        }
        let values = evaluate_property_map(
            &operation.properties,
            time,
            &format!("Appearance operation {}", operation.id),
        )?;
        match plugins.evaluate_style_operation_values(
            &operation.operation.component_id,
            operation.id,
            &values,
            time,
            fps,
            resolution,
        ) {
            EvalOutput::Produced(style) => styles.push(style),
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        }
    }
    Ok(EvalOutput::Produced(styles))
}
