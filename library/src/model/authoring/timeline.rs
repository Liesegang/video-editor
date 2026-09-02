use std::collections::HashMap;

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::model::frame::color::Color;
use crate::model::project::property::{PropertyMap, PropertyValue};

use super::{
    Constraint, MaskId, MatteRef, ModuleInstanceId, TimelineId, TimelineItemId, TimelineTrackId,
    TransitionId,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    pub id: TimelineId,
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: OrderedFloat<f64>,
    pub duration: OrderedFloat<f64>,
    pub background_color: Color,
    pub color_profile: String,
    pub track_order: Vec<TimelineTrackId>,
    pub authored_properties: PropertyMap,
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
#[serde(rename_all = "snake_case")]
pub enum TimelineTrackKind {
    Visual,
    Audio,
    AudioVisual,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TimelineItem {
    pub id: TimelineItemId,
    pub track_id: TimelineTrackId,
    pub name: String,
    pub source: SourceRef,
    pub interval: TimelineInterval,
    pub layer: i64,
    pub parent: Option<TimelineItemId>,
    pub mask_ids: Vec<MaskId>,
    pub matte: Option<MatteRef>,
    pub constraints: Vec<Constraint>,
    pub transition_in: Option<TransitionId>,
    pub transition_out: Option<TransitionId>,
    pub generated_item_id: Option<super::GeneratedItemId>,
    pub authored_properties: PropertyMap,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TimelineInterval {
    pub start: OrderedFloat<f64>,
    pub duration: OrderedFloat<f64>,
}

impl TimelineInterval {
    pub fn new(start: f64, duration: f64) -> Result<Self, String> {
        if !start.is_finite() || start < 0.0 {
            return Err("Timeline interval start must be finite and non-negative".to_string());
        }
        if !duration.is_finite() || duration < 0.0 {
            return Err("Timeline interval duration must be finite and non-negative".to_string());
        }
        Ok(Self {
            start: OrderedFloat(start),
            duration: OrderedFloat(duration),
        })
    }

    pub fn contains(self, time: f64) -> bool {
        time >= self.start.into_inner()
            && time < self.start.into_inner() + self.duration.into_inner()
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SourceRef {
    Asset {
        asset_id: uuid::Uuid,
        time_map: TimeMap,
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
    Module {
        module_instance_id: ModuleInstanceId,
    },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ShapeSource {
    pub shape_kind: ShapeKind,
    pub parameters: HashMap<String, PropertyValue>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Path,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct CompositionInstance {
    pub timeline_id: TimelineId,
    pub time_map: TimeMap,
    pub duration_policy: DurationPolicy,
    pub parameter_overrides: HashMap<String, PropertyValue>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TimeMap {
    pub source_start: OrderedFloat<f64>,
    pub playback_rate: OrderedFloat<f64>,
}

impl Default for TimeMap {
    fn default() -> Self {
        Self {
            source_start: OrderedFloat(0.0),
            playback_rate: OrderedFloat(1.0),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DurationPolicy {
    Fixed,
    Scale,
    Loop,
    Responsive {
        intro_end: OrderedFloat<f64>,
        outro_start: OrderedFloat<f64>,
    },
}

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
