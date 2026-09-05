//! Execution of finite, Node-authored Image Transition Modules.
//!
//! Timeline ownership stays outside this runtime: the host injects evaluated
//! A/B handles and normalized Progress through the Module's published
//! interface, while only the reusable processing topology is Node-authored.

use super::frame_values::neutralize_root_blend;
use super::module_image::{
    ModuleImageRuntime, TransitionImageContext, TransitionImageSourceContext,
};
use super::*;
use crate::core::render_plan::CompiledTransition;
use crate::model::authoring::{ModuleHostContract, TransitionMediaType};
use crate::model::frame::entity::{FrameTransitionSource, NormalizedProgress16};

pub(super) struct TransitionModuleImageRequest<'a> {
    pub(super) timeline_id: TimelineId,
    pub(super) timeline_time: MediaTime,
    pub(super) instance_path: &'a InstancePath,
    pub(super) transition: &'a CompiledTransition,
    pub(super) progress: NormalizedProgress16,
    pub(super) from: FrameTransitionSource,
    pub(super) to: FrameTransitionSource,
}

impl AuthoringFrameEvaluator<'_> {
    pub(super) fn evaluate_transition_module_image(
        &mut self,
        request: TransitionModuleImageRequest<'_>,
    ) -> Result<FrameItem, LibraryError> {
        let TransitionModuleImageRequest {
            timeline_id,
            timeline_time,
            instance_path,
            transition,
            progress,
            from,
            to,
        } = request;
        let host = transition.module_host.ok_or_else(|| {
            LibraryError::Validation(format!(
                "Transition {} has a Module processor but no compiled Module host",
                transition.id
            ))
        })?;
        let expected_host = ModuleHost::Transition {
            timeline_id,
            transition_id: transition.id,
        };
        if host != expected_host {
            return Err(LibraryError::Validation(format!(
                "Transition {} compiled Module host does not match its Timeline placement",
                transition.id
            )));
        }
        let invocation = self
            .plan
            .effective_transition_invocation(host, instance_path)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan has no Module invocation for Transition {}",
                    transition.id
                ))
            })?;
        let definition = self
            .plan
            .module_definitions
            .get(&invocation.definition_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan has no Module definition {} for Transition {}",
                    invocation.definition_id, transition.id
                ))
            })?;
        let ModuleHostContract::Transition(contract) = &definition.host_contract else {
            return Err(LibraryError::Validation(format!(
                "Transition {} invocation selects a general-purpose Module",
                transition.id
            )));
        };
        if contract.media_type != TransitionMediaType::Image {
            return Err(LibraryError::Render(format!(
                "Transition {} selects an Audio Module in the Image runtime",
                transition.id
            )));
        }
        if invocation.output_id != contract.output_id {
            return Err(LibraryError::Validation(format!(
                "Transition {} invocation does not select its protected Output",
                transition.id
            )));
        }
        let output = definition.outputs.get(&contract.output_id).ok_or_else(|| {
            LibraryError::Validation(format!(
                "Transition {} Module has no compiled protected Output",
                transition.id
            ))
        })?;
        let from_input = definition
            .media_inputs
            .get(&contract.from_input_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Transition {} Module has no compiled A input",
                    transition.id
                ))
            })?;
        let to_input = definition
            .media_inputs
            .get(&contract.to_input_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Transition {} Module has no compiled B input",
                    transition.id
                ))
            })?;
        let timeline = self.project.timelines.get(&timeline_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline {timeline_id} does not exist"))
        })?;
        let local_time = timeline_time
            .checked_sub(transition.progress.interval().start)
            .map_err(LibraryError::Validation)?;
        let context = TransitionImageContext {
            transition_id: transition.id.as_uuid(),
            timeline_time: OrderedFloat(timeline_time.to_seconds_f64()),
            from: TransitionImageSourceContext {
                item_id: from.item_id,
                source_time: from.source_time,
            },
            to: TransitionImageSourceContext {
                item_id: to.item_id,
                source_time: to.source_time,
            },
        };
        let mut external_images = HashMap::new();
        for (input_id, binding) in &invocation.input_bindings {
            if *input_id == contract.from_input_id || *input_id == contract.to_input_id {
                return Err(LibraryError::Validation(format!(
                    "Transition {} invocation cannot bind its host-owned A/B inputs",
                    transition.id
                )));
            }
            let input = definition.media_inputs.get(input_id).ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Transition {} binds unpublished media input {input_id}",
                    transition.id
                ))
            })?;
            if input.data_type != PortDataType::Image {
                return Err(LibraryError::Render(format!(
                    "Transition Module input {input_id} has {:?} media type; the Image Transition runtime accepts only additional Image inputs",
                    input.data_type
                )));
            }
            if !output.reachable_media_inputs.contains(input_id) {
                continue;
            }
            if let Some(mut frame) =
                self.evaluate_media_binding(binding, timeline_id, timeline_time, instance_path)?
            {
                // Published media inputs are isolated Module sources. Their
                // Timeline placement blend belongs to their original schedule
                // slot and must not be evaluated against transparent here.
                neutralize_root_blend(&mut frame);
                external_images.insert(input.target.clone(), frame);
            }
        }
        external_images.insert(from_input.target.clone(), from.item);
        external_images.insert(to_input.target.clone(), to.item);
        for input in definition
            .media_inputs
            .values()
            .filter(|input| input.required && output.reachable_media_inputs.contains(&input.id))
        {
            if !external_images.contains_key(&input.target) {
                return Err(LibraryError::Render(format!(
                    "Transition {} required media input {} produced no frame",
                    transition.id, input.id
                )));
            }
        }
        let mut host_parameters = HashMap::new();
        host_parameters.insert(
            contract.progress_parameter_id,
            PropertyValue::Number(OrderedFloat(f64::from(progress.as_f32()))),
        );
        let mut runtime = ModuleImageRuntime::new(
            self.project,
            &definition,
            &invocation,
            instance_path,
            local_time,
            timeline.width,
            timeline.height,
            timeline.fps.to_f64(),
            self.plugins,
            external_images,
            host_parameters,
            Some(context),
        );
        let output = runtime.evaluate_terminal(output)?.ok_or_else(|| {
            LibraryError::Render(format!(
                "Transition {} Module produced no Image output",
                transition.id
            ))
        })?;
        Ok(FrameItem::Group(FrameGroup {
            source_id: transition.id.as_uuid(),
            kind: FrameGroupKind::TransitionOutput,
            width: timeline.width,
            height: timeline.height,
            background_color: transparent(),
            transform: Transform::default(),
            blend_mode: transition.output_blend_mode,
            effect_time: OrderedFloat(local_time.to_seconds_f64()),
            effects: Vec::new(),
            items: vec![output],
        }))
    }
}
