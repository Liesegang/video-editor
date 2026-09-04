use serde::{Deserialize, Serialize};

use crate::model::blend::BlendMode;
use crate::model::project::connection::PortDataType;
use crate::model::project::property::PropertyValue;

use super::{
    AttachmentId, AutomationTrack, ModuleInvocation, TimelineId, TimelineItemId, TimelineTrackId,
};

/// A processor hosted by an authored Timeline stage. Ordinary built-in Effects
/// stay as lightweight entries; only explicit Module uses reference topology.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub id: AttachmentId,
    pub owner: AttachmentOwner,
    pub stage: AttachmentStage,
    pub order: i64,
    pub enabled: bool,
    pub bypassed: bool,
    pub processor: AttachmentProcessor,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttachmentOwner {
    Timeline { timeline_id: TimelineId },
    Track { track_id: TimelineTrackId },
    Item { item_id: TimelineItemId },
}

impl AttachmentOwner {
    pub const fn supports_stage(&self, stage: AttachmentStage) -> bool {
        match self {
            Self::Item { .. } => matches!(
                stage,
                AttachmentStage::ItemTimeMap
                    | AttachmentStage::ItemPreTransform
                    | AttachmentStage::ItemPostTransform
                    | AttachmentStage::AudioPreFader
                    | AttachmentStage::AudioPostFader
            ),
            Self::Track { .. } => matches!(
                stage,
                AttachmentStage::TrackPostComposite | AttachmentStage::TrackPostMix
            ),
            Self::Timeline { .. } => matches!(
                stage,
                AttachmentStage::TimelinePostComposite | AttachmentStage::TimelinePostMix
            ),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AttachmentStage {
    ItemTimeMap,
    ItemPreTransform,
    ItemPostTransform,
    TrackPostComposite,
    TimelinePostComposite,
    AudioPreFader,
    AudioPostFader,
    TrackPostMix,
    TimelinePostMix,
}

impl AttachmentStage {
    /// Media flowing through an Effect evaluation stage. Time Map is reserved
    /// for a Behavior contract and therefore has no Effect media type.
    pub const fn effect_media_type(self) -> Option<PortDataType> {
        match self {
            Self::ItemPreTransform
            | Self::ItemPostTransform
            | Self::TrackPostComposite
            | Self::TimelinePostComposite => Some(PortDataType::Image),
            Self::AudioPreFader
            | Self::AudioPostFader
            | Self::TrackPostMix
            | Self::TimelinePostMix => Some(PortDataType::Audio),
            Self::ItemTimeMap => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AttachmentProcessor {
    BuiltinEffect(BuiltinEffectInstance),
    Module(ModuleInvocation),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct BuiltinEffectInstance {
    pub operation: OperationRef,
    /// Persisted so a Project remains inspectable when an operation is absent.
    pub contract: EffectContractSnapshot,
    pub parameters: std::collections::HashMap<String, BuiltinEffectParameter>,
    pub blend_mode: BlendMode,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct BuiltinEffectParameter {
    pub value: PropertyValue,
    pub automation: Option<AutomationTrack>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct OperationRef {
    pub category: String,
    pub component_id: String,
    pub operation: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct EffectContractSnapshot {
    pub input_type: PortDataType,
    pub output_type: PortDataType,
    pub parameters: Vec<EffectParameterContract>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct EffectParameterContract {
    pub key: String,
    pub data_type: PortDataType,
    pub default_value: PropertyValue,
}
