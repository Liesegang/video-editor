use std::collections::HashMap;

use ordered_float::OrderedFloat;

use crate::model::authoring::{
    AttachmentId, AttachmentOwner, AttachmentStage, EventBindingId, ModuleDefinitionId,
    ModuleInstanceId, SignalBindingId, TimelineId, TimelineInterval, TimelineItemId,
    TimelineTrackId,
};

#[derive(Clone, PartialEq, Debug)]
pub struct RenderPlan {
    pub root_timeline_id: TimelineId,
    pub timelines: HashMap<TimelineId, CompiledTimeline>,
    pub module_definitions: HashMap<ModuleDefinitionId, CompiledModuleDefinition>,
    pub module_invocations: Vec<ModuleInvocation>,
    pub signal_binding_ids: Vec<SignalBindingId>,
    pub event_binding_ids: Vec<EventBindingId>,
    pub dependencies: DependencyIndex,
}

#[derive(Clone, PartialEq, Debug)]
pub struct CompiledTimeline {
    pub id: TimelineId,
    pub schedule: Vec<ScheduledItem>,
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
    pub evaluation_order: Vec<uuid::Uuid>,
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
