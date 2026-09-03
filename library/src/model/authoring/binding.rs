use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::model::project::property::PropertyValue;

use super::{
    EventBindingId, InstancePath, ModuleDefinitionId, ModuleInstanceId, PublishedActionId,
    PublishedParameterId, PublishedSignalId, SignalBindingId,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BindingScope {
    Definition {
        definition_id: ModuleDefinitionId,
    },
    Instance {
        instance_path: InstancePath,
        module_instance_id: ModuleInstanceId,
    },
    Query {
        collection: String,
        predicate: String,
    },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignalSource {
    Published {
        instance_path: InstancePath,
        module_instance_id: ModuleInstanceId,
        signal_id: PublishedSignalId,
    },
    AudioEnvelope {
        channel: String,
    },
    MidiControl {
        device: String,
        control: u16,
    },
    DataField {
        data_source: String,
        field: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum BindingOperator {
    Replace,
    Add,
    Multiply,
    Minimum,
    Maximum,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct SignalMapping {
    pub input_min: OrderedFloat<f64>,
    pub input_max: OrderedFloat<f64>,
    pub output_min: OrderedFloat<f64>,
    pub output_max: OrderedFloat<f64>,
    pub clamp: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct SignalBinding {
    pub id: SignalBindingId,
    pub source: SignalSource,
    pub scope: BindingScope,
    pub target_parameter_id: PublishedParameterId,
    pub mapping: SignalMapping,
    pub operator: BindingOperator,
    pub smoothing_seconds: OrderedFloat<f64>,
    pub priority: i32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventSource {
    Published {
        instance_path: InstancePath,
        module_instance_id: ModuleInstanceId,
        signal_id: PublishedSignalId,
    },
    MidiNoteOn {
        device: String,
        note: u8,
    },
    Marker {
        name: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TriggerPolicy {
    Restart,
    IgnoreWhilePlaying,
    Queue,
    Overlap,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct EventBinding {
    pub id: EventBindingId,
    pub source: EventSource,
    pub scope: BindingScope,
    pub target_action_id: PublishedActionId,
    pub trigger_policy: TriggerPolicy,
    pub priority: i32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct EffectiveValueContribution {
    pub label: String,
    pub value: PropertyValue,
}

#[derive(Clone, PartialEq, Debug)]
pub struct EffectiveValue {
    pub value: PropertyValue,
    pub contributions: Vec<EffectiveValueContribution>,
}
