use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::model::authoring::{
    AttachmentProcessor, AuthoringProject, MediaInputBinding, MediaOutputKind, ModuleDefinition,
    ModuleDefinitionId, ModuleHostContract, ModuleInvocation, ModulePortAddress, SourceRef,
    TimelineId, TimelineItemId, Transition,
};
use crate::model::project::connection::{
    AUDIO_OUTPUT_PORT, MERGE_SOUNDS_PORT, PortDataType, PortDirection,
};

use super::{
    CompiledModuleDefinition, CompiledModuleInvocation, CompiledModuleOutput, CompiledNode,
    CompiledTimeline, CompiledTransition, DependencyIndex, ModuleHost,
    NormalizedTransitionProgress, PlannedSource, RenderCapability, RenderPlan, ScheduledItem,
    TimelineRangeDependency, TransitionHandleRequirement, TransitionSourceInvocation,
};

mod transition_instances;
use transition_instances::compile_transition_instance_controls;

pub struct RenderPlanCompiler;

impl RenderPlanCompiler {
    pub fn compile(project: &AuthoringProject) -> Result<RenderPlan, String> {
        project.validate()?;
        validate_nested_timelines(project)?;

        let referenced = referenced_definitions(project)?;
        let definitions = referenced
            .into_iter()
            .map(|id| {
                let definition = project
                    .module_definitions
                    .get(&id)
                    .ok_or_else(|| format!("Missing Module definition {id}"))?;
                compile_module(definition).map(|compiled| (id, Arc::new(compiled)))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        let timelines = project
            .timelines
            .keys()
            .copied()
            .map(|id| compile_timeline(project, id).map(|timeline| (id, Arc::new(timeline))))
            .collect::<Result<HashMap<_, _>, _>>()?;
        Self::assemble(project, timelines, definitions)
    }

    pub(super) fn assemble(
        project: &AuthoringProject,
        timelines: HashMap<TimelineId, Arc<CompiledTimeline>>,
        module_definitions: HashMap<ModuleDefinitionId, Arc<CompiledModuleDefinition>>,
    ) -> Result<RenderPlan, String> {
        project.validate()?;
        validate_nested_timelines(project)?;

        let mut invocations = Vec::new();
        let mut dependencies = DependencyIndex::default();
        let mut timeline_ids = timelines.keys().copied().collect::<Vec<_>>();
        timeline_ids.sort();
        for timeline_id in timeline_ids {
            let timeline = timelines
                .get(&timeline_id)
                .ok_or_else(|| format!("Compiled Timeline {timeline_id} disappeared"))?;
            for scheduled in &timeline.schedule {
                dependencies.timeline_ranges.insert(
                    scheduled.item_id,
                    TimelineRangeDependency {
                        timeline_id,
                        start: scheduled.interval.start,
                        duration: scheduled.interval.duration,
                    },
                );
                if scheduled.source != PlannedSource::Module {
                    continue;
                }
                let item = project.items.get(&scheduled.item_id).ok_or_else(|| {
                    format!(
                        "RenderPlan schedule refers to missing item {}",
                        scheduled.item_id
                    )
                })?;
                let SourceRef::Module(invocation) = &item.source else {
                    return Err(format!(
                        "RenderPlan schedule source is stale for item {}",
                        scheduled.item_id
                    ));
                };
                register_invocation(
                    project,
                    &module_definitions,
                    &mut invocations,
                    &mut dependencies,
                    ModuleHost::TimelineItem {
                        timeline_id,
                        item_id: item.id,
                    },
                    invocation,
                )?;
            }
            for compiled_transition in &timeline.transitions {
                let transition = project
                    .transitions
                    .get(&compiled_transition.id)
                    .ok_or_else(|| {
                        format!(
                            "Compiled Timeline refers to missing Transition {}",
                            compiled_transition.id
                        )
                    })?;
                register_transition_invocation(
                    project,
                    &module_definitions,
                    &mut invocations,
                    &mut dependencies,
                    transition,
                )?;
            }
        }

        let mut attachments = project.attachments.values().collect::<Vec<_>>();
        attachments.sort_by_key(|attachment| (attachment.stage, attachment.order, attachment.id));
        for attachment in attachments {
            if let AttachmentProcessor::Module(invocation) = &attachment.processor {
                register_invocation(
                    project,
                    &module_definitions,
                    &mut invocations,
                    &mut dependencies,
                    ModuleHost::Attachment(attachment.id),
                    invocation,
                )?;
            }
        }

        let transition_instance_controls = compile_transition_instance_controls(
            project,
            &module_definitions,
            &invocations,
            &mut dependencies,
        )?;
        Ok(RenderPlan {
            root_timeline_id: project.root_timeline_id,
            timelines,
            module_definitions,
            module_invocations: invocations,
            transition_instance_controls,
            dependencies,
        })
    }
}

fn register_transition_invocation(
    project: &AuthoringProject,
    definitions: &HashMap<ModuleDefinitionId, Arc<CompiledModuleDefinition>>,
    invocations: &mut Vec<CompiledModuleInvocation>,
    dependencies: &mut DependencyIndex,
    transition: &Transition,
) -> Result<(), String> {
    let Some(module) = transition.processor.module_processor() else {
        return Ok(());
    };
    let instance = project
        .module_instances
        .get(&module.instance_id)
        .ok_or_else(|| format!("Missing Module instance {}", module.instance_id))?;
    let definition = definitions.get(&instance.definition_id).ok_or_else(|| {
        format!(
            "Missing compiled Transition Module definition {}",
            instance.definition_id
        )
    })?;
    let ModuleHostContract::Transition(contract) = &definition.host_contract else {
        return Err(format!(
            "Transition {} references a non-Transition Module definition",
            transition.id
        ));
    };
    let host = ModuleHost::Transition {
        timeline_id: transition.timeline_id,
        transition_id: transition.id,
    };
    let interval = transition.interval()?;
    dependencies.host_ranges.insert(
        host,
        TimelineRangeDependency {
            timeline_id: transition.timeline_id,
            start: interval.start,
            duration: interval.duration,
        },
    );
    let invocation = ModuleInvocation {
        instance_id: module.instance_id,
        output_id: contract.output_id,
        input_bindings: module.input_bindings.clone(),
        automation_tracks: module.automation_tracks.clone(),
    };
    register_invocation(
        project,
        definitions,
        invocations,
        dependencies,
        host,
        &invocation,
    )?;
    for item_id in [transition.from_item_id, transition.to_item_id] {
        dependencies
            .media_input_consumers
            .entry(item_id)
            .or_default()
            .push(host);
    }
    Ok(())
}

fn register_invocation(
    project: &AuthoringProject,
    definitions: &HashMap<ModuleDefinitionId, Arc<CompiledModuleDefinition>>,
    invocations: &mut Vec<CompiledModuleInvocation>,
    dependencies: &mut DependencyIndex,
    host: ModuleHost,
    authored: &crate::model::authoring::ModuleInvocation,
) -> Result<(), String> {
    let instance = project
        .module_instances
        .get(&authored.instance_id)
        .ok_or_else(|| format!("Missing Module instance {}", authored.instance_id))?;
    let definition = definitions.get(&instance.definition_id).ok_or_else(|| {
        format!(
            "Missing compiled Module definition {} for instance {}",
            instance.definition_id, instance.id
        )
    })?;
    let output = definition.outputs.get(&authored.output_id).ok_or_else(|| {
        format!(
            "Module instance {} selects missing Output terminal {}",
            instance.id, authored.output_id
        )
    })?;
    validate_invocation_inputs(host, authored, definition)?;

    let index = invocations.len();
    if let ModuleHost::TimelineItem { item_id, .. } = host
        && let Some(range) = dependencies.timeline_ranges.get(&item_id).copied()
    {
        dependencies.host_ranges.insert(host, range);
    }
    if dependencies
        .invocation_indices
        .insert(host, index)
        .is_some()
    {
        return Err(format!(
            "Module host {host:?} has more than one source invocation"
        ));
    }
    let compiled = CompiledModuleInvocation {
        host,
        instance_id: instance.id,
        definition_id: instance.definition_id,
        output_id: authored.output_id,
        parameter_overrides: instance.parameter_overrides.clone(),
        input_bindings: authored.input_bindings.clone(),
        automation_tracks: authored.automation_tracks.clone(),
    };
    for binding in authored
        .input_bindings
        .iter()
        .filter(|(input_id, _)| output.reachable_media_inputs.contains(input_id))
        .map(|(_, binding)| binding)
    {
        let MediaInputBinding::TimelineItemOutput { item_id, .. } = binding;
        dependencies
            .media_input_consumers
            .entry(*item_id)
            .or_default()
            .push(host);
    }
    dependencies
        .definition_invocations
        .entry(instance.definition_id)
        .or_default()
        .push(host);
    dependencies
        .instance_invocations
        .entry(instance.id)
        .or_default()
        .push(host);
    invocations.push(compiled);
    Ok(())
}

fn validate_invocation_inputs(
    host: ModuleHost,
    invocation: &crate::model::authoring::ModuleInvocation,
    definition: &CompiledModuleDefinition,
) -> Result<(), String> {
    for input_id in invocation.input_bindings.keys() {
        if !definition.media_inputs.contains_key(input_id) {
            return Err(format!(
                "Module invocation binds unpublished media input {input_id}"
            ));
        }
    }
    for input in definition.media_inputs.values() {
        let binding = invocation.input_bindings.get(&input.id);
        let transition_input_is_implicit = matches!(
            (host, &definition.host_contract),
            (
                ModuleHost::Transition { .. },
                ModuleHostContract::Transition(contract)
            ) if input.id == contract.from_input_id || input.id == contract.to_input_id
        );
        if input.required
            && binding.is_none()
            && !(input.primary && matches!(host, ModuleHost::Attachment(_)))
            && !transition_input_is_implicit
        {
            return Err(format!(
                "Required published media input {} is unbound",
                input.id
            ));
        }
        let Some(MediaInputBinding::TimelineItemOutput { output, .. }) = binding else {
            continue;
        };
        let source_type = match output {
            MediaOutputKind::Image => PortDataType::Image,
            MediaOutputKind::Audio => PortDataType::Audio,
        };
        if !input.data_type.accepts(source_type) {
            return Err(format!(
                "Published media input {} cannot accept {source_type:?}",
                input.id
            ));
        }
    }
    Ok(())
}

pub(super) fn compile_timeline(
    project: &AuthoringProject,
    timeline_id: TimelineId,
) -> Result<CompiledTimeline, String> {
    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
    let track_order = timeline
        .track_order
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect::<HashMap<_, _>>();
    let mut schedule = project
        .items
        .values()
        .filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == timeline_id)
        })
        .map(|item| {
            let source = match &item.source {
                SourceRef::Asset { .. } => PlannedSource::Asset,
                SourceRef::Text { .. } => PlannedSource::Text,
                SourceRef::Shape { .. } => PlannedSource::Shape,
                SourceRef::Solid { .. } => PlannedSource::Solid,
                SourceRef::Composition(instance) => PlannedSource::Composition {
                    timeline_id: instance.timeline_id,
                },
                SourceRef::Module(_) => PlannedSource::Module,
            };
            Ok(ScheduledItem {
                item_id: item.id,
                track_id: item.track_id,
                track_order: *track_order
                    .get(&item.track_id)
                    .ok_or_else(|| format!("Item {} is on an unordered Track", item.id))?,
                layer: item.layer,
                interval: item.interval,
                time_map: item.time_map,
                source,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    schedule.sort_by_key(|item| {
        (
            item.track_order,
            item.layer,
            item.interval.start,
            item.item_id,
        )
    });
    let mut track_schedules: HashMap<_, Vec<_>> = HashMap::new();
    for (index, item) in schedule.iter().enumerate() {
        track_schedules
            .entry(item.track_id)
            .or_default()
            .push(index);
    }
    let schedule_indices = schedule
        .iter()
        .enumerate()
        .map(|(index, item)| (item.item_id, index))
        .collect::<HashMap<_, _>>();
    let mut transitions = project
        .transitions
        .values()
        .filter(|transition| transition.timeline_id == timeline_id)
        .map(|transition| {
            let output = transition.processor.contract.media_type.output_kind();
            let interval = transition.interval()?;
            let interval_end = interval.end()?;
            let source = |item_id| -> Result<TransitionSourceInvocation, String> {
                let schedule_index = *schedule_indices.get(&item_id).ok_or_else(|| {
                    format!(
                        "Transition {} refers to an unscheduled item {item_id}",
                        transition.id
                    )
                })?;
                let item_interval = schedule
                    .get(schedule_index)
                    .ok_or_else(|| {
                        format!("Transition {} source schedule is invalid", transition.id)
                    })?
                    .interval;
                let item_end = item_interval.end()?;
                Ok(TransitionSourceInvocation {
                    item_id,
                    schedule_index,
                    output,
                    required_hidden_handle: TransitionHandleRequirement {
                        before: if item_interval.start > interval.start {
                            item_interval.start.checked_sub(interval.start)?
                        } else {
                            crate::model::authoring::MediaTime::zero()
                        },
                        after: if item_end < interval_end {
                            interval_end.checked_sub(item_end)?
                        } else {
                            crate::model::authoring::MediaTime::zero()
                        },
                    },
                })
            };
            let from = source(transition.from_item_id)?;
            let to = source(transition.to_item_id)?;
            let output_blend_mode = project
                .items
                .get(&transition.to_item_id)
                .ok_or_else(|| {
                    format!(
                        "Transition {} refers to missing to item {}",
                        transition.id, transition.to_item_id
                    )
                })?
                .blend_mode;
            Ok(CompiledTransition {
                id: transition.id,
                edit_point: transition.edit_point,
                from,
                to,
                output_schedule_index: to.schedule_index,
                output_blend_mode,
                progress: NormalizedTransitionProgress::new(interval)?,
                processor: transition.processor.clone(),
                parameters: transition.parameters.clone(),
                module_host: transition.processor.module_processor().map(|_| {
                    ModuleHost::Transition {
                        timeline_id: transition.timeline_id,
                        transition_id: transition.id,
                    }
                }),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    transitions.sort_by_key(|transition| (transition.progress.interval().start, transition.id));
    let fingerprint = timeline_schedule_fingerprint(project, timeline_id)?;
    Ok(CompiledTimeline {
        id: timeline_id,
        fingerprint,
        schedule,
        track_schedules,
        transitions,
    })
}

pub(super) fn compile_module(
    definition: &ModuleDefinition,
) -> Result<CompiledModuleDefinition, String> {
    definition.validate()?;
    let order = topological_order(definition)?;
    let mut active_nodes = HashSet::new();
    let mut outputs = HashMap::new();
    for output in definition.outputs() {
        let mut ancestry = HashSet::new();
        let mut reachable_input_targets = HashSet::new();
        let mut sources = HashMap::new();
        for (data_type, target) in output.targets() {
            reachable_input_targets.insert(target.clone());
            if let Some(source) = definition
                .graph
                .connections
                .iter()
                .find(|connection| connection.to == target)
                .map(|connection| connection.from.clone())
            {
                let reachable = nodes_reaching_output(definition, &source);
                ancestry.extend(reachable.nodes);
                reachable_input_targets.extend(reachable.input_targets);
                sources.insert(data_type, source);
            }
        }
        active_nodes.extend(
            ancestry
                .iter()
                .copied()
                .filter(|node_id| *node_id != output.node_id),
        );
        let evaluation_order = order
            .iter()
            .filter(|node_id| ancestry.contains(node_id) && **node_id != output.node_id)
            .copied()
            .collect();
        let reachable_media_inputs = definition
            .interface
            .media_inputs
            .iter()
            .filter(|input| reachable_input_targets.contains(&input.target))
            .map(|input| input.id)
            .collect();
        outputs.insert(
            output.id,
            CompiledModuleOutput {
                terminal: output,
                sources,
                evaluation_order,
                reachable_media_inputs,
                required_capabilities: BTreeSet::new(),
            },
        );
    }
    let nodes = active_nodes
        .iter()
        .map(|id| {
            let node = definition
                .graph
                .nodes
                .get(id)
                .ok_or_else(|| format!("Compiled output reaches missing Module Node {id}"))?;
            let bypass_routes = crate::model::authoring::ModuleNodePortContract::resolve(node)?
                .ports
                .into_iter()
                .filter(|port| port.direction == PortDirection::Output)
                .filter_map(|port| {
                    node.bypass_input_for_output(&port.key)
                        .map(|input| (port.key, input.to_string()))
                })
                .collect();
            Ok((
                *id,
                CompiledNode {
                    id: *id,
                    content: node.content().clone(),
                    enabled: node.enabled,
                    bypassed: node.bypassed,
                    blend_mode: node.blend_mode,
                    properties: node.properties().clone(),
                    bypass_routes,
                },
            ))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    let mut connections = definition
        .graph
        .connections
        .iter()
        .filter(|connection| {
            active_nodes.contains(&connection.from.node_id)
                && active_nodes.contains(&connection.to.node_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    connections.sort_by(|left, right| {
        (left.to.node_id, &left.to.port, left.order, left.id).cmp(&(
            right.to.node_id,
            &right.to.port,
            right.order,
            right.id,
        ))
    });
    let particle_renderers = super::particle::compile_particle_renderers(definition, &active_nodes);
    for output in outputs.values_mut() {
        if output
            .evaluation_order
            .iter()
            .any(|node_id| particle_renderers.contains_key(node_id))
        {
            output.required_capabilities.insert(RenderCapability::Gpu);
        }
    }
    Ok(CompiledModuleDefinition {
        id: definition.id,
        topology_revision: definition.topology_revision,
        interface_version: definition.interface_version,
        host_contract: definition.host_contract.clone(),
        fingerprint: definition_fingerprint(definition)?,
        nodes,
        connections,
        parameters: definition
            .interface
            .parameters
            .iter()
            .cloned()
            .map(|parameter| (parameter.id, parameter))
            .collect(),
        media_inputs: definition
            .interface
            .media_inputs
            .iter()
            .cloned()
            .map(|input| (input.id, input))
            .collect(),
        outputs,
        particle_renderers,
        signals: definition
            .interface
            .signals
            .iter()
            .cloned()
            .map(|signal| (signal.id, signal))
            .collect(),
        actions: definition
            .interface
            .actions
            .iter()
            .cloned()
            .map(|action| (action.id, action))
            .collect(),
    })
}

fn topological_order(definition: &ModuleDefinition) -> Result<Vec<uuid::Uuid>, String> {
    let mut indegree = definition
        .graph
        .nodes
        .keys()
        .copied()
        .map(|node_id| (node_id, 0_usize))
        .collect::<HashMap<_, _>>();
    let mut outgoing: HashMap<_, Vec<_>> = HashMap::new();
    for connection in &definition.graph.connections {
        *indegree
            .get_mut(&connection.to.node_id)
            .ok_or_else(|| "Module connection target is missing".to_string())? += 1;
        outgoing
            .entry(connection.from.node_id)
            .or_default()
            .push(connection.to.node_id);
    }
    for targets in outgoing.values_mut() {
        targets.sort();
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(node_id, degree)| (*degree == 0).then_some(*node_id))
        .collect::<BTreeSet<_>>();
    let mut result = Vec::with_capacity(indegree.len());
    while let Some(node_id) = ready.pop_first() {
        result.push(node_id);
        for target in outgoing.get(&node_id).into_iter().flatten() {
            let degree = indegree
                .get_mut(target)
                .ok_or_else(|| "Module traversal lost a Node".to_string())?;
            *degree -= 1;
            if *degree == 0 {
                ready.insert(*target);
            }
        }
    }
    if result.len() != definition.graph.nodes.len() {
        return Err(format!(
            "Module definition {} contains a cycle",
            definition.id
        ));
    }
    Ok(result)
}

#[derive(Default)]
struct ExecutableReachability {
    nodes: HashSet<uuid::Uuid>,
    input_targets: HashSet<ModulePortAddress>,
}

/// Traces only the inputs that the runtime can evaluate for one graph output.
/// Disabled Nodes terminate evaluation. Bypassed Nodes follow their canonical
/// same-typed input instead of retaining every authored upstream branch.
fn nodes_reaching_output(
    definition: &ModuleDefinition,
    output: &ModulePortAddress,
) -> ExecutableReachability {
    let mut reachable = ExecutableReachability::default();
    let mut visited_outputs = HashSet::new();
    let mut pending = vec![output.clone()];
    while let Some(source) = pending.pop() {
        if !visited_outputs.insert(source.clone()) {
            continue;
        }
        let Some(node) = definition.graph.nodes.get(&source.node_id) else {
            continue;
        };
        reachable.nodes.insert(node.id);
        if !node.enabled {
            continue;
        }

        if node.bypassed {
            let bypassed_sound_merge =
                matches!(node.content(), crate::model::node::NodeContent::SoundMerge)
                    && source.port == AUDIO_OUTPUT_PORT;
            let input_port = if bypassed_sound_merge {
                Some(MERGE_SOUNDS_PORT)
            } else {
                node.bypass_input_for_output(&source.port)
            };
            let Some(input_port) = input_port else {
                continue;
            };
            let target = ModulePortAddress {
                node_id: node.id,
                port: input_port.to_string(),
            };
            reachable.input_targets.insert(target.clone());
            let mut inputs = definition
                .graph
                .connections
                .iter()
                .filter(|connection| connection.to == target)
                .collect::<Vec<_>>();
            inputs.sort_by_key(|connection| (connection.order, connection.id));
            pending.extend(
                inputs
                    .into_iter()
                    .take(if bypassed_sound_merge { 1 } else { usize::MAX })
                    .map(|connection| connection.from.clone()),
            );
            continue;
        }

        for connection in definition
            .graph
            .connections
            .iter()
            .filter(|connection| connection.to.node_id == node.id)
        {
            reachable.input_targets.insert(connection.to.clone());
            pending.push(connection.from.clone());
        }
        reachable.input_targets.extend(
            definition
                .interface
                .media_inputs
                .iter()
                .filter(|input| input.target.node_id == node.id)
                .map(|input| input.target.clone()),
        );
    }
    reachable
}

pub(super) fn definition_fingerprint(definition: &ModuleDefinition) -> Result<[u8; 32], String> {
    let mut executable = definition.clone();
    executable.name.clear();
    executable.sharing = crate::model::authoring::ModuleDefinitionSharing::Private;
    for node in executable.graph.nodes.values_mut() {
        node.name.clear();
        node.ui_position = [0.0, 0.0];
        node.ui_size = [0.0, 0.0];
        node.ui_collapsed = false;
    }
    for parameter in &mut executable.interface.parameters {
        parameter.name.clear();
    }
    for input in &mut executable.interface.media_inputs {
        input.name.clear();
    }
    for signal in &mut executable.interface.signals {
        signal.name.clear();
    }
    for action in &mut executable.interface.actions {
        action.name.clear();
    }
    let value = serde_json::to_value(executable)
        .map_err(|error| format!("Cannot fingerprint Module definition: {error}"))?;
    let canonical = canonical_json(value);
    let encoded = serde_json::to_vec(&canonical)
        .map_err(|error| format!("Cannot encode Module fingerprint: {error}"))?;
    Ok(Sha256::digest(encoded).into())
}

pub(super) fn timeline_schedule_fingerprint(
    project: &AuthoringProject,
    timeline_id: TimelineId,
) -> Result<[u8; 32], String> {
    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
    let mut items = project
        .items
        .values()
        .filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == timeline_id)
        })
        .map(|item| {
            let source = match &item.source {
                SourceRef::Asset { .. } => (0_u8, None),
                SourceRef::Text { .. } => (1, None),
                SourceRef::Shape { .. } => (2, None),
                SourceRef::Solid { .. } => (3, None),
                SourceRef::Composition(instance) => (4, Some(instance.timeline_id)),
                SourceRef::Module(_) => (5, None),
            };
            (
                item.id,
                item.track_id,
                item.layer,
                item.interval,
                item.time_map,
                item.blend_mode,
                source,
            )
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item.0);
    let mut transitions = project
        .transitions
        .values()
        .filter(|transition| transition.timeline_id == timeline_id)
        .collect::<Vec<_>>();
    transitions.sort_by_key(|transition| transition.id);
    let value = serde_json::json!({
        "timeline_id": timeline.id,
        "track_order": timeline.track_order,
        "items": items,
        "transitions": transitions,
    });
    let encoded = serde_json::to_vec(&canonical_json(value))
        .map_err(|error| format!("Cannot encode Timeline fingerprint: {error}"))?;
    Ok(Sha256::digest(encoded).into())
}

fn canonical_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let sorted = object
                .into_iter()
                .map(|(key, value)| (key, canonical_json(value)))
                .collect::<std::collections::BTreeMap<_, _>>();
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical_json).collect())
        }
        value => value,
    }
}

pub(super) fn referenced_definitions(
    project: &AuthoringProject,
) -> Result<BTreeSet<ModuleDefinitionId>, String> {
    let mut ids = BTreeSet::new();
    for item in project.items.values() {
        if let SourceRef::Module(invocation) = &item.source {
            let instance = project
                .module_instances
                .get(&invocation.instance_id)
                .ok_or_else(|| format!("Missing Module instance {}", invocation.instance_id))?;
            ids.insert(instance.definition_id);
        }
    }
    for attachment in project.attachments.values() {
        if let AttachmentProcessor::Module(invocation) = &attachment.processor {
            let instance = project
                .module_instances
                .get(&invocation.instance_id)
                .ok_or_else(|| format!("Missing Module instance {}", invocation.instance_id))?;
            ids.insert(instance.definition_id);
        }
    }
    for transition in project.transitions.values() {
        if let Some(module) = transition.processor.module_processor() {
            let instance = project
                .module_instances
                .get(&module.instance_id)
                .ok_or_else(|| format!("Missing Module instance {}", module.instance_id))?;
            ids.insert(instance.definition_id);
        }
    }
    Ok(ids)
}

fn validate_nested_timelines(project: &AuthoringProject) -> Result<(), String> {
    let mut children: HashMap<TimelineId, Vec<TimelineId>> = HashMap::new();
    for item in project.items.values() {
        let SourceRef::Composition(instance) = &item.source else {
            continue;
        };
        let owner = timeline_for_item(project, item.id)?;
        children
            .entry(owner)
            .or_default()
            .push(instance.timeline_id);
    }
    fn visit(
        timeline: TimelineId,
        children: &HashMap<TimelineId, Vec<TimelineId>>,
        active: &mut HashSet<TimelineId>,
        complete: &mut HashSet<TimelineId>,
    ) -> Result<(), String> {
        if complete.contains(&timeline) {
            return Ok(());
        }
        if !active.insert(timeline) {
            return Err(format!("Nested Timeline cycle reaches {timeline}"));
        }
        for child in children.get(&timeline).into_iter().flatten() {
            visit(*child, children, active, complete)?;
        }
        active.remove(&timeline);
        complete.insert(timeline);
        Ok(())
    }
    let mut active = HashSet::new();
    let mut complete = HashSet::new();
    for timeline in project.timelines.keys().copied() {
        visit(timeline, &children, &mut active, &mut complete)?;
    }
    Ok(())
}

fn timeline_for_item(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> Result<TimelineId, String> {
    let item = project
        .items
        .get(&item_id)
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    project
        .tracks
        .get(&item.track_id)
        .map(|track| track.timeline_id)
        .ok_or_else(|| format!("Item {item_id} has a missing Track"))
}
