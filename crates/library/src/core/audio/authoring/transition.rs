//! Timeline-owned Audio Crossfade evaluation for the production audio mixer.

use std::sync::Arc;

use ordered_float::OrderedFloat;

use super::*;
use crate::core::render_plan::{
    CompiledModuleDefinition, CompiledModuleInvocation, CompiledTransition,
};
use crate::model::authoring::{
    InstancePath, ModuleHostContract, TransitionId, TransitionMediaType, TransitionModuleInterface,
};
use crate::model::node::{
    NodeContent, TRANSITION_AUDIO_INPUT_NODE_ID, TRANSITION_AUDIO_MIX_NODE_ID,
    TRANSITION_PROGRESS_INPUT_NODE_ID, ValueContent,
};
use crate::model::project::connection::{DATA_VALUE_OUTPUT_PORT, DATA_VALUE_PROPERTY};
use crate::model::project::{
    AUDIO_OUTPUT_PORT, MERGE_SOUNDS_PORT, NUMBER_RESULT_OUTPUT_PORT, PortDataType,
    TRANSITION_FROM_INPUT_PORT, TRANSITION_PROGRESS_INPUT_PORT, TRANSITION_TO_INPUT_PORT,
};
use crate::model::property::{Property, PropertyValue};

pub(super) type AudioTransitionIndex =
    HashMap<(TimelineId, TimelineItemId), Vec<AudioTransitionParticipant>>;

#[derive(Clone)]
pub(super) struct AudioTransitionParticipant {
    transition_index: usize,
    is_to: bool,
    programs: Arc<AudioTransitionPrograms>,
}

#[derive(Clone)]
struct AudioTransitionPrograms {
    base: Arc<AudioTransitionProgram>,
    instances: HashMap<InstancePath, Arc<AudioTransitionProgram>>,
}

impl AudioTransitionPrograms {
    fn for_path(&self, instance_path: Option<&InstancePath>) -> &AudioTransitionProgram {
        instance_path
            .and_then(|path| self.instances.get(path))
            .map_or(self.base.as_ref(), AsRef::as_ref)
    }
}

#[derive(Clone, Copy)]
pub(super) struct ActiveAudioTransition {
    pub(super) id: TransitionId,
    pub(super) gain: f32,
}

pub(super) fn build_audio_transition_index(
    plan: &RenderPlan,
) -> Result<AudioTransitionIndex, AuthoringAudioError> {
    let mut index = HashMap::new();
    for (timeline_id, timeline) in &plan.timelines {
        for (transition_index, transition) in timeline.transitions.iter().enumerate() {
            if transition.processor.contract.media_type != TransitionMediaType::Audio {
                continue;
            }
            let base = Arc::new(compile_audio_transition_program(plan, transition, None)?);
            let instances = plan
                .transition_instance_controls
                .keys()
                .filter(|target| target.transition_id == transition.id)
                .map(|target| {
                    compile_audio_transition_program(plan, transition, Some(&target.instance_path))
                        .map(|program| (target.instance_path.clone(), Arc::new(program)))
                })
                .collect::<Result<HashMap<_, _>, _>>()?;
            let programs = Arc::new(AudioTransitionPrograms { base, instances });
            index
                .entry((*timeline_id, transition.from.item_id))
                .or_insert_with(Vec::new)
                .push(AudioTransitionParticipant {
                    transition_index,
                    is_to: false,
                    programs: Arc::clone(&programs),
                });
            index
                .entry((*timeline_id, transition.to.item_id))
                .or_insert_with(Vec::new)
                .push(AudioTransitionParticipant {
                    transition_index,
                    is_to: true,
                    programs: Arc::clone(&programs),
                });
        }
    }
    Ok(index)
}

#[derive(Clone)]
enum AudioTransitionProgram {
    LinearCrossfade,
    Module(AudioExpression),
}

#[derive(Clone)]
enum AudioExpression {
    Silence,
    From,
    To,
    Sum(Vec<Self>),
    Crossfade {
        from: Box<Self>,
        to: Box<Self>,
        progress: ValueExpression,
    },
}

#[derive(Clone)]
enum ValueExpression {
    Progress,
    Authored {
        node_id: Uuid,
        key: String,
        property: Property,
    },
    Constant(PropertyValue),
    Automation(crate::model::authoring::AutomationTrack),
    Binary {
        node_id: Uuid,
        operation: ValueContent,
        left: Box<Self>,
        right: Box<Self>,
    },
}

#[derive(Clone, Copy, Default)]
struct AudioWeights {
    from: f32,
    to: f32,
}

fn compile_audio_transition_program(
    plan: &RenderPlan,
    transition: &CompiledTransition,
    instance_path: Option<&InstancePath>,
) -> Result<AudioTransitionProgram, AuthoringAudioError> {
    if transition.processor.is_builtin_audio_crossfade() {
        return Ok(AudioTransitionProgram::LinearCrossfade);
    }
    if transition.processor.module_processor().is_none() {
        let identity = transition.processor.operation().map_or_else(
            || "unknown processor".to_string(),
            |operation| format!("{}@{}", operation.component_id, operation.version),
        );
        return Err(unsupported(
            transition.id,
            format!("'{identity}' has no registered Audio runtime"),
        ));
    }
    let host = transition.module_host.ok_or_else(|| {
        AuthoringAudioError::InvalidSchedule(format!(
            "Audio Transition {} has no compiled Module host",
            transition.id
        ))
    })?;
    let invocation = match instance_path {
        Some(instance_path) => plan.effective_transition_invocation(host, instance_path),
        None => plan.invocation(host).cloned(),
    }
    .ok_or_else(|| {
        AuthoringAudioError::InvalidSchedule(format!(
            "Audio Transition {} has no compiled Module invocation",
            transition.id
        ))
    })?;
    let definition = plan
        .module_definitions
        .get(&invocation.definition_id)
        .ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!(
                "Audio Transition {} has no compiled Module definition {}",
                transition.id, invocation.definition_id
            ))
        })?;
    let ModuleHostContract::Transition(contract) = &definition.host_contract else {
        return Err(unsupported(
            transition.id,
            "Module has no Transition host contract".into(),
        ));
    };
    if contract.media_type != TransitionMediaType::Audio
        || invocation.output_id != contract.output_id
    {
        return Err(unsupported(
            transition.id,
            "Module does not expose its protected Audio Output".into(),
        ));
    }
    if !invocation.input_bindings.is_empty() {
        return Err(unsupported(
            transition.id,
            "Audio Transition Module extra Published media inputs are not supported by the current mixer"
                .into(),
        ));
    }
    let output = definition.outputs.get(&contract.output_id).ok_or_else(|| {
        AuthoringAudioError::InvalidSchedule(format!(
            "Audio Transition {} has no compiled protected Output",
            transition.id
        ))
    })?;
    let source = output.source(PortDataType::Audio).ok_or_else(|| {
        unsupported(
            transition.id,
            "Module protected Output has no Audio source".into(),
        )
    })?;
    let mut compiler = AudioTransitionModuleCompiler {
        transition_id: transition.id,
        definition,
        invocation: &invocation,
        contract,
        audio_path: HashSet::new(),
        value_path: HashSet::new(),
    };
    compiler
        .audio_output(source)
        .map(AudioTransitionProgram::Module)
}

struct AudioTransitionModuleCompiler<'a> {
    transition_id: TransitionId,
    definition: &'a CompiledModuleDefinition,
    invocation: &'a CompiledModuleInvocation,
    contract: &'a TransitionModuleInterface,
    audio_path: HashSet<(Uuid, String)>,
    value_path: HashSet<(Uuid, String)>,
}

impl AudioTransitionModuleCompiler<'_> {
    fn audio_output(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<AudioExpression, AuthoringAudioError> {
        let key = (source.node_id, source.port.clone());
        if !self.audio_path.insert(key.clone()) {
            return Err(unsupported(
                self.transition_id,
                format!("Audio graph cycles at {}:{}", source.node_id, source.port),
            ));
        }
        let result = self.audio_output_inner(source);
        self.audio_path.remove(&key);
        result
    }

    fn audio_output_inner(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<AudioExpression, AuthoringAudioError> {
        let node = self.definition.nodes.get(&source.node_id).ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!(
                "Audio Transition {} reaches missing Module Node {}",
                self.transition_id, source.node_id
            ))
        })?;
        if !node.enabled {
            return Ok(AudioExpression::Silence);
        }
        if node.bypassed {
            let input_port = if matches!(node.content, NodeContent::SoundMerge) {
                MERGE_SOUNDS_PORT
            } else {
                node.bypass_routes.get(&source.port).ok_or_else(|| {
                    unsupported(
                        self.transition_id,
                        format!("Node {} has no Audio bypass route", node.id),
                    )
                })?
            };
            return self
                .audio_inputs(node.id, input_port)?
                .into_iter()
                .next()
                .map_or(Ok(AudioExpression::Silence), |input| {
                    self.audio_output(&input)
                });
        }
        if source.port != AUDIO_OUTPUT_PORT {
            return Err(unsupported(
                self.transition_id,
                format!("Node {} output '{}' is not Audio", node.id, source.port),
            ));
        }
        match &node.content {
            NodeContent::NativeOperation(operation)
                if operation.catalog_id == TRANSITION_AUDIO_INPUT_NODE_ID =>
            {
                if node.id == self.source_a_target()?.node_id {
                    Ok(AudioExpression::From)
                } else if node.id == self.source_b_target()?.node_id {
                    Ok(AudioExpression::To)
                } else {
                    Err(unsupported(
                        self.transition_id,
                        format!("Node {} is not a protected A/B boundary", node.id),
                    ))
                }
            }
            NodeContent::NativeOperation(operation)
                if operation.catalog_id == TRANSITION_AUDIO_MIX_NODE_ID =>
            {
                let from = self.required_audio_input(node.id, TRANSITION_FROM_INPUT_PORT)?;
                let to = self.required_audio_input(node.id, TRANSITION_TO_INPUT_PORT)?;
                let progress = self.value_input(node.id, TRANSITION_PROGRESS_INPUT_PORT)?;
                Ok(AudioExpression::Crossfade {
                    from: Box::new(self.audio_output(&from)?),
                    to: Box::new(self.audio_output(&to)?),
                    progress,
                })
            }
            NodeContent::SoundMerge => {
                let inputs = self.audio_inputs(node.id, MERGE_SOUNDS_PORT)?;
                let expressions = inputs
                    .iter()
                    .map(|input| self.audio_output(input))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(AudioExpression::Sum(expressions))
            }
            _ => Err(unsupported(
                self.transition_id,
                format!(
                    "Module Node {} ({:?}) has no supported Audio Transition runtime",
                    node.id, node.content
                ),
            )),
        }
    }

    fn required_audio_input(
        &self,
        node_id: Uuid,
        port: &str,
    ) -> Result<ModulePortAddress, AuthoringAudioError> {
        let inputs = self.audio_inputs(node_id, port)?;
        let [input] = inputs.as_slice() else {
            return Err(unsupported(
                self.transition_id,
                format!("Audio input {node_id}:{port} must have exactly one source"),
            ));
        };
        Ok(input.clone())
    }

    fn audio_inputs(
        &self,
        node_id: Uuid,
        port: &str,
    ) -> Result<Vec<ModulePortAddress>, AuthoringAudioError> {
        let target = ModulePortAddress {
            node_id,
            port: port.to_string(),
        };
        let mut inputs = self
            .definition
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| (connection.order, connection.id, connection.from.clone()))
            .collect::<Vec<_>>();
        inputs.sort_by_key(|(order, id, _)| (*order, *id));
        Ok(inputs.into_iter().map(|(_, _, source)| source).collect())
    }

    fn value_output(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<ValueExpression, AuthoringAudioError> {
        let key = (source.node_id, source.port.clone());
        if !self.value_path.insert(key.clone()) {
            return Err(unsupported(
                self.transition_id,
                format!("value graph cycles at {}:{}", source.node_id, source.port),
            ));
        }
        let result = self.value_output_inner(source);
        self.value_path.remove(&key);
        result
    }

    fn value_output_inner(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<ValueExpression, AuthoringAudioError> {
        let node = self.definition.nodes.get(&source.node_id).ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!(
                "Audio Transition {} value reaches missing Node {}",
                self.transition_id, source.node_id
            ))
        })?;
        if !node.enabled {
            return Err(unsupported(
                self.transition_id,
                format!("disabled value Node {} produces no Progress", node.id),
            ));
        }
        if node.bypassed {
            let port = node.bypass_routes.get(&source.port).ok_or_else(|| {
                unsupported(
                    self.transition_id,
                    format!("Node {} has no value bypass route", node.id),
                )
            })?;
            return self.value_input(node.id, port);
        }
        match node.content {
            NodeContent::NativeOperation(ref operation)
                if operation.catalog_id == TRANSITION_PROGRESS_INPUT_NODE_ID
                    && source.port == NUMBER_RESULT_OUTPUT_PORT =>
            {
                self.value_input(node.id, TRANSITION_PROGRESS_INPUT_PORT)
            }
            NodeContent::Value(operation) if source.port == NUMBER_RESULT_OUTPUT_PORT => {
                Ok(ValueExpression::Binary {
                    node_id: node.id,
                    operation,
                    left: Box::new(self.value_input(node.id, operation.primary_input())?),
                    right: Box::new(self.value_input(node.id, operation.secondary_input())?),
                })
            }
            NodeContent::Data(_) if source.port == DATA_VALUE_OUTPUT_PORT => {
                self.value_input(node.id, DATA_VALUE_PROPERTY)
            }
            _ => Err(unsupported(
                self.transition_id,
                format!(
                    "Module Node {}:{} has no supported Progress runtime",
                    node.id, source.port
                ),
            )),
        }
    }

    fn value_input(
        &mut self,
        node_id: Uuid,
        port: &str,
    ) -> Result<ValueExpression, AuthoringAudioError> {
        if let Some(parameter) =
            self.definition.parameters.values().find(|parameter| {
                parameter.target.node_id == node_id && parameter.target.port == port
            })
        {
            if parameter.id == self.contract.progress_parameter_id {
                return Ok(ValueExpression::Progress);
            }
            if let Some(track) = self.invocation.automation_tracks.get(&parameter.id) {
                return Ok(ValueExpression::Automation(track.clone()));
            }
            return Ok(ValueExpression::Constant(
                self.invocation
                    .parameter_overrides
                    .get(&parameter.id)
                    .unwrap_or(&parameter.default_value)
                    .clone(),
            ));
        }
        let target = ModulePortAddress {
            node_id,
            port: port.to_string(),
        };
        let mut connections = self
            .definition
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .collect::<Vec<_>>();
        connections.sort_by_key(|connection| (connection.order, connection.id));
        if connections.len() > 1 {
            return Err(unsupported(
                self.transition_id,
                format!("value input {node_id}:{port} has multiple sources"),
            ));
        }
        if let Some(connection) = connections.first() {
            return self.value_output(&connection.from);
        }
        let node = self.definition.nodes.get(&node_id).ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!(
                "Audio Transition {} has no Module Node {node_id}",
                self.transition_id
            ))
        })?;
        let property_name = crate::plugin::property_name_from_port(port).unwrap_or(port);
        let property = node.properties.get(property_name).ok_or_else(|| {
            unsupported(
                self.transition_id,
                format!("value input {node_id}:{port} has no source or authored value"),
            )
        })?;
        Ok(ValueExpression::Authored {
            node_id,
            key: property_name.to_string(),
            property: property.clone(),
        })
    }

    fn source_a_target(&self) -> Result<&ModulePortAddress, AuthoringAudioError> {
        self.definition
            .media_inputs
            .get(&self.contract.from_input_id)
            .map(|input| &input.target)
            .ok_or_else(|| {
                AuthoringAudioError::InvalidSchedule(format!(
                    "Audio Transition {} has no protected A input",
                    self.transition_id
                ))
            })
    }

    fn source_b_target(&self) -> Result<&ModulePortAddress, AuthoringAudioError> {
        self.definition
            .media_inputs
            .get(&self.contract.to_input_id)
            .map(|input| &input.target)
            .ok_or_else(|| {
                AuthoringAudioError::InvalidSchedule(format!(
                    "Audio Transition {} has no protected B input",
                    self.transition_id
                ))
            })
    }
}

impl AudioTransitionProgram {
    fn weights(
        &self,
        progress: f32,
        local_time: MediaTime,
        transition_id: TransitionId,
    ) -> Result<AudioWeights, AuthoringAudioError> {
        match self {
            Self::LinearCrossfade => Ok(AudioWeights {
                from: 1.0 - progress,
                to: progress,
            }),
            Self::Module(expression) => expression.evaluate(progress, local_time, transition_id),
        }
    }
}

impl AudioExpression {
    fn evaluate(
        &self,
        progress: f32,
        local_time: MediaTime,
        transition_id: TransitionId,
    ) -> Result<AudioWeights, AuthoringAudioError> {
        let weights = match self {
            Self::Silence => AudioWeights::default(),
            Self::From => AudioWeights { from: 1.0, to: 0.0 },
            Self::To => AudioWeights { from: 0.0, to: 1.0 },
            Self::Sum(inputs) => {
                inputs
                    .iter()
                    .try_fold(AudioWeights::default(), |sum, input| {
                        input
                            .evaluate(progress, local_time, transition_id)
                            .map(|value| AudioWeights {
                                from: sum.from + value.from,
                                to: sum.to + value.to,
                            })
                    })?
            }
            Self::Crossfade {
                from,
                to,
                progress: expression,
            } => {
                let value = expression.evaluate(progress, local_time, transition_id)?;
                let PropertyValue::Number(value) = value else {
                    return Err(evaluation_error(
                        transition_id,
                        "Audio Crossfade Progress did not evaluate to Number".into(),
                    ));
                };
                if !value.is_finite() {
                    return Err(evaluation_error(
                        transition_id,
                        "Audio Crossfade Progress is not finite".into(),
                    ));
                }
                let factor = value.into_inner().clamp(0.0, 1.0) as f32;
                let from = from.evaluate(progress, local_time, transition_id)?;
                let to = to.evaluate(progress, local_time, transition_id)?;
                AudioWeights {
                    from: from.from * (1.0 - factor) + to.from * factor,
                    to: from.to * (1.0 - factor) + to.to * factor,
                }
            }
        };
        if !weights.from.is_finite() || !weights.to.is_finite() {
            return Err(evaluation_error(
                transition_id,
                "Audio Transition produced non-finite source weights".into(),
            ));
        }
        Ok(weights)
    }
}

impl ValueExpression {
    fn evaluate(
        &self,
        progress: f32,
        local_time: MediaTime,
        transition_id: TransitionId,
    ) -> Result<PropertyValue, AuthoringAudioError> {
        match self {
            Self::Progress => Ok(PropertyValue::Number(OrderedFloat(f64::from(progress)))),
            Self::Constant(value) => Ok(value.clone()),
            Self::Automation(track) => track
                .evaluate_at(local_time)
                .map_err(|message| evaluation_error(transition_id, message.to_string())),
            Self::Authored {
                node_id,
                key,
                property,
            } => property
                .evaluate_at(local_time.to_seconds_f64())
                .map_err(|error| {
                    evaluation_error(
                        transition_id,
                        format!("cannot evaluate Module Node {node_id} property '{key}': {error}"),
                    )
                }),
            Self::Binary {
                node_id,
                operation,
                left,
                right,
            } => crate::model::numeric::evaluate_numeric_binary(
                operation.numeric_operation(),
                &left.evaluate(progress, local_time, transition_id)?,
                &right.evaluate(progress, local_time, transition_id)?,
            )
            .map_err(|error| {
                evaluation_error(
                    transition_id,
                    format!("numeric Module Node {node_id} failed: {error:?}"),
                )
            }),
        }
    }
}

fn unsupported(transition_id: TransitionId, reason: String) -> AuthoringAudioError {
    AuthoringAudioError::UnsupportedTransitionProcessor {
        transition_id: transition_id.as_uuid(),
        reason,
    }
}

fn evaluation_error(transition_id: TransitionId, message: String) -> AuthoringAudioError {
    AuthoringAudioError::TransitionProcessorEvaluation {
        transition_id: transition_id.as_uuid(),
        message,
    }
}

impl AuthoringAudioMixer<'_> {
    pub(super) fn active_audio_transition(
        &self,
        timeline_id: TimelineId,
        item_id: TimelineItemId,
        timeline_time: MediaTime,
        instance_path: Option<&InstancePath>,
    ) -> Result<Option<ActiveAudioTransition>, AuthoringAudioError> {
        let compiled = self.plan.timelines.get(&timeline_id).ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!(
                "RenderPlan has no Timeline {timeline_id}"
            ))
        })?;
        let Some(participants) = self.audio_transition_index.get(&(timeline_id, item_id)) else {
            return Ok(None);
        };
        let mut active = None;
        for participant in participants {
            let transition = &compiled.transitions[participant.transition_index];
            if !transition
                .progress
                .interval()
                .contains(timeline_time)
                .map_err(schedule_error)?
            {
                continue;
            }
            if active.is_some() {
                return Err(AuthoringAudioError::InvalidSchedule(format!(
                    "Timeline item {item_id} participates in multiple active Audio transitions"
                )));
            }
            let progress = transition
                .progress
                .sample_at(timeline_time)
                .map_err(schedule_error)? as f32;
            let local_time = timeline_time
                .checked_sub(transition.progress.interval().start)
                .map_err(schedule_error)?;
            let weights = participant.programs.for_path(instance_path).weights(
                progress,
                local_time,
                transition.id,
            )?;
            active = Some(ActiveAudioTransition {
                id: transition.id,
                gain: if participant.is_to {
                    weights.to
                } else {
                    weights.from
                },
            });
        }
        Ok(active)
    }
}
