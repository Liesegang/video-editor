use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::model::authoring::{
    AttachmentProcessor, AuthoringProject, InstanceLocator, MediaInputBinding, MediaOutputKind,
    ModuleDefinition, ModuleDefinitionId, SourceRef, TimelineId, TimelineItemId,
};
use crate::model::project::connection::PortDataType;
use crate::model::project::connection::PortDirection;

use super::{
    CompiledModuleDefinition, CompiledModuleInvocation, CompiledModuleOutput, CompiledNode,
    CompiledTimeline, DependencyIndex, ModuleHost, PlannedSource, RenderPlan, ScheduledItem,
    TimelineRangeDependency,
};

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

        validate_media_input_cycles(project, &invocations)?;
        Ok(RenderPlan {
            root_timeline_id: project.root_timeline_id,
            timelines,
            module_definitions,
            module_invocations: invocations,
            dependencies,
        })
    }
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
    let output = definition
        .media_outputs
        .get(&authored.output_id)
        .ok_or_else(|| {
            format!(
                "Module instance {} selects missing published output {}",
                instance.id, authored.output_id
            )
        })?;
    if matches!(host, ModuleHost::TimelineItem { .. })
        && output.interface.data_type != PortDataType::Image
    {
        return Err(format!(
            "Node Clip Module output {} must be Image in the first render slice",
            authored.output_id
        ));
    }
    validate_invocation_inputs(host, authored, definition)?;

    let index = invocations.len();
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
        input_bindings: authored.input_bindings.clone(),
        automation_tracks: authored.automation_tracks.clone(),
    };
    for binding in authored.input_bindings.values() {
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
        if input.required
            && binding.is_none()
            && !(input.primary && matches!(host, ModuleHost::Attachment(_)))
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
    let fingerprint = timeline_schedule_fingerprint(project, timeline_id)?;
    Ok(CompiledTimeline {
        id: timeline_id,
        fingerprint,
        schedule,
        track_schedules,
    })
}

pub(super) fn compile_module(
    definition: &ModuleDefinition,
) -> Result<CompiledModuleDefinition, String> {
    definition.validate()?;
    let order = topological_order(definition)?;
    let mut active_nodes = HashSet::new();
    let mut media_outputs = HashMap::new();
    for output in &definition.interface.media_outputs {
        let ancestry = nodes_reaching(&definition.graph.connections, output.source.node_id);
        active_nodes.extend(ancestry.iter().copied());
        let evaluation_order = order
            .iter()
            .filter(|node_id| ancestry.contains(node_id))
            .copied()
            .collect();
        media_outputs.insert(
            output.id,
            CompiledModuleOutput {
                interface: output.clone(),
                evaluation_order,
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
    Ok(CompiledModuleDefinition {
        id: definition.id,
        topology_revision: definition.topology_revision,
        interface_version: definition.interface_version,
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
        media_outputs,
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

fn nodes_reaching(
    connections: &[crate::model::authoring::ModuleConnection],
    output_node: uuid::Uuid,
) -> HashSet<uuid::Uuid> {
    let mut incoming: HashMap<_, Vec<_>> = HashMap::new();
    for connection in connections {
        incoming
            .entry(connection.to.node_id)
            .or_default()
            .push(connection.from.node_id);
    }
    let mut active = HashSet::new();
    let mut pending = vec![output_node];
    while let Some(node_id) = pending.pop() {
        if active.insert(node_id) {
            pending.extend(incoming.get(&node_id).into_iter().flatten().copied());
        }
    }
    active
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
    for output in &mut executable.interface.media_outputs {
        output.name.clear();
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
                source,
            )
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item.0);
    let value = serde_json::json!({
        "timeline_id": timeline.id,
        "track_order": timeline.track_order,
        "items": items,
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

fn validate_media_input_cycles(
    project: &AuthoringProject,
    invocations: &[CompiledModuleInvocation],
) -> Result<(), String> {
    let mut graph: HashMap<TimelineItemId, Vec<TimelineItemId>> = HashMap::new();
    for invocation in invocations {
        let ModuleHost::TimelineItem {
            timeline_id,
            item_id,
        } = invocation.host
        else {
            continue;
        };
        for binding in invocation.input_bindings.values() {
            let MediaInputBinding::TimelineItemOutput {
                locator,
                item_id: source_id,
                ..
            } = binding;
            let source_timeline = timeline_for_item(project, *source_id)?;
            if matches!(locator, InstanceLocator::SameTimeline) && source_timeline != timeline_id {
                return Err(format!(
                    "Module input on item {item_id} uses SameTimeline for item {source_id} in another Timeline"
                ));
            }
            graph.entry(item_id).or_default().push(*source_id);
        }
    }
    fn visit(
        item: TimelineItemId,
        graph: &HashMap<TimelineItemId, Vec<TimelineItemId>>,
        active: &mut HashSet<TimelineItemId>,
        complete: &mut HashSet<TimelineItemId>,
    ) -> Result<(), String> {
        if complete.contains(&item) {
            return Ok(());
        }
        if !active.insert(item) {
            return Err(format!(
                "Timeline media-input dependency cycle reaches item {item}"
            ));
        }
        for dependency in graph.get(&item).into_iter().flatten() {
            if graph.contains_key(dependency) {
                visit(*dependency, graph, active, complete)?;
            }
        }
        active.remove(&item);
        complete.insert(item);
        Ok(())
    }
    let mut active = HashSet::new();
    let mut complete = HashSet::new();
    for item in graph.keys().copied() {
        visit(item, &graph, &mut active, &mut complete)?;
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
