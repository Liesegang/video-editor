use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::authoring::{
    AuthoringProject, BindingScope, ModuleDefinition, ModuleDefinitionId, ModuleRole, SourceRef,
    TimelineId,
};
use crate::model::node::NodeContent;
use crate::plugin::{EFFECT_APPLY_OPERATION, EFFECT_CATEGORY};

use super::{
    CompiledBindingIndex, CompiledModuleDefinition, CompiledModuleOperation, CompiledTimeline,
    DependencyIndex, ModuleInvocation, ModuleInvocationOwner, PlannedSource, RenderPlan,
    ScheduledItem,
};

pub struct RenderPlanCompiler;

impl RenderPlanCompiler {
    pub fn compile(project: &AuthoringProject) -> Result<RenderPlan, String> {
        let module_definitions = project
            .module_definitions
            .iter()
            .map(|(id, definition)| compile_module(*id, definition).map(|module| (*id, module)))
            .collect::<Result<HashMap<_, _>, _>>()?;

        Self::compile_with_parts(project, module_definitions, None)
    }

    pub(super) fn compile_with_parts(
        project: &AuthoringProject,
        module_definitions: HashMap<ModuleDefinitionId, CompiledModuleDefinition>,
        compiled_timelines: Option<HashMap<TimelineId, CompiledTimeline>>,
    ) -> Result<RenderPlan, String> {
        project.validate()?;
        validate_nested_timelines(project)?;

        let mut module_invocations = Vec::new();
        let mut dependencies = DependencyIndex::default();
        let timelines = match compiled_timelines {
            Some(timelines) => timelines,
            None => project
                .timelines
                .keys()
                .copied()
                .map(|timeline_id| {
                    compile_timeline(project, timeline_id).map(|timeline| (timeline_id, timeline))
                })
                .collect::<Result<HashMap<_, _>, _>>()?,
        };

        for timeline in timelines.values() {
            for item in &timeline.schedule {
                dependencies.timeline_ranges.insert(
                    item.item_id,
                    (timeline.id, item.interval.start, item.interval.duration),
                );
                if let PlannedSource::Module { module_instance_id } = &item.source {
                    register_invocation(
                        project,
                        &mut module_invocations,
                        &mut dependencies,
                        ModuleInvocationOwner::Item(item.item_id),
                        *module_instance_id,
                    )?;
                }
            }
        }

        let mut attachments: Vec<_> = project.attachments.values().collect();
        attachments.sort_by_key(|attachment| (attachment.stage, attachment.order, attachment.id));
        for attachment in attachments {
            register_invocation(
                project,
                &mut module_invocations,
                &mut dependencies,
                ModuleInvocationOwner::Attachment {
                    attachment_id: attachment.id,
                    owner: attachment.owner.clone(),
                    stage: attachment.stage,
                },
                attachment.module_instance_id,
            )?;
        }

        let bindings = compile_bindings(project)?;

        Ok(RenderPlan {
            root_timeline_id: project.root_timeline_id,
            timelines,
            module_definitions,
            module_invocations,
            bindings,
            dependencies,
        })
    }
}

fn compile_bindings(project: &AuthoringProject) -> Result<CompiledBindingIndex, String> {
    let mut compiled = CompiledBindingIndex::default();
    let mut signals = project
        .signal_bindings
        .values()
        .cloned()
        .collect::<Vec<_>>();
    signals.sort_by_key(|binding| binding.id);
    for binding in signals {
        for definition_id in binding_target_definitions(project, &binding.scope)? {
            compiled.add_signal(definition_id, binding.clone());
        }
        compiled.add_signal_source(binding);
    }
    let mut events = project.event_bindings.values().cloned().collect::<Vec<_>>();
    events.sort_by_key(|binding| binding.id);
    for binding in events {
        for definition_id in binding_target_definitions(project, &binding.scope)? {
            compiled.add_event(definition_id, binding.clone());
        }
        compiled.add_event_source(binding);
    }
    compiled.finish();
    Ok(compiled)
}

fn binding_target_definitions(
    project: &AuthoringProject,
    scope: &BindingScope,
) -> Result<Vec<ModuleDefinitionId>, String> {
    let mut definitions = match scope {
        BindingScope::Definition { definition_id } => vec![*definition_id],
        BindingScope::Instance {
            module_instance_id, ..
        } => vec![
            project
                .module_instances
                .get(module_instance_id)
                .ok_or_else(|| format!("Missing Module instance {module_instance_id}"))?
                .definition_id,
        ],
        BindingScope::Query { .. } => project.module_definitions.keys().copied().collect(),
    };
    definitions.sort();
    Ok(definitions)
}

pub(super) fn compile_timeline(
    project: &AuthoringProject,
    timeline_id: TimelineId,
) -> Result<CompiledTimeline, String> {
    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
    let track_order: HashMap<_, _> = timeline
        .track_order
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();
    let mut schedule = Vec::new();
    for item in project.items.values().filter(|item| {
        project
            .tracks
            .get(&item.track_id)
            .is_some_and(|track| track.timeline_id == timeline.id)
    }) {
        let source = match &item.source {
            SourceRef::Asset { .. } => PlannedSource::Asset,
            SourceRef::Text { .. } => PlannedSource::Text,
            SourceRef::Shape { .. } => PlannedSource::Shape,
            SourceRef::Solid { .. } => PlannedSource::Solid,
            SourceRef::Composition(instance) => PlannedSource::Composition {
                timeline_id: instance.timeline_id,
            },
            SourceRef::Module { module_instance_id } => PlannedSource::Module {
                module_instance_id: *module_instance_id,
            },
        };
        schedule.push(ScheduledItem {
            item_id: item.id,
            track_id: item.track_id,
            track_order: *track_order
                .get(&item.track_id)
                .ok_or_else(|| format!("Item {} is on an unordered Track", item.id))?,
            layer: item.layer,
            interval: item.interval,
            source,
        });
    }
    schedule.sort_by_key(|item| {
        (
            item.track_order,
            item.layer,
            item.interval.start,
            item.item_id,
        )
    });
    let mut attachment_ids: Vec<_> = project
        .attachments
        .values()
        .filter_map(|attachment| match &attachment.owner {
            crate::model::authoring::AttachmentOwner::Timeline { timeline_id: owner }
                if *owner == timeline.id =>
            {
                Some(attachment.id)
            }
            _ => None,
        })
        .collect();
    attachment_ids.sort();
    Ok(CompiledTimeline {
        id: timeline.id,
        schedule,
        attachment_ids,
    })
}

fn register_invocation(
    project: &AuthoringProject,
    invocations: &mut Vec<ModuleInvocation>,
    dependencies: &mut DependencyIndex,
    owner: ModuleInvocationOwner,
    instance_id: crate::model::authoring::ModuleInstanceId,
) -> Result<(), String> {
    let instance = project
        .module_instances
        .get(&instance_id)
        .ok_or_else(|| format!("Missing Module instance {instance_id}"))?;
    let index = invocations.len();
    invocations.push(ModuleInvocation {
        owner,
        module_instance_id: instance_id,
        definition_id: instance.definition_id,
    });
    dependencies
        .definition_invocations
        .entry(instance.definition_id)
        .or_default()
        .push(index);
    Ok(())
}

pub(super) fn compile_module(
    id: ModuleDefinitionId,
    definition: &ModuleDefinition,
) -> Result<CompiledModuleDefinition, String> {
    definition.validate()?;
    let mut indegree: HashMap<_, usize> = definition
        .graph
        .nodes
        .keys()
        .copied()
        .map(|node_id| (node_id, 0))
        .collect();
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
    let mut ready: Vec<_> = indegree
        .iter()
        .filter_map(|(node_id, count)| (*count == 0).then_some(*node_id))
        .collect();
    ready.sort();
    let mut queue = VecDeque::from(ready);
    let mut evaluation_order = Vec::with_capacity(indegree.len());
    while let Some(node_id) = queue.pop_front() {
        evaluation_order.push(node_id);
        if let Some(targets) = outgoing.get(&node_id) {
            let mut targets = targets.clone();
            targets.sort();
            for target in targets {
                let count = indegree
                    .get_mut(&target)
                    .ok_or_else(|| "Module connection target is missing".to_string())?;
                *count -= 1;
                if *count == 0 {
                    queue.push_back(target);
                }
            }
        }
    }
    if evaluation_order.len() != definition.graph.nodes.len() {
        return Err(format!("Module definition {id} contains a cycle"));
    }
    let active_nodes = nodes_reaching_output(definition);
    evaluation_order.retain(|node_id| active_nodes.contains(node_id));
    let operations = match definition.role {
        ModuleRole::Effect => evaluation_order
            .iter()
            .map(|node_id| {
                let node = &definition.graph.nodes[node_id];
                let NodeContent::PluginOperation(operation) = node.content() else {
                    return Err(format!(
                        "Effect Module {id} contains non-Effect Node {node_id}"
                    ));
                };
                if operation.category != EFFECT_CATEGORY
                    || operation.operation != EFFECT_APPLY_OPERATION
                {
                    return Err(format!(
                        "Effect Module {id} contains incompatible operation {}/{}/{}",
                        operation.category, operation.component_id, operation.operation
                    ));
                }
                Ok(CompiledModuleOperation::ImageEffect {
                    node_id: *node_id,
                    effect_type: operation.component_id.clone(),
                    enabled: node.enabled,
                    bypassed: node.bypassed,
                    properties: node.properties().clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        ModuleRole::Generator | ModuleRole::Behavior | ModuleRole::Analyzer => Vec::new(),
    };
    Ok(CompiledModuleDefinition {
        id,
        version: definition.version,
        fingerprint: definition_fingerprint(definition)?,
        evaluation_order,
        operations,
    })
}

fn nodes_reaching_output(definition: &ModuleDefinition) -> std::collections::HashSet<uuid::Uuid> {
    let Some(output_node_id) = definition.output_node_id else {
        return std::collections::HashSet::new();
    };
    let mut incoming: HashMap<uuid::Uuid, Vec<uuid::Uuid>> = HashMap::new();
    for connection in &definition.graph.connections {
        incoming
            .entry(connection.to.node_id)
            .or_default()
            .push(connection.from.node_id);
    }
    let mut active = std::collections::HashSet::new();
    let mut pending = vec![output_node_id];
    while let Some(node_id) = pending.pop() {
        if !active.insert(node_id) {
            continue;
        }
        if let Some(sources) = incoming.get(&node_id) {
            pending.extend(sources.iter().copied());
        }
    }
    active
}

pub(super) fn definition_fingerprint(definition: &ModuleDefinition) -> Result<[u8; 32], String> {
    use sha2::{Digest, Sha256};

    let mut executable = definition.clone();
    for node in executable.graph.nodes.values_mut() {
        node.ui_position = [0.0, 0.0];
        node.ui_size = [1.0, 1.0];
        node.ui_collapsed = false;
    }
    let encoded = serde_json::to_vec(&executable)
        .map_err(|error| format!("Cannot fingerprint Module definition: {error}"))?;
    Ok(Sha256::digest(encoded).into())
}

fn validate_nested_timelines(project: &AuthoringProject) -> Result<(), String> {
    let mut children: HashMap<TimelineId, Vec<TimelineId>> = HashMap::new();
    for item in project.items.values() {
        if let SourceRef::Composition(instance) = &item.source {
            if !project.timelines.contains_key(&instance.timeline_id) {
                return Err(format!(
                    "Item {} refers to a missing nested Timeline",
                    item.id
                ));
            }
            let owner_timeline = project
                .tracks
                .get(&item.track_id)
                .ok_or_else(|| format!("Item {} has a missing Track", item.id))?
                .timeline_id;
            children
                .entry(owner_timeline)
                .or_default()
                .push(instance.timeline_id);
        }
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
        if let Some(nested) = children.get(&timeline) {
            for child in nested {
                visit(*child, children, active, complete)?;
            }
        }
        active.remove(&timeline);
        complete.insert(timeline);
        Ok(())
    }
    let mut active = HashSet::new();
    let mut complete = HashSet::new();
    for timeline in project.timelines.keys() {
        visit(*timeline, &children, &mut active, &mut complete)?;
    }
    Ok(())
}
