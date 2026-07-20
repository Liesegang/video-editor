use crate::model::property::{Property, PropertyMap, PropertyValue};
use crate::plugin::{EvaluationContext, PropertyEvaluationError, PropertyEvaluationOutcome};

use super::ProjectManager;

impl ProjectManager {
    pub fn evaluate_property_value(
        &self,
        property: &Property,
        context: &PropertyMap,
        time: f64,
        fps: f64,
        resolution: (u64, u64),
    ) -> Result<PropertyValue, PropertyEvaluationError> {
        let evaluation_context = EvaluationContext::new(context, fps, resolution);
        self.plugin_manager
            .get_property_evaluators()
            .evaluate(property, time, &evaluation_context)
    }

    pub fn evaluate_property_with_diagnostics(
        &self,
        property: &Property,
        context: &PropertyMap,
        time: f64,
        fps: f64,
        resolution: (u64, u64),
    ) -> Result<PropertyEvaluationOutcome, PropertyEvaluationError> {
        let evaluation_context = EvaluationContext::new(context, fps, resolution);
        self.plugin_manager
            .get_property_evaluators()
            .evaluate_with_diagnostics(property, time, &evaluation_context)
    }
}
