use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::property_evaluation::{AudioPropertyContext, volume_at};
use crate::core::framing::FrameEvaluator;
use crate::model::NodeContent;
use crate::model::project::{
    AUDIO_OUTPUT_PORT, Composition, EvalOutput, MERGE_SOUNDS_PORT, NodeContainer, PortAddress,
    PortDataType, PortDirection, PortMultiplicity, PortOwner, Project, ProjectConnection,
    TIME_PORT,
};
use crate::model::property::PropertyMap;
use crate::plugin::{PluginManager, PropertyEvaluatorRegistry};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct AudioRoute(Vec<PortOwner>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioRouteStepKind {
    Scope,
    RemappedScope,
    Gain,
    CompositionInstance,
    Media,
}

#[derive(Clone, Debug)]
struct AudioRouteStep<'a> {
    owner: PortOwner,
    kind: AudioRouteStepKind,
    has_explicit_time: bool,
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
        Self::new_for_owner(
            project,
            composition,
            PortOwner::Composition(composition.id),
            plugin_manager,
            property_evaluators,
        )
    }

    pub(super) fn new_for_owner(
        project: &'a Project,
        composition: &'a Composition,
        root_owner: PortOwner,
        plugin_manager: &'a PluginManager,
        property_evaluators: &'a PropertyEvaluatorRegistry,
    ) -> Self {
        Self::new_for_root(
            project,
            composition,
            root_owner,
            None,
            plugin_manager,
            property_evaluators,
        )
    }

    pub(super) fn new_for_output(
        project: &'a Project,
        composition: &'a Composition,
        output: &PortAddress,
        plugin_manager: &'a PluginManager,
        property_evaluators: &'a PropertyEvaluatorRegistry,
    ) -> Self {
        Self::new_for_root(
            project,
            composition,
            output.owner,
            Some(output),
            plugin_manager,
            property_evaluators,
        )
    }

    fn new_for_root(
        project: &'a Project,
        composition: &'a Composition,
        root_owner: PortOwner,
        root_output: Option<&PortAddress>,
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
        let routes = plan_audio_routes(project, root_owner, root_output);
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
        // Keep the current Composition coordinate separate from an authored
        // Time remap. Scope/activity checks always use Composition time; a
        // routed source time is applied only after that check. Otherwise a
        // Clip-local value would be fed back through Clip::local_time and a
        // non-zero-start Clip would incorrectly become inactive.
        let mut composition_time = timeline_time;
        let mut routed_source_time = None;
        let mut gain = 1.0;
        for step in &route.steps {
            let mut scope = match self.frame_evaluator.evaluate_owner_scope_with_scratch(
                step.owner,
                composition_time,
                path,
            ) {
                Ok(EvalOutput::Produced(scope)) => scope,
                Ok(EvalOutput::NoOutput) => return None,
                Err(error) => {
                    log::trace!("audio scope {:?} failed closed: {error}", step.owner);
                    return None;
                }
            };
            // An upstream Node without its own Time wire inherits the routed
            // source coordinate. An explicit Time input is authoritative and
            // replaces it. Container scopes continue to use Composition time
            // so their half-open activity intervals stay well-defined.
            if matches!(step.owner, PortOwner::Node(_))
                && !step.has_explicit_time
                && let Some(source_time) = routed_source_time
            {
                scope.time = source_time;
            }
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
                AudioRouteStepKind::RemappedScope => routed_source_time = Some(scope.time),
                AudioRouteStepKind::CompositionInstance => {
                    // A Composition Instance intentionally crosses into a new
                    // Composition coordinate space. Its resolved placement
                    // time becomes that Composition's timeline exactly once.
                    composition_time = scope.time;
                    routed_source_time = None;
                }
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

fn plan_audio_routes<'a>(
    project: &'a Project,
    root_owner: PortOwner,
    root_output: Option<&PortAddress>,
) -> Vec<AudioRoutePlan<'a>> {
    let mut routes = Vec::new();
    collect_audio_routes(
        project,
        root_owner,
        root_output,
        &mut HashSet::new(),
        &mut Vec::new(),
        &mut HashSet::new(),
        &mut routes,
    );
    routes
}

pub(super) fn routed_audio_media_nodes(project: &Project, owner: PortOwner) -> Vec<Uuid> {
    let mut emitted = HashSet::new();
    plan_audio_routes(project, owner, None)
        .into_iter()
        .filter_map(|route| emitted.insert(route.node_id).then_some(route.node_id))
        .collect()
}

fn collect_audio_routes<'a>(
    project: &'a Project,
    owner: PortOwner,
    requested_output: Option<&PortAddress>,
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
            if project.get_composition(composition_id).is_some()
                && requested_output.is_none_or(|output| valid_audio_output(project, owner, output))
            {
                steps.push(AudioRouteStep {
                    owner,
                    kind: AudioRouteStepKind::Scope,
                    has_explicit_time: false,
                    properties: None,
                    composition_id: None,
                    diagnostic_scope: None,
                });
                collect_audio_container_routes(project, owner, path, steps, emitted, routes);
                steps.pop();
            }
        }
        PortOwner::Track(track_id) => {
            if let Some(track) = project.get_track(track_id)
                && requested_output.is_none_or(|output| valid_audio_output(project, owner, output))
            {
                let step =
                    audio_gain_step(project, owner, AudioRouteStepKind::Gain, &track.properties);
                steps.push(step);
                collect_audio_container_routes(project, owner, path, steps, emitted, routes);
                steps.pop();
            }
        }
        PortOwner::Clip(clip_id) => {
            if let Some(clip) = project.get_clip(clip_id)
                && requested_output.is_none_or(|output| valid_audio_output(project, owner, output))
            {
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
            let default_output;
            let requested_output = if let Some(output) = requested_output {
                output
            } else {
                default_output = PortAddress::new(owner, AUDIO_OUTPUT_PORT);
                &default_output
            };
            if node.enabled && valid_audio_output(project, owner, requested_output) {
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
                                None,
                                path,
                                steps,
                                emitted,
                                routes,
                            );
                            steps.pop();
                        }
                    }
                    NodeContent::PluginOperation(operation) => {
                        if node.bypassed {
                            let connection = node
                                .bypass_input_for_output(&requested_output.port)
                                .map(|input| PortAddress::new(owner, input))
                                .and_then(|target| valid_single_audio_input(project, &target));
                            if let Some(connection) = connection {
                                steps.push(audio_node_scope_step(project, owner));
                                collect_audio_routes(
                                    project,
                                    connection.from.owner,
                                    Some(&connection.from),
                                    path,
                                    steps,
                                    emitted,
                                    routes,
                                );
                                steps.pop();
                            } else {
                                log::trace!(
                                    "audio mixer skipped malformed bypass route for PluginOperation {}",
                                    node.id
                                );
                            }
                        } else {
                            // Audio operations need a runtime evaluator.
                            // Never reinterpret an unavailable operation as
                            // an implicit pass-through unless it is authored
                            // in bypass state.
                            log::trace!(
                                "audio mixer skipped unsupported PluginOperation {} ({}/{})",
                                node.id,
                                operation.category,
                                operation.component_id
                            );
                        }
                    }
                    NodeContent::SoundMerge => {
                        if requested_output.port != AUDIO_OUTPUT_PORT {
                            path.remove(&owner);
                            return;
                        }
                        if let Some(container) = project.structural_sound_merge_owner(node_id)
                            && !project.structural_sound_merge_is_well_formed(container)
                        {
                            log::trace!(
                                "audio mixer skipped malformed structural Sound Merge {node_id}"
                            );
                            path.remove(&owner);
                            return;
                        }
                        let target = PortAddress::new(owner, MERGE_SOUNDS_PORT);
                        let Some(inputs) = valid_variadic_audio_inputs(project, &target) else {
                            log::trace!("audio mixer skipped malformed Sound Merge {node_id}");
                            path.remove(&owner);
                            return;
                        };
                        let input_count = if node.bypassed { 1 } else { inputs.len() };
                        steps.push(audio_node_scope_step(project, owner));
                        for connection in inputs.into_iter().take(input_count) {
                            collect_audio_routes(
                                project,
                                connection.from.owner,
                                Some(&connection.from),
                                path,
                                steps,
                                emitted,
                                routes,
                            );
                        }
                        steps.pop();
                    }
                    NodeContent::NativeOperation(operation) => {
                        log::warn!(
                            "Native catalog node {} ({}) has no audio runtime; producing No Output",
                            node.id,
                            operation.catalog_id
                        );
                    }
                    NodeContent::Generator(_)
                    | NodeContent::Value(_)
                    | NodeContent::Data(_)
                    | NodeContent::List(_)
                    | NodeContent::Path(_)
                    | NodeContent::SoundAnalysis(_)
                    | NodeContent::Merge => {}
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
        let output = PortAddress::new(source.source, AUDIO_OUTPUT_PORT);
        collect_audio_routes(
            project,
            source.source,
            Some(&output),
            path,
            steps,
            emitted,
            routes,
        );
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
        has_explicit_time: has_explicit_time_input(project, owner),
        properties: Some(properties),
        composition_id: project.find_containing_composition(owner.id()),
        diagnostic_scope: Some(format!("{prefix}:{}", owner.id())),
    }
}

fn audio_node_scope_step(project: &Project, owner: PortOwner) -> AudioRouteStep<'_> {
    let has_explicit_time = has_explicit_time_input(project, owner);
    AudioRouteStep {
        owner,
        kind: if has_explicit_time {
            AudioRouteStepKind::RemappedScope
        } else {
            AudioRouteStepKind::Scope
        },
        has_explicit_time,
        properties: None,
        composition_id: None,
        diagnostic_scope: None,
    }
}

fn has_explicit_time_input(project: &Project, owner: PortOwner) -> bool {
    project
        .connections
        .iter()
        .any(|connection| connection.to == PortAddress::new(owner, TIME_PORT))
}

fn valid_audio_output(project: &Project, owner: PortOwner, output: &PortAddress) -> bool {
    output.owner == owner
        && project
            .port_definition(output, PortDirection::Output)
            .is_some_and(|definition| {
                definition.data_type == PortDataType::Audio
                    && definition.multiplicity == PortMultiplicity::Single
            })
}

fn valid_single_audio_input<'a>(
    project: &'a Project,
    target: &PortAddress,
) -> Option<&'a ProjectConnection> {
    let inputs = valid_audio_inputs(project, target, PortMultiplicity::Single)?;
    (inputs.len() == 1).then_some(inputs[0])
}

fn valid_variadic_audio_inputs<'a>(
    project: &'a Project,
    target: &PortAddress,
) -> Option<Vec<&'a ProjectConnection>> {
    valid_audio_inputs(project, target, PortMultiplicity::Variadic)
}

fn valid_audio_inputs<'a>(
    project: &'a Project,
    target: &PortAddress,
    multiplicity: PortMultiplicity,
) -> Option<Vec<&'a ProjectConnection>> {
    let target_definition = project.port_definition(target, PortDirection::Input)?;
    if target_definition.data_type != PortDataType::Audio
        || target_definition.multiplicity != multiplicity
    {
        return None;
    }
    let mut inputs = project
        .connections
        .iter()
        .filter(|connection| connection.to == *target)
        .collect::<Vec<_>>();
    inputs.sort_by_key(|connection| (connection.order, connection.id));
    if multiplicity == PortMultiplicity::Single && inputs.len() != 1 {
        return None;
    }
    let mut source_addresses = HashSet::new();
    let mut connection_ids = HashSet::new();
    for (expected_order, connection) in inputs.iter().enumerate() {
        if connection.order != expected_order as i64
            || !source_addresses.insert(&connection.from)
            || !connection_ids.insert(connection.id)
            || !valid_audio_output(project, connection.from.owner, &connection.from)
            || !project.validate_connection(connection).is_empty()
        {
            return None;
        }
    }
    Some(inputs)
}
