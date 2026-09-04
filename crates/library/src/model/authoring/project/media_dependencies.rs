//! Authoritative media-evaluation dependency validation.
//!
//! Item Modules, Transition Modules, and nested Timeline composition all
//! participate in one execution graph. RenderPlan compilation relies on this
//! validation instead of maintaining a second, less complete cycle model.

use std::collections::{HashMap, HashSet};

use super::super::{
    AuthoringProject, InstanceLocator, InstancePath, MediaInputBinding, SourceRef, TimelineId,
    TimelineItemId, TransitionId, TransitionModuleInstanceTarget,
};

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
struct TimelineScope {
    timeline_id: TimelineId,
    instance_path: Option<InstancePath>,
}

impl TimelineScope {
    const fn definition(timeline_id: TimelineId) -> Self {
        Self {
            timeline_id,
            instance_path: None,
        }
    }

    fn concrete(timeline_id: TimelineId, instance_path: InstancePath) -> Self {
        Self {
            timeline_id,
            instance_path: Some(instance_path),
        }
    }

    fn nested(&self, item_id: TimelineItemId, timeline_id: TimelineId) -> Self {
        Self {
            timeline_id,
            instance_path: self.instance_path.as_ref().map(|path| path.nested(item_id)),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
enum MediaDependencyNode {
    /// Runtime rejects re-entering the same Timeline definition even through
    /// a different concrete placement, so this key is intentionally not
    /// scoped by InstancePath.
    Timeline(TimelineId),
    Item {
        scope: TimelineScope,
        item_id: TimelineItemId,
    },
    Transition {
        scope: TimelineScope,
        transition_id: TransitionId,
    },
}

type DependencyGraph = HashMap<MediaDependencyNode, HashSet<MediaDependencyNode>>;

pub(super) fn validate_media_dependency_cycles(project: &AuthoringProject) -> Result<(), String> {
    let scopes = timeline_scopes(project)?;
    let mut graph = DependencyGraph::new();

    for scope in scopes {
        let timeline_node = MediaDependencyNode::Timeline(scope.timeline_id);
        graph.entry(timeline_node.clone()).or_default();

        for item in project.items.values().filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == scope.timeline_id)
        }) {
            let item_node = MediaDependencyNode::Item {
                scope: scope.clone(),
                item_id: item.id,
            };
            add_edge(&mut graph, timeline_node.clone(), item_node.clone());
            match &item.source {
                SourceRef::Composition(instance) => add_edge(
                    &mut graph,
                    item_node,
                    MediaDependencyNode::Timeline(instance.timeline_id),
                ),
                SourceRef::Module(invocation) => add_binding_edges(
                    project,
                    &mut graph,
                    item_node,
                    &scope,
                    invocation.input_bindings.values(),
                )?,
                _ => {}
            }
        }

        for transition in project
            .transitions
            .values()
            .filter(|transition| transition.timeline_id == scope.timeline_id)
        {
            let transition_node = MediaDependencyNode::Transition {
                scope: scope.clone(),
                transition_id: transition.id,
            };
            add_edge(&mut graph, timeline_node.clone(), transition_node.clone());
            for item_id in [transition.from_item_id, transition.to_item_id] {
                add_edge(
                    &mut graph,
                    transition_node.clone(),
                    MediaDependencyNode::Item {
                        scope: scope.clone(),
                        item_id,
                    },
                );
            }
            let Some(module) = transition.processor.module_processor() else {
                continue;
            };
            // The definition-owned binding must be independently executable;
            // a placement mask cannot make a cyclic shared default valid.
            add_binding_edges(
                project,
                &mut graph,
                transition_node.clone(),
                &scope,
                module.input_bindings.values(),
            )?;
            if let Some(instance_path) = &scope.instance_path {
                let controls = project.effective_transition_module_controls(
                    &TransitionModuleInstanceTarget {
                        instance_path: instance_path.clone(),
                        transition_id: transition.id,
                        module_instance_id: module.instance_id,
                    },
                )?;
                add_binding_edges(
                    project,
                    &mut graph,
                    transition_node,
                    &scope,
                    controls.input_bindings.values(),
                )?;
            }
        }
    }

    reject_cycles(&graph)
}

fn timeline_scopes(project: &AuthoringProject) -> Result<HashSet<TimelineScope>, String> {
    let mut scopes = project
        .timelines
        .keys()
        .copied()
        .map(TimelineScope::definition)
        .collect::<HashSet<_>>();
    collect_concrete_scopes(
        project,
        TimelineScope::concrete(
            project.root_timeline_id,
            InstancePath::root(project.root_timeline_id),
        ),
        &mut HashSet::new(),
        &mut scopes,
    )?;
    Ok(scopes)
}

fn collect_concrete_scopes(
    project: &AuthoringProject,
    scope: TimelineScope,
    active_timelines: &mut HashSet<TimelineId>,
    scopes: &mut HashSet<TimelineScope>,
) -> Result<(), String> {
    if !active_timelines.insert(scope.timeline_id) {
        return Err(format!(
            "Nested Timeline cycle reaches {}",
            scope.timeline_id
        ));
    }
    if !scopes.insert(scope.clone()) {
        active_timelines.remove(&scope.timeline_id);
        return Ok(());
    }
    for item in project.items.values().filter(|item| {
        project
            .tracks
            .get(&item.track_id)
            .is_some_and(|track| track.timeline_id == scope.timeline_id)
    }) {
        if let SourceRef::Composition(instance) = &item.source {
            collect_concrete_scopes(
                project,
                scope.nested(item.id, instance.timeline_id),
                active_timelines,
                scopes,
            )?;
        }
    }
    active_timelines.remove(&scope.timeline_id);
    Ok(())
}

fn add_binding_edges<'a>(
    project: &AuthoringProject,
    graph: &mut DependencyGraph,
    host: MediaDependencyNode,
    host_scope: &TimelineScope,
    bindings: impl Iterator<Item = &'a MediaInputBinding>,
) -> Result<(), String> {
    for binding in bindings {
        let MediaInputBinding::TimelineItemOutput {
            locator, item_id, ..
        } = binding;
        let source_timeline_id = timeline_for_item(project, *item_id)?;
        let source_scope = match locator {
            InstanceLocator::SameTimeline => host_scope.clone(),
            InstanceLocator::Exact(path) => {
                TimelineScope::concrete(source_timeline_id, path.clone())
            }
        };
        add_edge(
            graph,
            host.clone(),
            MediaDependencyNode::Item {
                scope: source_scope,
                item_id: *item_id,
            },
        );
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
        .ok_or_else(|| format!("Media dependency refers to missing item {item_id}"))?;
    project
        .tracks
        .get(&item.track_id)
        .map(|track| track.timeline_id)
        .ok_or_else(|| format!("Media dependency item {item_id} has no Track"))
}

fn add_edge(graph: &mut DependencyGraph, from: MediaDependencyNode, to: MediaDependencyNode) {
    graph.entry(to.clone()).or_default();
    graph.entry(from).or_default().insert(to);
}

fn reject_cycles(graph: &DependencyGraph) -> Result<(), String> {
    fn visit(
        node: &MediaDependencyNode,
        graph: &DependencyGraph,
        active: &mut HashSet<MediaDependencyNode>,
        complete: &mut HashSet<MediaDependencyNode>,
    ) -> Result<(), String> {
        if complete.contains(node) {
            return Ok(());
        }
        if !active.insert(node.clone()) {
            return Err(format!(
                "Media evaluation dependency cycle reaches {}",
                describe_node(node)
            ));
        }
        for dependency in graph.get(node).into_iter().flatten() {
            visit(dependency, graph, active, complete)?;
        }
        active.remove(node);
        complete.insert(node.clone());
        Ok(())
    }

    let mut active = HashSet::new();
    let mut complete = HashSet::new();
    for node in graph.keys() {
        visit(node, graph, &mut active, &mut complete)?;
    }
    Ok(())
}

fn describe_node(node: &MediaDependencyNode) -> String {
    match node {
        MediaDependencyNode::Timeline(timeline_id) => format!("Timeline {timeline_id}"),
        MediaDependencyNode::Item { item_id, .. } => format!("Timeline item {item_id}"),
        MediaDependencyNode::Transition { transition_id, .. } => {
            format!("Transition {transition_id}")
        }
    }
}
