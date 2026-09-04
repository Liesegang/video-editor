use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::animation::EasingFunction;
use crate::model::frame::color::Color;
use crate::model::project::PortDataType;
use crate::model::project::property::{KeyframeId, PropertyMap, PropertyValue};

use super::{
    CompositionParameterId, MediaTime, ModuleInstanceId, PublishedMediaInputId,
    PublishedMediaOutputId, PublishedParameterId, RationalRate, TimelineId, TimelineItemId,
    TimelineTrackId,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    pub id: TimelineId,
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: RationalRate,
    pub duration: MediaTime,
    pub background_color: Color,
    pub color_profile: String,
    pub track_order: Vec<TimelineTrackId>,
    pub authored_properties: PropertyMap,
    /// Stable public controls for every placement of this nested Timeline.
    /// Targets remain private to the definition; instances store only IDs.
    pub published_parameters: Vec<CompositionParameter>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TimelineTrack {
    pub id: TimelineTrackId,
    pub timeline_id: TimelineId,
    pub name: String,
    pub kind: TimelineTrackKind,
    pub authored_properties: PropertyMap,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TimelineTrackKind {
    Visual,
    Audio,
    AudioVisual,
}

/// One human-authored placement. It never owns Node topology.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TimelineItem {
    pub id: TimelineItemId,
    pub track_id: TimelineTrackId,
    pub name: String,
    pub source: SourceRef,
    pub interval: TimelineInterval,
    /// Maps Timeline time into this placement's local source/animation time.
    pub time_map: TimeMap,
    pub layer: i64,
    pub parent: Option<TimelineItemId>,
    pub authored_properties: PropertyMap,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TimelineInterval {
    pub start: MediaTime,
    pub duration: MediaTime,
}

impl TimelineInterval {
    pub fn new(start: MediaTime, duration: MediaTime) -> Result<Self, String> {
        if start.is_negative() {
            return Err("Timeline interval start must be non-negative".to_string());
        }
        if duration.is_negative() {
            return Err("Timeline interval duration must be finite and non-negative".to_string());
        }
        Ok(Self { start, duration })
    }

    pub fn contains(self, time: MediaTime) -> Result<bool, String> {
        Ok(time >= self.start && time < self.end()?)
    }

    pub fn end(self) -> Result<MediaTime, String> {
        self.start.checked_add(self.duration)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SourceRef {
    Asset {
        asset_id: uuid::Uuid,
    },
    Text {
        text: String,
    },
    Shape {
        shape: ShapeSource,
    },
    Solid {
        color: Color,
    },
    Composition(CompositionInstance),
    /// A user-visible Node Clip. Only the referenced Module owns topology.
    Module(ModuleInvocation),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ShapeSource {
    pub shape_kind: ShapeKind,
    pub parameters: HashMap<String, PropertyValue>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Path,
}

/// Placement payload for a nested Timeline definition.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct CompositionInstance {
    pub timeline_id: TimelineId,
    pub duration_policy: DurationPolicy,
    pub parameter_overrides: HashMap<CompositionParameterId, PropertyValue>,
}

/// One public control owned by a Timeline definition.
///
/// The target may refer to definition-internal items, but callers placing the
/// Timeline can address it only through [`CompositionParameterId`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct CompositionParameter {
    pub id: CompositionParameterId,
    pub name: String,
    pub data_type: PortDataType,
    pub default_value: PropertyValue,
    pub target: CompositionParameterTarget,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionParameterTarget {
    TextContent {
        item_id: TimelineItemId,
    },
    ItemProperty {
        item_id: TimelineItemId,
        property_key: String,
    },
}

impl CompositionParameterTarget {
    pub const fn item_id(&self) -> TimelineItemId {
        match self {
            Self::TextContent { item_id } | Self::ItemProperty { item_id, .. } => *item_id,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct TimeMap {
    pub source_start: MediaTime,
    pub playback_rate: RationalRate,
}

impl TimeMap {
    pub fn local_time(
        self,
        interval: TimelineInterval,
        timeline_time: MediaTime,
    ) -> Result<MediaTime, String> {
        timeline_time
            .checked_sub(interval.start)?
            .checked_mul_rate(self.playback_rate)?
            .checked_add(self.source_start)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DurationPolicy {
    Fixed,
    Scale,
    Loop,
    Responsive {
        intro_end: MediaTime,
        outro_start: MediaTime,
    },
}

/// Runtime nesting address. Item IDs, rather than Module-internal IDs, form
/// the path so repeated nested Timeline placements remain distinguishable.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct InstancePath {
    pub root_timeline_id: TimelineId,
    pub composition_items: Vec<TimelineItemId>,
}

impl InstancePath {
    pub fn root(root_timeline_id: TimelineId) -> Self {
        Self {
            root_timeline_id,
            composition_items: Vec::new(),
        }
    }

    pub fn nested(&self, item_id: TimelineItemId) -> Self {
        let mut composition_items = self.composition_items.clone();
        composition_items.push(item_id);
        Self {
            root_timeline_id: self.root_timeline_id,
            composition_items,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ModuleInvocation {
    pub instance_id: ModuleInstanceId,
    pub output_id: PublishedMediaOutputId,
    pub input_bindings: HashMap<PublishedMediaInputId, MediaInputBinding>,
    /// Keyframes remain owned by the Timeline host, never the Module graph.
    pub automation_tracks: HashMap<PublishedParameterId, AutomationTrack>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaInputBinding {
    TimelineItemOutput {
        locator: InstanceLocator,
        item_id: TimelineItemId,
        output: MediaOutputKind,
        stage: ItemOutputStage,
    },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InstanceLocator {
    SameTimeline,
    Exact(InstancePath),
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaOutputKind {
    Image,
    Audio,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemOutputStage {
    Content,
    PostEffects,
    PostTransform,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct AutomationTrack {
    pub keyframes: Vec<AutomationKeyframe>,
}

impl AutomationTrack {
    pub fn new(keyframe: AutomationKeyframe) -> Result<Self, String> {
        Ok(Self {
            keyframes: vec![keyframe],
        })
    }

    pub fn upsert(
        &mut self,
        time: MediaTime,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<KeyframeId, String> {
        if let Some(existing) = self
            .keyframes
            .iter_mut()
            .find(|keyframe| keyframe.time == time)
        {
            existing.value = value;
            if let Some(easing) = easing {
                existing.easing = easing;
            }
            return Ok(existing.id);
        }
        let keyframe =
            AutomationKeyframe::new(time, value, easing.unwrap_or(EasingFunction::Linear));
        let id = keyframe.id;
        self.keyframes.push(keyframe);
        self.keyframes.sort_by_key(|keyframe| keyframe.time);
        Ok(id)
    }

    pub fn update_keyframe(
        &mut self,
        keyframe_id: KeyframeId,
        time: Option<MediaTime>,
        value: Option<PropertyValue>,
        easing: Option<EasingFunction>,
    ) -> Result<(), String> {
        if time.is_some_and(MediaTime::is_negative) {
            return Err("Automation Keyframe time must be non-negative".to_string());
        }
        if let Some(time) = time
            && self
                .keyframes
                .iter()
                .any(|keyframe| keyframe.id != keyframe_id && keyframe.time == time)
        {
            return Err("Automation already has a Keyframe at that time".to_string());
        }
        let keyframe = self
            .keyframes
            .iter_mut()
            .find(|keyframe| keyframe.id == keyframe_id)
            .ok_or_else(|| format!("Missing Automation Keyframe {keyframe_id}"))?;
        if let Some(time) = time {
            keyframe.time = time;
        }
        if let Some(value) = value {
            keyframe.value = value;
        }
        if let Some(easing) = easing {
            keyframe.easing = easing;
        }
        self.keyframes.sort_by_key(|keyframe| keyframe.time);
        Ok(())
    }

    pub fn remove_keyframe(&mut self, keyframe_id: KeyframeId) -> bool {
        let before = self.keyframes.len();
        self.keyframes.retain(|keyframe| keyframe.id != keyframe_id);
        self.keyframes.len() != before
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct AutomationKeyframe {
    pub id: KeyframeId,
    pub time: MediaTime,
    pub value: PropertyValue,
    pub easing: EasingFunction,
}

impl AutomationKeyframe {
    pub fn new(time: MediaTime, value: PropertyValue, easing: EasingFunction) -> Self {
        Self {
            id: KeyframeId::new(),
            time,
            value,
            easing,
        }
    }
}
