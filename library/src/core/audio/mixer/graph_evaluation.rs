use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::property_evaluation::{AudioPropertyContext, volume_at};
use crate::core::framing::FrameEvaluator;
use crate::model::NodeContent;
use crate::model::project::{
    Composition, EvalOutput, NodeContainer, PortDataType, PortDirection, PortOwner, Project,
};
use crate::model::property::PropertyMap;
use crate::plugin::{PluginManager, PropertyEvaluatorRegistry};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct AudioRoute(Vec<PortOwner>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioRouteStepKind {
    Scope,
    Gain,
    CompositionInstance,
    Media,
}

#[derive(Clone, Debug)]
struct AudioRouteStep<'a> {
    owner: PortOwner,
    kind: AudioRouteStepKind,
    properties: Option<&'a PropertyMap>,
    composition_id: Option<Uuid>,
    diagnostic_scope: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct AudioRoutePlan<'a> {
    steps: Vec<AudioRouteStep<'a>>,
    pub(super) node_id: Uuid,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EvaluatedAudioLeaf {
    pub(super) source_time: f64,
    pub(super) gain: f32,
}

/// Request-local evaluation state shared by preview, offline render, and
/// prefetch planning. Composition Instances recurse into the authoritative
/// top-level definition with the placement's evaluated Time; no Composition
/// or Clip timing field is mutated or shared between placements.
pub(super) struct AudioGraphEvaluator<'a> {
    frame_evaluator: FrameEvaluator<'a>,
    property_contexts: HashMap<Uuid, AudioPropertyContext<'a>>,
    pub(super) routes: Vec<AudioRoutePlan<'a>>,
}

impl<'a> AudioGraphEvaluator<'a> {
    pub(super) fn new(
        project: &'a Project,
        composition: &'a Composition,
        plugin_manager: &'a PluginManager,
        property_evaluators: &'a PropertyEvaluatorRegistry,
    ) -> Self {
        let property_contexts = project
            .compositions
            .iter()
            .map(|composition| {
                (
                    composition.id,
                    AudioPropertyContext::new(
                        property_evaluators,
                        composition.fps,
                        (composition.width, composition.height),
                    ),
                )
            })
            .collect();
        let routes = plan_audio_routes(project, PortOwner::Composition(composition.id));
        Self {
            frame_evaluator: FrameEvaluator::new(
                project,
                composition,
                plugin_manager.get_property_evaluators(),
                plugin_manager,
            ),
            property_contexts,
            routes,
        }
    }

    pub(super) fn evaluate_route(
        &self,
        route: &AudioRoutePlan<'_>,
        timeline_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> Option<EvaluatedAudioLeaf> {
        let mut timeline_time = timeline_time;
        let mut gain = 1.0;
        for step in &route.steps {
            let scope = match self.frame_evaluator.evaluate_owner_scope_with_scratch(
                step.owner,
                timeline_time,
                path,
            ) {
                Ok(EvalOutput::Produced(scope)) => scope,
                Ok(EvalOutput::NoOutput) => return None,
                Err(error) => {
                    log::trace!("audio scope {:?} failed closed: {error}", step.owner);
                    return None;
                }
            };
            if let Some(properties) = step.properties {
                let composition_id = step.composition_id?;
                let context = self.property_contexts.get(&composition_id)?;
                gain *= volume_at(
                    properties,
                    scope.time,
                    context,
                    step.diagnostic_scope.as_deref().unwrap_or("audio"),
                );
            }
            match step.kind {
                AudioRouteStepKind::Scope | AudioRouteStepKind::Gain => {}
                AudioRouteStepKind::CompositionInstance => timeline_time = scope.time,
                AudioRouteStepKind::Media => {
                    return Some(EvaluatedAudioLeaf {
                        source_time: scope.time,
                        gain,
                    });
                }
            }
        }
        None
    }
}

fn plan_audio_routes(project: &Project, root_owner: PortOwner) -> Vec<AudioRoutePlan<'_>> {
    let mut routes = Vec::new();
    collect_audio_routes(
        project,
        root_owner,
        &mut HashSet::new(),
        &mut Vec::new(),
        &mut HashSet::new(),
        &mut routes,
    );
    routes
}

pub(super) fn routed_audio_media_nodes(project: &Project, owner: PortOwner) -> Vec<Uuid> {
    let mut emitted = HashSet::new();
    plan_audio_routes(project, owner)
        .into_iter()
        .filter_map(|route| emitted.insert(route.node_id).then_some(route.node_id))
        .collect()
}

fn collect_audio_routes<'a>(
    project: &'a Project,
    owner: PortOwner,
    path: &mut HashSet<PortOwner>,
    steps: &mut Vec<AudioRouteStep<'a>>,
    emitted: &mut HashSet<AudioRoute>,
    routes: &mut Vec<AudioRoutePlan<'a>>,
) {
    if !path.insert(owner) {
        log::trace!("audio route {owner:?} failed closed because it is recursive");
        return;
    }

    match owner {
        PortOwner::Composition(composition_id) => {
            if project.get_composition(composition_id).is_some() {
                steps.push(AudioRouteStep {
                    owner,
                    kind: AudioRouteStepKind::Scope,
                    properties: None,
                    composition_id: None,
                    diagnostic_scope: None,
                });
                collect_audio_container_routes(project, owner, path, steps, emitted, routes);
                steps.pop();
            }
        }
        PortOwner::Track(track_id) => {
            if let Some(track) = project.get_track(track_id) {
                let step =
                    audio_gain_step(project, owner, AudioRouteStepKind::Gain, &track.properties);
                steps.push(step);
                collect_audio_container_routes(project, owner, path, steps, emitted, routes);
                steps.pop();
            }
        }
        PortOwner::Clip(clip_id) => {
            if let Some(clip) = project.get_clip(clip_id) {
                let step =
                    audio_gain_step(project, owner, AudioRouteStepKind::Gain, &clip.properties);
                steps.push(step);
                collect_audio_container_routes(project, owner, path, steps, emitted, routes);
                steps.pop();
            }
        }
        PortOwner::Node(node_id) => {
            let Some(node) = project.get_node(node_id) else {
                path.remove(&owner);
                return;
            };
            if node.enabled && node_has_audio_output(project, owner) {
                match node.content() {
                    NodeContent::Media(_) => {
                        let step = audio_gain_step(
                            project,
                            owner,
                            AudioRouteStepKind::Media,
                            node.properties(),
                        );
                        steps.push(step);
                        let route =
                            AudioRoute(steps.iter().map(|step| step.owner).collect::<Vec<_>>());
                        if emitted.insert(route) {
                            routes.push(AudioRoutePlan {
                                steps: steps.clone(),
                                node_id,
                            });
                        }
                        steps.pop();
                    }
                    NodeContent::CompositionInstance(instance) => {
                        if !matches!(
                            project.find_node_container(node_id),
                            Some(NodeContainer::Clip(_))
                        ) {
                            log::trace!(
                                "audio mixer skipped Composition Instance {node_id} outside a Clip"
                            );
                        } else if project.get_composition(instance.composition_id).is_some() {
                            let step = audio_gain_step(
                                project,
                                owner,
                                AudioRouteStepKind::CompositionInstance,
                                node.properties(),
                            );
                            steps.push(step);
                            collect_audio_routes(
                                project,
                                PortOwner::Composition(instance.composition_id),
                                path,
                                steps,
                                emitted,
                                routes,
                            );
                            steps.pop();
                        }
                    }
                    NodeContent::PluginOperation(operation) => {
                        // Audio operations need a runtime evaluator. Never
                        // reinterpret their inputs as an implicit sum or
                        // pass-through.
                        log::trace!(
                            "audio mixer skipped unsupported PluginOperation {} ({}/{})",
                            node.id,
                            operation.category,
                            operation.component_id
                        );
                    }
                    NodeContent::Generator(_) | NodeContent::Value(_) | NodeContent::Merge => {}
                }
            }
        }
    }
    path.remove(&owner);
}

fn collect_audio_container_routes<'a>(
    project: &'a Project,
    owner: PortOwner,
    path: &mut HashSet<PortOwner>,
    steps: &mut Vec<AudioRouteStep<'a>>,
    emitted: &mut HashSet<AudioRoute>,
    routes: &mut Vec<AudioRoutePlan<'a>>,
) {
    for source in project.container_audio_sources(owner) {
        collect_audio_routes(project, source.source, path, steps, emitted, routes);
    }
}

fn audio_gain_step<'a>(
    project: &Project,
    owner: PortOwner,
    kind: AudioRouteStepKind,
    properties: &'a PropertyMap,
) -> AudioRouteStep<'a> {
    let prefix = match owner {
        PortOwner::Composition(_) => "composition",
        PortOwner::Track(_) => "track",
        PortOwner::Clip(_) => "clip",
        PortOwner::Node(_) => "node",
    };
    AudioRouteStep {
        owner,
        kind,
        properties: Some(properties),
        composition_id: project.find_containing_composition(owner.id()),
        diagnostic_scope: Some(format!("{prefix}:{}", owner.id())),
    }
}

fn node_has_audio_output(project: &Project, owner: PortOwner) -> bool {
    project.port_definitions(owner).into_iter().any(|port| {
        port.direction == PortDirection::Output && port.data_type == PortDataType::Audio
    })
}
