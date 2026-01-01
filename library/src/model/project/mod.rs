pub mod asset;
pub mod clip_helpers;
pub mod effect;
pub mod ensemble;
pub mod project;
pub mod property;
pub mod style;
mod track_clip_factories;

pub use effect::EffectConfig;
pub use ensemble::{DecoratorInstance, EffectorInstance};

use crate::model::project::property::PropertyMap;
use crate::model::project::style::StyleInstance;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "node_type")]
pub enum Node {
    Track(TrackData),
    Clip(TrackClip),
}

impl Node {
    /// Get the ID of this node
    pub fn id(&self) -> Uuid {
        match self {
            Node::Track(t) => t.id,
            Node::Clip(c) => c.id,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct TrackData {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub child_ids: Vec<Uuid>,
}

impl TrackData {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            child_ids: Vec::new(),
        }
    }

    /// Add a child node ID
    pub fn add_child(&mut self, child_id: Uuid) {
        self.child_ids.push(child_id);
    }

    /// Insert a child node ID at a specific index
    pub fn insert_child(&mut self, index: usize, child_id: Uuid) {
        if index <= self.child_ids.len() {
            self.child_ids.insert(index, child_id);
        } else {
            self.child_ids.push(child_id);
        }
    }

    /// Remove a child node ID
    pub fn remove_child(&mut self, child_id: Uuid) -> bool {
        if let Some(pos) = self.child_ids.iter().position(|id| *id == child_id) {
            self.child_ids.remove(pos);
            true
        } else {
            false
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "lowercase")] // Serialize as "video", "image", etc.
pub enum TrackClipKind {
    Video,
    Image,
    Audio,
    Text,
    Shape,
    SkSL,
    Composition,
    // Add other kinds as needed
}

impl std::fmt::Display for TrackClipKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TrackClipKind::Video => "video",
            TrackClipKind::Image => "image",
            TrackClipKind::Audio => "audio",
            TrackClipKind::Text => "text",
            TrackClipKind::Shape => "shape",
            TrackClipKind::SkSL => "sksl",
            TrackClipKind::Composition => "composition",
        };
        write!(f, "{}", s)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct TrackClip {
    pub id: Uuid,
    pub reference_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub kind: TrackClipKind,
    #[serde(default)]
    pub in_frame: u64,
    #[serde(default)]
    pub out_frame: u64,
    #[serde(default)]
    pub source_begin_frame: i64,
    #[serde(default)]
    pub duration_frame: Option<u64>,

    #[serde(default = "default_fps")]
    pub fps: f64,

    #[serde(default)]
    pub properties: PropertyMap,
    #[serde(default)]
    pub styles: Vec<StyleInstance>,
    #[serde(default)]
    pub effects: Vec<EffectConfig>,
    #[serde(default)]
    pub effectors: Vec<EffectorInstance>,
    #[serde(default)]
    pub decorators: Vec<DecoratorInstance>,
}

impl TrackClip {
    pub fn new(
        id: Uuid,
        reference_id: Option<Uuid>,
        kind: TrackClipKind,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: Option<u64>,
        fps: f64,
        properties: PropertyMap,
        styles: Vec<StyleInstance>,
        effects: Vec<EffectConfig>,
        effectors: Vec<EffectorInstance>,
        decorators: Vec<DecoratorInstance>,
    ) -> Self {
        Self {
            id,
            reference_id,
            kind,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            fps,
            properties,
            styles,
            effects,
            effectors,
            decorators,
        }
    }

    // Ported helper constructors from Entity

    // Helper for consistency with Entity
    pub fn set_constant_property(
        &mut self,
        key: &str,
        value: crate::model::project::property::PropertyValue,
    ) {
        self.properties.set(
            key.to_string(),
            crate::model::project::property::Property::constant(value),
        );
    }

    /// Update or upsert a clip property value/keyframe.
    /// If property exists and is keyframed, upserts keyframe at time.
    /// If property is constant or doesn't exist, sets as constant.
    pub fn update_property_or_keyframe(
        &mut self,
        key: &str,
        time: f64,
        value: property::PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) {
        if let Some(prop) = self.properties.get_mut(key) {
            if prop.evaluator == "keyframe" {
                prop.upsert_keyframe(time, value, easing);
            } else {
                self.properties
                    .set(key.to_string(), property::Property::constant(value));
            }
        } else {
            self.properties
                .set(key.to_string(), property::Property::constant(value));
        }
    }

    /// Update effect property at specified index.
    pub fn update_effect_property(
        &mut self,
        effect_index: usize,
        key: &str,
        time: f64,
        value: property::PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), &'static str> {
        let effect = self
            .effects
            .get_mut(effect_index)
            .ok_or("Effect not found")?;
        effect.update_property_or_keyframe(key, time, value, easing);
        Ok(())
    }

    /// Update style property at specified index.
    pub fn update_style_property(
        &mut self,
        style_index: usize,
        key: &str,
        time: f64,
        value: property::PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), &'static str> {
        let style = self.styles.get_mut(style_index).ok_or("Style not found")?;
        style.update_property_or_keyframe(key, time, value, easing);
        Ok(())
    }
}

const fn default_fps() -> f64 {
    30.0
}
