//! Runtime evaluation for canonical authored data leaf Nodes.

use std::collections::HashSet;

use uuid::Uuid;

use super::evaluator::{FrameEvaluator, cycle_error, missing_error};
use crate::model::NodeContent;
use crate::model::project::connection::{DATA_VALUE_OUTPUT_PORT, DATA_VALUE_PROPERTY};
use crate::model::project::{EvalOutput, EvalResult, PortOwner};
use crate::model::property::PropertyValue;
use crate::plugin::ResolvedNodeInputs;

impl FrameEvaluator<'_> {
    pub(super) fn evaluate_data_node_output(
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
        let NodeContent::Data(data) = node.content() else {
            return Ok(EvalOutput::NoOutput);
        };
        if !node.enabled || node.bypassed || output_port != DATA_VALUE_OUTPUT_PORT {
            return Ok(EvalOutput::NoOutput);
        }
        let scope = match self.scope_for_owner(owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = (|| {
            let Some(property) = node.properties().get(DATA_VALUE_PROPERTY) else {
                return Ok(EvalOutput::NoOutput);
            };
            let composition = self
                .composition_for_owner(owner)
                .ok_or_else(|| missing_error(owner))?;
            let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
            let value = match self
                .context(composition, Some(&inputs))
                .evaluate_property_value(property, node.properties(), scope.time)
            {
                Ok(value) => value,
                Err(error) => {
                    log::error!("Data Node '{}' value produced no output: {error}", node.id);
                    return Ok(EvalOutput::NoOutput);
                }
            };
            if data.accepts_value(&value) {
                Ok(EvalOutput::Produced(value))
            } else {
                log::error!(
                    "Data Node '{}' expected {:?}, got incompatible property value {:?}",
                    node.id,
                    data,
                    value
                );
                Ok(EvalOutput::NoOutput)
            }
        })();
        path.remove(&owner);
        result
    }
}
