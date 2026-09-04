//! Shared descriptor validation and execution for Ensemble operations.

use std::collections::HashMap;

use crate::error::LibraryError;
use crate::model::project::EvalOutput;
use crate::model::property::PropertyValue;
use crate::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY,
    OperationDescriptor,
};

use super::PluginManager;

impl PluginManager {
    /// Resolve the exact one-Shape contract persisted by a Text Ensemble
    /// operation. Effectors already have that contract; Decorators may expose
    /// a self-contained form while retaining a richer multi-input graph form
    /// under the same component identity.
    pub fn text_ensemble_operation_descriptor(
        &self,
        category: &str,
        component_id: &str,
    ) -> Result<OperationDescriptor, LibraryError> {
        let descriptor = match category {
            EFFECTOR_CATEGORY => self.operation_descriptor(
                EFFECTOR_CATEGORY,
                component_id,
                EFFECTOR_APPLY_OPERATION,
            )?,
            DECORATOR_CATEGORY => self
                .get_decorator_plugin(component_id)
                .ok_or_else(|| {
                    LibraryError::Plugin(format!(
                        "Decorator plugin '{component_id}' is unavailable"
                    ))
                })?
                .inline_text_descriptor()
                .map_err(|error| LibraryError::Plugin(error.to_string()))?
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Decorator '{component_id}' requires Node Editor media inputs"
                    ))
                })?,
            _ => {
                return Err(LibraryError::Validation(format!(
                    "'{category}' is not a Text Ensemble operation category"
                )));
            }
        };
        if !crate::model::authoring::text_ensemble_direct_contract_is_compatible(
            descriptor.declared_ports(),
        ) {
            return Err(LibraryError::Validation(format!(
                "Operation {}/{}/{} requires Node Editor media inputs and cannot run inline on Text",
                descriptor.category(),
                descriptor.component_id(),
                descriptor.operation(),
            )));
        }
        Ok(descriptor)
    }

    /// Construct a Text operation through the same descriptor-owned Node
    /// factory as graph operations, including every typed default.
    pub fn create_text_ensemble_operation_node(
        &self,
        category: &str,
        component_id: &str,
    ) -> Result<crate::model::Node, LibraryError> {
        self.text_ensemble_operation_descriptor(category, component_id)?
            .create_node()
            .map_err(|error| LibraryError::Plugin(error.to_string()))
    }

    /// Evaluates a graph Effector after resolving its connected and authored
    /// properties, then enters the executor shared with Timeline authoring.
    pub fn evaluate_effector_operation(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> EvalOutput<crate::core::ensemble::types::EffectorConfig> {
        let descriptor = match self.operation_descriptor(
            EFFECTOR_CATEGORY,
            component_id,
            EFFECTOR_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Effector plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return EvalOutput::NoOutput;
            }
        };
        let Some(properties) = context.evaluate_operation_properties(
            descriptor.properties(),
            properties,
            eval_time,
            &format!("Effector {component_id}"),
        ) else {
            return EvalOutput::NoOutput;
        };
        self.evaluate_effector_operation_values(
            component_id,
            source_id,
            &properties,
            eval_time,
            context.evaluation_fps(),
            context.evaluation_resolution(),
        )
    }

    /// Executes an Effector from descriptor-validated values sampled by an
    /// authoring runtime.
    pub fn evaluate_effector_operation_values(
        &self,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &HashMap<String, PropertyValue>,
        eval_time: f64,
        fps: f64,
        resolution: (u64, u64),
    ) -> EvalOutput<crate::core::ensemble::types::EffectorConfig> {
        let Some(plugin) = self.get_effector_plugin(component_id) else {
            log::warn!("Effector plugin {component_id} is unavailable; producing NoOutput");
            return EvalOutput::NoOutput;
        };
        let descriptor = match self.operation_descriptor(
            EFFECTOR_CATEGORY,
            component_id,
            EFFECTOR_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Effector plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return EvalOutput::NoOutput;
            }
        };
        let Some(properties) = super::validated_operation_values(
            descriptor.properties(),
            properties,
            &format!("Effector {component_id}"),
        ) else {
            return EvalOutput::NoOutput;
        };
        let Some(context) = super::evaluated_operation(&properties, eval_time, fps, resolution)
        else {
            log::warn!("Effector {component_id} received invalid evaluation context");
            return EvalOutput::NoOutput;
        };
        plugin
            .evaluate_source(&context, source_id)
            .map(EvalOutput::Produced)
            .unwrap_or(EvalOutput::NoOutput)
    }

    /// Evaluates a graph Decorator after resolving its connected and authored
    /// properties, then enters the executor shared with Timeline authoring.
    pub fn evaluate_decorator_operation(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> EvalOutput<crate::core::ensemble::types::DecoratorConfig> {
        let descriptor = match self.operation_descriptor(
            DECORATOR_CATEGORY,
            component_id,
            DECORATOR_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Decorator plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return EvalOutput::NoOutput;
            }
        };
        let Some(properties) = context.evaluate_operation_properties(
            descriptor.properties(),
            properties,
            eval_time,
            &format!("Decorator {component_id}"),
        ) else {
            return EvalOutput::NoOutput;
        };
        self.evaluate_decorator_operation_values(
            component_id,
            source_id,
            &properties,
            eval_time,
            context.evaluation_fps(),
            context.evaluation_resolution(),
        )
    }

    /// Executes a Decorator from descriptor-validated values sampled by an
    /// authoring runtime.
    pub fn evaluate_decorator_operation_values(
        &self,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &HashMap<String, PropertyValue>,
        eval_time: f64,
        fps: f64,
        resolution: (u64, u64),
    ) -> EvalOutput<crate::core::ensemble::types::DecoratorConfig> {
        self.evaluate_decorator_values(
            component_id,
            source_id,
            properties,
            eval_time,
            fps,
            resolution,
            DecoratorExecution::Graph,
        )
    }

    /// Execute the self-contained one-Shape form used by Text sources and by
    /// Node Clips promoted from them.
    pub fn evaluate_text_decorator_operation_values(
        &self,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &HashMap<String, PropertyValue>,
        eval_time: f64,
        fps: f64,
        resolution: (u64, u64),
    ) -> EvalOutput<crate::core::ensemble::types::DecoratorConfig> {
        self.evaluate_decorator_values(
            component_id,
            source_id,
            properties,
            eval_time,
            fps,
            resolution,
            DecoratorExecution::InlineText,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the common operation executor receives identity, sampled values, and evaluation context"
    )]
    fn evaluate_decorator_values(
        &self,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &HashMap<String, PropertyValue>,
        eval_time: f64,
        fps: f64,
        resolution: (u64, u64),
        execution: DecoratorExecution,
    ) -> EvalOutput<crate::core::ensemble::types::DecoratorConfig> {
        let Some(plugin) = self.get_decorator_plugin(component_id) else {
            log::warn!("Decorator plugin {component_id} is unavailable; producing NoOutput");
            return EvalOutput::NoOutput;
        };
        let descriptor = match execution {
            DecoratorExecution::Graph => self.operation_descriptor(
                DECORATOR_CATEGORY,
                component_id,
                DECORATOR_APPLY_OPERATION,
            ),
            DecoratorExecution::InlineText => {
                self.text_ensemble_operation_descriptor(DECORATOR_CATEGORY, component_id)
            }
        };
        let descriptor = match descriptor {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Decorator plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return EvalOutput::NoOutput;
            }
        };
        let Some(properties) = super::validated_operation_values(
            descriptor.properties(),
            properties,
            &format!("Decorator {component_id}"),
        ) else {
            return EvalOutput::NoOutput;
        };
        let Some(context) = super::evaluated_operation(&properties, eval_time, fps, resolution)
        else {
            log::warn!("Decorator {component_id} received invalid evaluation context");
            return EvalOutput::NoOutput;
        };
        let evaluated = match execution {
            DecoratorExecution::Graph => plugin.evaluate_source(&context, source_id),
            DecoratorExecution::InlineText => {
                plugin.evaluate_inline_text_source(&context, source_id)
            }
        };
        evaluated
            .map(EvalOutput::Produced)
            .unwrap_or(EvalOutput::NoOutput)
    }
}

#[derive(Clone, Copy)]
enum DecoratorExecution {
    Graph,
    InlineText,
}
