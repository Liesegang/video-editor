//! Shared execution boundaries for native Shape transforms and Path Effects.

use std::collections::HashMap;

use crate::model::frame::draw_type::PathEffect;
use crate::model::frame::transform::Transform;
use crate::model::project::{EvalOutput, EvalResult};
use crate::model::property::{PropertyMap, PropertyValue};
use crate::plugin::{
    PATH_EFFECT_APPLY_OPERATION, PATH_EFFECT_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID,
    TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
};

use super::PluginManager;

impl PluginManager {
    /// Evaluate a graph-authored whole-Shape transform after its PropertyMap
    /// and connected scalar inputs have been sampled by the Project runtime.
    pub fn evaluate_transform_operation(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        component_id: &str,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> EvalOutput<Transform> {
        let descriptor = match self.operation_descriptor(
            TRANSFORM_CATEGORY,
            component_id,
            TRANSFORM_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Transform operation {component_id} is unavailable: {error}; producing NoOutput"
                );
                return EvalOutput::NoOutput;
            }
        };
        crate::plugin::transforms::evaluate_source(
            context,
            descriptor.properties(),
            properties,
            eval_time,
        )
        .map(EvalOutput::Produced)
        .unwrap_or(EvalOutput::NoOutput)
    }

    /// Evaluate the same descriptor from values already sampled by a compiled
    /// Module. Percent scale and independent X/Y handling remain owned by the
    /// production Transform implementation.
    pub fn evaluate_shape_transform_operation_values(
        &self,
        component_id: &str,
        values: &HashMap<String, PropertyValue>,
    ) -> EvalOutput<Transform> {
        if component_id != SHAPE_TRANSFORM_COMPONENT_ID {
            log::warn!(
                "Shape Transform operation {component_id} is unavailable; producing NoOutput"
            );
            return EvalOutput::NoOutput;
        }
        let descriptor = match self.operation_descriptor(
            TRANSFORM_CATEGORY,
            component_id,
            TRANSFORM_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Shape Transform operation {component_id} is unavailable: {error}; producing NoOutput"
                );
                return EvalOutput::NoOutput;
            }
        };
        let Some(values) = super::validated_operation_values(
            descriptor.properties(),
            values,
            &format!("Shape Transform {component_id}"),
        ) else {
            return EvalOutput::NoOutput;
        };
        crate::plugin::transforms::transform_from_values(&values)
            .map(EvalOutput::Produced)
            .unwrap_or(EvalOutput::NoOutput)
    }

    /// Evaluate one graph-authored Path Effect after Project property
    /// evaluation, then enter the same value-only plugin boundary as Modules.
    pub fn evaluate_path_effect_operation(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        component_id: &str,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> EvalResult<PathEffect> {
        let descriptor = match self.operation_descriptor(
            PATH_EFFECT_CATEGORY,
            component_id,
            PATH_EFFECT_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Path Effect plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return Ok(EvalOutput::NoOutput);
            }
        };
        let Some(values) = context.evaluate_operation_properties(
            descriptor.properties(),
            properties,
            eval_time,
            &format!("Path Effect {component_id}"),
        ) else {
            return Ok(EvalOutput::NoOutput);
        };
        self.evaluate_path_effect_values(
            component_id,
            &values,
            eval_time,
            context.evaluation_fps(),
            context.evaluation_resolution(),
        )
    }

    /// Evaluate a Path Effect from descriptor-validated values sampled by a
    /// compiled Module runtime.
    pub fn evaluate_path_effect_values(
        &self,
        component_id: &str,
        values: &HashMap<String, PropertyValue>,
        eval_time: f64,
        fps: f64,
        resolution: (u64, u64),
    ) -> EvalResult<PathEffect> {
        let Some(plugin) = self.get_path_effect_plugin(component_id) else {
            log::warn!("Path Effect plugin {component_id} is unavailable; producing NoOutput");
            return Ok(EvalOutput::NoOutput);
        };
        let descriptor = match self.operation_descriptor(
            PATH_EFFECT_CATEGORY,
            component_id,
            PATH_EFFECT_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Path Effect plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return Ok(EvalOutput::NoOutput);
            }
        };
        let Some(values) = super::validated_operation_values(
            descriptor.properties(),
            values,
            &format!("Path Effect {component_id}"),
        ) else {
            return Ok(EvalOutput::NoOutput);
        };
        let Some(context) = super::evaluated_operation(&values, eval_time, fps, resolution) else {
            log::warn!("Path Effect {component_id} received invalid evaluation context");
            return Ok(EvalOutput::NoOutput);
        };
        plugin.evaluate_source(&context).map(EvalOutput::Produced)
    }
}
