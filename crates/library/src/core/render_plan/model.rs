use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::model::BlendMode;
use crate::model::authoring::{
    AttachmentId, AutomatableParameter, AutomationTrack, MediaInputBinding, MediaOutputKind,
    MediaTime, ModuleConnection, ModuleDefinitionId, ModuleHostContract, ModuleInstanceId,
    ModuleOutput, ModuleOutputId, ModulePortAddress, PublishedAction, PublishedActionId,
    PublishedMediaInput, PublishedMediaInputId, PublishedParameter, PublishedParameterId,
    PublishedSignal, PublishedSignalId, TimeMap, TimelineId, TimelineInterval, TimelineItemId,
    TimelineTrackId, TransitionId, TransitionModuleInstanceTarget, TransitionProcessor,
};
use crate::model::node::NodeContent;
use crate::model::project::property::PropertyMap;

/// Derived, immutable execution description for one authoring Project.
///
/// Timeline placement stays hierarchical. Reusable Module definitions are
/// compiled once and each Node Clip contributes only a lightweight invocation.
#[derive(Clone, PartialEq, Debug)]
pub struct RenderPlan {
    pub root_timeline_id: TimelineId,
    pub timelines: HashMap<TimelineId, Arc<CompiledTimeline>>,
    pub module_definitions: HashMap<ModuleDefinitionId, Arc<CompiledModuleDefinition>>,
    pub module_invocations: Vec<CompiledModuleInvocation>,
    /// Sparse concrete-placement controls. Processing topology and the base
    /// Transition invocation remain shared.
    pub transition_instance_controls:
        HashMap<TransitionModuleInstanceTarget, CompiledTransitionInstanceControls>,
    pub dependencies: DependencyIndex,
}

impl RenderPlan {
    pub fn invocation(&self, host: ModuleHost) -> Option<&CompiledModuleInvocation> {
        self.dependencies
            .invocation_indices
            .get(&host)
            .and_then(|index| self.module_invocations.get(*index))
    }

    pub fn effective_transition_invocation(
        &self,
        host: ModuleHost,
        instance_path: &crate::model::authoring::InstancePath,
    ) -> Option<CompiledModuleInvocation> {
        let mut invocation = self.invocation(host)?.clone();
        let target = TransitionModuleInstanceTarget {
            instance_path: instance_path.clone(),
            transition_id: match host {
                ModuleHost::Transition { transition_id, .. } => transition_id,
                _ => return Some(invocation),
            },
            module_instance_id: invocation.instance_id,
        };
        if let Some(controls) = self.transition_instance_controls.get(&target) {
            invocation.parameter_overrides = controls.parameter_overrides.clone();
            invocation.input_bindings = controls.input_bindings.clone();
            invocation.automation_tracks = controls.automation_tracks.clone();
        }
        Some(invocation)
    }
}

/// One Timeline schedule. Child Timelines are referenced, not flattened.
#[derive(Clone, PartialEq, Debug)]
pub struct CompiledTimeline {
    pub id: TimelineId,
    pub fingerprint: [u8; 32],
    pub schedule: Vec<ScheduledItem>,
    pub track_schedules: HashMap<TimelineTrackId, Vec<usize>>,
    pub transitions: Vec<CompiledTransition>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ScheduledItem {
    pub item_id: TimelineItemId,
    pub track_id: TimelineTrackId,
    pub track_order: usize,
    pub layer: i64,
    pub interval: TimelineInterval,
    pub time_map: TimeMap,
    pub source: PlannedSource,
}

impl ScheduledItem {
    pub fn is_active(&self, timeline_time: MediaTime) -> Result<bool, String> {
        self.interval.contains(timeline_time)
    }

    pub fn local_time(&self, timeline_time: MediaTime) -> Result<MediaTime, String> {
        self.time_map.local_time(self.interval, timeline_time)
    }
}

/// Runtime-relevant source classification. Ordinary Timeline items remain
/// ordinary items; only an explicit Module source is a Node Clip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlannedSource {
    Asset,
    Text,
    Shape,
    Solid,
    Composition { timeline_id: TimelineId },
    Module,
}

/// One reusable executable Module. The `Arc` stored by [`RenderPlan`] is shared
/// by every invocation of the same definition.
#[derive(Clone, PartialEq, Debug)]
pub struct CompiledModuleDefinition {
    pub id: ModuleDefinitionId,
    pub topology_revision: u64,
    pub interface_version: u64,
    pub host_contract: ModuleHostContract,
    pub fingerprint: [u8; 32],
    pub nodes: HashMap<uuid::Uuid, CompiledNode>,
    pub connections: Vec<ModuleConnection>,
    pub parameters: HashMap<PublishedParameterId, PublishedParameter>,
    pub media_inputs: HashMap<PublishedMediaInputId, PublishedMediaInput>,
    pub outputs: HashMap<ModuleOutputId, CompiledModuleOutput>,
    /// GPU particle executables keyed by their existing Module Output. The
    /// topology is compiled once per definition and never expanded per item.
    pub particle_outputs: HashMap<ModuleOutputId, CompiledParticleDefinition>,
    /// Retained at the compiled boundary for the future stateful/event runtime;
    /// the first stateless Image slice does not evaluate these interfaces.
    pub signals: HashMap<PublishedSignalId, PublishedSignal>,
    pub actions: HashMap<PublishedActionId, PublishedAction>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CompiledParticleDefinition {
    pub emitter_node_id: uuid::Uuid,
    pub initialize_node_id: uuid::Uuid,
    pub gravity_node_id: uuid::Uuid,
    pub drag_node_id: uuid::Uuid,
    pub renderer_node_id: uuid::Uuid,
    /// Stable Module-owned mutable state slot. Runtime keys combine it with
    /// InstancePath and ModuleInstanceId before allocating any buffer.
    pub state_slot_id: uuid::Uuid,
}

#[derive(Clone, PartialEq, Debug)]
pub struct CompiledNode {
    pub id: uuid::Uuid,
    pub content: NodeContent,
    pub enabled: bool,
    pub bypassed: bool,
    pub blend_mode: BlendMode,
    pub properties: PropertyMap,
    pub bypass_routes: HashMap<String, String>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct CompiledModuleOutput {
    pub terminal: ModuleOutput,
    /// Connected graph source for each media input on the single Output
    /// terminal. An unconnected input is valid and evaluates to no media; a
    /// Published media input may also target either terminal input.
    pub sources: HashMap<crate::model::project::PortDataType, ModulePortAddress>,
    /// Stable topological order containing only Nodes that can reach this
    /// Output terminal. Dead editor branches never expand an invocation.
    pub evaluation_order: Vec<uuid::Uuid>,
}

/// Lightweight references to the two ordinary schedule entries evaluated by
/// one transition. Neither source is expanded into a processing Node graph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransitionSourceInvocation {
    pub item_id: TimelineItemId,
    pub schedule_index: usize,
    pub output: MediaOutputKind,
    /// Extra source media needed outside this placement's visible interval.
    /// Runtime and UI must validate the backing source and report a missing
    /// handle; they must not require both Timeline placements to overlap.
    pub required_hidden_handle: TransitionHandleRequirement,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransitionHandleRequirement {
    pub before: MediaTime,
    pub after: MediaTime,
}

impl TransitionHandleRequirement {
    pub const fn is_empty(self) -> bool {
        self.before.value() == 0 && self.after.value() == 0
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct CompiledTransition {
    pub id: TransitionId,
    pub edit_point: MediaTime,
    pub from: TransitionSourceInvocation,
    pub to: TransitionSourceInvocation,
    /// The transition replaces both source placements at the `to` item's
    /// deterministic compositing slot. Unrelated schedule entries retain
    /// their ordering around that slot.
    pub output_schedule_index: usize,
    pub progress: NormalizedTransitionProgress,
    pub processor: TransitionProcessor,
    pub parameters: HashMap<String, AutomatableParameter>,
    /// Public host key for an optional reusable Module invocation. Runtime
    /// resolves it through `RenderPlan::invocation`; no graph is expanded per
    /// Transition placement.
    pub module_host: Option<ModuleHost>,
}

/// Timeline-time mapping for the normalized transition input consumed by a
/// processor. Runtime sampling is deterministic and clamped to `0..=1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NormalizedTransitionProgress {
    interval: TimelineInterval,
}

impl NormalizedTransitionProgress {
    pub fn new(interval: TimelineInterval) -> Result<Self, String> {
        if interval.duration <= MediaTime::zero() {
            return Err("Transition progress duration must be greater than zero".to_string());
        }
        Ok(Self { interval })
    }

    pub const fn interval(self) -> TimelineInterval {
        self.interval
    }

    pub fn sample_at(self, timeline_time: MediaTime) -> Result<f64, String> {
        let elapsed = timeline_time.checked_sub(self.interval.start)?;
        Ok((elapsed.to_seconds_f64() / self.interval.duration.to_seconds_f64()).clamp(0.0, 1.0))
    }
}

impl CompiledModuleOutput {
    pub fn source(
        &self,
        data_type: crate::model::project::PortDataType,
    ) -> Option<&ModulePortAddress> {
        self.sources.get(&data_type)
    }
}

/// Host identity is independent from Module-internal Node IDs. New host kinds
/// (Track/Bus/Master stages) extend this enum without changing Module topology.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum ModuleHost {
    TimelineItem {
        timeline_id: TimelineId,
        item_id: TimelineItemId,
    },
    Attachment(AttachmentId),
    Transition {
        timeline_id: TimelineId,
        transition_id: TransitionId,
    },
}

#[derive(Clone, PartialEq, Debug)]
pub struct CompiledModuleInvocation {
    pub host: ModuleHost,
    pub instance_id: ModuleInstanceId,
    pub definition_id: ModuleDefinitionId,
    pub output_id: ModuleOutputId,
    pub parameter_overrides: HashMap<PublishedParameterId, crate::model::property::PropertyValue>,
    pub input_bindings: HashMap<PublishedMediaInputId, MediaInputBinding>,
    pub automation_tracks: HashMap<PublishedParameterId, AutomationTrack>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct CompiledTransitionInstanceControls {
    pub target: TransitionModuleInstanceTarget,
    pub parameter_overrides: HashMap<PublishedParameterId, crate::model::property::PropertyValue>,
    pub input_bindings: HashMap<PublishedMediaInputId, MediaInputBinding>,
    pub automation_tracks: HashMap<PublishedParameterId, AutomationTrack>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct TimelineRangeDependency {
    pub timeline_id: TimelineId,
    pub start: MediaTime,
    pub duration: MediaTime,
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct TimelineInstanceRangeDependency {
    pub target: TransitionModuleInstanceTarget,
    pub timeline_id: TimelineId,
    pub start: MediaTime,
    pub duration: MediaTime,
}

/// Reverse edges used by incremental frame invalidation. These indices refer
/// to public Module/Timeline identities only, never an internal Node UUID.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct DependencyIndex {
    pub timeline_ranges: HashMap<TimelineItemId, TimelineRangeDependency>,
    /// Exact authored range for every invocation host that occupies Timeline
    /// time. Transition Module edits use this instead of invalidating the
    /// whole Project or silently leaving Preview stale.
    pub host_ranges: HashMap<ModuleHost, TimelineRangeDependency>,
    pub definition_invocations: HashMap<ModuleDefinitionId, Vec<ModuleHost>>,
    pub instance_invocations: HashMap<ModuleInstanceId, Vec<ModuleHost>>,
    pub media_input_consumers: HashMap<TimelineItemId, Vec<ModuleHost>>,
    pub invocation_indices: HashMap<ModuleHost, usize>,
    pub transition_instance_ranges:
        HashMap<TransitionModuleInstanceTarget, TimelineInstanceRangeDependency>,
    pub definition_transition_instances:
        HashMap<ModuleDefinitionId, Vec<TransitionModuleInstanceTarget>>,
    pub instance_transition_instances:
        HashMap<ModuleInstanceId, Vec<TransitionModuleInstanceTarget>>,
    pub transition_instance_media_consumers:
        HashMap<TimelineItemId, Vec<TransitionModuleInstanceTarget>>,
}

impl DependencyIndex {
    pub fn affected_by_definition(&self, definition_id: ModuleDefinitionId) -> InvalidationSet {
        let mut affected = self.invalidations_for_hosts(
            self.definition_invocations
                .get(&definition_id)
                .into_iter()
                .flatten()
                .copied(),
        );
        self.extend_transition_instances(
            &mut affected,
            self.definition_transition_instances
                .get(&definition_id)
                .into_iter()
                .flatten(),
        );
        affected
    }

    pub fn affected_by_instance(&self, instance_id: ModuleInstanceId) -> InvalidationSet {
        let mut affected = self.invalidations_for_hosts(
            self.instance_invocations
                .get(&instance_id)
                .into_iter()
                .flatten()
                .copied(),
        );
        self.extend_transition_instances(
            &mut affected,
            self.instance_transition_instances
                .get(&instance_id)
                .into_iter()
                .flatten(),
        );
        affected
    }

    pub fn affected_by_transition_instance(
        &self,
        target: &TransitionModuleInstanceTarget,
    ) -> InvalidationSet {
        let mut affected = InvalidationSet::default();
        self.extend_transition_instances(&mut affected, std::iter::once(target));
        affected
    }

    pub fn affected_by_item(&self, item_id: TimelineItemId) -> InvalidationSet {
        let mut affected = InvalidationSet::default();
        let mut pending = vec![item_id];
        let mut visited = BTreeSet::new();
        while let Some(item_id) = pending.pop() {
            if !visited.insert(item_id) {
                continue;
            }
            if let Some(range) = self.timeline_ranges.get(&item_id) {
                affected.timelines.insert(range.timeline_id);
                affected.ranges.insert(*range);
            }
            for host in self
                .media_input_consumers
                .get(&item_id)
                .into_iter()
                .flatten()
            {
                affected.invocations.insert(*host);
                if let Some(range) = self.host_ranges.get(host) {
                    affected.timelines.insert(range.timeline_id);
                    affected.ranges.insert(*range);
                }
                if let ModuleHost::TimelineItem { item_id, .. } = host {
                    pending.push(*item_id);
                }
            }
            self.extend_transition_instances(
                &mut affected,
                self.transition_instance_media_consumers
                    .get(&item_id)
                    .into_iter()
                    .flatten(),
            );
        }
        affected
    }

    fn invalidations_for_hosts(&self, hosts: impl Iterator<Item = ModuleHost>) -> InvalidationSet {
        let mut affected = InvalidationSet::default();
        for host in hosts {
            affected.invocations.insert(host);
            if let Some(range) = self.host_ranges.get(&host) {
                affected.timelines.insert(range.timeline_id);
                affected.ranges.insert(*range);
                continue;
            }
            if let ModuleHost::TimelineItem {
                timeline_id,
                item_id,
            } = host
            {
                affected.timelines.insert(timeline_id);
                if let Some(range) = self.timeline_ranges.get(&item_id) {
                    affected.ranges.insert(*range);
                }
            }
        }
        affected
    }

    fn extend_transition_instances<'a>(
        &self,
        affected: &mut InvalidationSet,
        targets: impl Iterator<Item = &'a TransitionModuleInstanceTarget>,
    ) {
        for target in targets {
            affected.transition_instances.insert(target.clone());
            if let Some(range) = self.transition_instance_ranges.get(target) {
                affected.timelines.insert(range.timeline_id);
                affected.instance_ranges.insert(range.clone());
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct InvalidationSet {
    pub timelines: BTreeSet<TimelineId>,
    pub ranges: BTreeSet<TimelineRangeDependency>,
    pub invocations: BTreeSet<ModuleHost>,
    pub instance_ranges: BTreeSet<TimelineInstanceRangeDependency>,
    pub transition_instances: BTreeSet<TransitionModuleInstanceTarget>,
}

impl PartialOrd for TimelineRangeDependency {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimelineRangeDependency {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.timeline_id, self.start, self.duration).cmp(&(
            other.timeline_id,
            other.start,
            other.duration,
        ))
    }
}

impl PartialOrd for TimelineInstanceRangeDependency {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimelineInstanceRangeDependency {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.target, self.timeline_id, self.start, self.duration).cmp(&(
            &other.target,
            other.timeline_id,
            other.start,
            other.duration,
        ))
    }
}
