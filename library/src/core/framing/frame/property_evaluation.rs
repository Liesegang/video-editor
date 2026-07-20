use uuid::Uuid;

use crate::model::project::EvalOutput;
use crate::model::property::PropertyValue;
use crate::plugin::PropertyEvaluationError;

pub(super) fn output(
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
