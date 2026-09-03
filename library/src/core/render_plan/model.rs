use std::collections::{HashMap, HashSet};

use crate::model::project::property::PropertyMap;
use ordered_float::OrderedFloat;

use crate::model::authoring::{
    AttachmentId, AttachmentOwner, AttachmentStage, EventBinding, EventSource, ModuleDefinitionId,
    ModuleInstanceId, PublishedActionId, PublishedParameterId, SignalBinding, SignalSource,
    TimelineId, TimelineInterval, TimelineItemId, TimelineTrackId,
};

#[derive(Clone, PartialEq, Debug)]
pub struct RenderPlan {
    pub root_timeline_id: TimelineId,
    pub timelines: HashMap<TimelineId, CompiledTimeline>,
    pub module_definitions: HashMap<ModuleDefinitionId, CompiledModuleDefinition>,
    pub module_invocations: Vec<ModuleInvocation>,
    pub bindings: CompiledBindingIndex,
    pub dependencies: DependencyIndex,
}

/// Published-interface routes compiled from authored Bindings.
///
/// The runtime indexes by stable public IDs and never addresses Module-internal
/// Node UUIDs. Query scopes are expanded only to matching definitions here;
/// collection membership is still checked by the runtime invocation scope.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct CompiledBindingIndex {
    signal_routes: HashMap<(ModuleDefinitionId, PublishedParameterId), Vec<SignalBinding>>,
    event_routes: HashMap<(ModuleDefinitionId, PublishedActionId), Vec<EventBinding>>,
    signal_sources: HashMap<SignalSource, Vec<SignalBinding>>,
    event_sources: HashMap<EventSource, Vec<EventBinding>>,
}

impl CompiledBindingIndex {
    pub fn signal_bindings(
        &self,
        definition_id: ModuleDefinitionId,
        parameter_id: PublishedParameterId,
    ) -> &[SignalBinding] {
        self.signal_routes
            .get(&(definition_id, parameter_id))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn event_bindings(
        &self,
        definition_id: ModuleDefinitionId,
        action_id: PublishedActionId,
    ) -> &[EventBinding] {
        self.event_routes
            .get(&(definition_id, action_id))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn signal_source_bindings(&self, source: &SignalSource) -> &[SignalBinding] {
        self.signal_sources
            .get(source)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn event_source_bindings(&self, source: &EventSource) -> &[EventBinding] {
        self.event_sources
            .get(source)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(super) fn add_signal(&mut self, definition_id: ModuleDefinitionId, binding: SignalBinding) {
        self.signal_routes
            .entry((definition_id, binding.target_parameter_id))
            .or_default()
            .push(binding);
    }

    pub(super) fn add_event(&mut self, definition_id: ModuleDefinitionId, binding: EventBinding) {
        self.event_routes
            .entry((definition_id, binding.target_action_id))
            .or_default()
            .push(binding);
    }

    pub(super) fn add_signal_source(&mut self, binding: SignalBinding) {
        self.signal_sources
            .entry(binding.source.clone())
            .or_default()
            .push(binding);
    }

    pub(super) fn add_event_source(&mut self, binding: EventBinding) {
        self.event_sources
            .entry(binding.source.clone())
            .or_default()
            .push(binding);
    }

    pub(super) fn finish(&mut self) {
        for routes in self.signal_routes.values_mut() {
            routes.sort_by_key(|binding| (binding.priority, binding.id));
        }
        for routes in self.event_routes.values_mut() {
            routes.sort_by_key(|binding| (binding.priority, binding.id));
        }
        for routes in self.signal_sources.values_mut() {
            routes.sort_by_key(|binding| (binding.priority, binding.id));
        }
        for routes in self.event_sources.values_mut() {
            routes.sort_by_key(|binding| (binding.priority, binding.id));
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct CompiledTimeline {
    pub id: TimelineId,
    pub schedule: Vec<ScheduledItem>,
    pub track_schedules: HashMap<TimelineTrackId, Vec<usize>>,
    pub matte_source_ids: HashSet<TimelineItemId>,
    pub attachment_ids: Vec<AttachmentId>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ScheduledItem {
    pub item_id: TimelineItemId,
    pub track_id: TimelineTrackId,
    pub track_order: usize,
    pub layer: i64,
    pub interval: TimelineInterval,
    pub source: PlannedSource,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlannedSource {
    Asset,
    Text,
    Shape,
    Solid,
    Composition {
        timeline_id: TimelineId,
    },
    Module {
        module_instance_id: ModuleInstanceId,
    },
}

#[derive(Clone, PartialEq, Debug)]
pub struct CompiledModuleDefinition {
    pub id: ModuleDefinitionId,
    pub version: u64,
    pub fingerprint: [u8; 32],
    pub evaluation_order: Vec<uuid::Uuid>,
    pub operations: Vec<CompiledModuleOperation>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum CompiledModuleOperation {
    ImageEffect {
        node_id: uuid::Uuid,
        effect_type: String,
        enabled: bool,
        bypassed: bool,
        properties: PropertyMap,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModuleInvocation {
    pub owner: ModuleInvocationOwner,
    pub module_instance_id: ModuleInstanceId,
    pub definition_id: ModuleDefinitionId,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ModuleInvocationOwner {
    Item(TimelineItemId),
    Attachment {
        attachment_id: AttachmentId,
        owner: AttachmentOwner,
        stage: AttachmentStage,
    },
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct DependencyIndex {
    pub timeline_ranges:
        HashMap<TimelineItemId, (TimelineId, OrderedFloat<f64>, OrderedFloat<f64>)>,
    pub definition_invocations: HashMap<ModuleDefinitionId, Vec<usize>>,
}
