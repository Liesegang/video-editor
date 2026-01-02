use crate::model::project::property::PropertyMap;
use crate::model::project::style::StyleInstance;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod animation;
pub mod frame;
pub mod node_graph;
pub mod project;
pub mod vector;

// Re-export core types for easier access
pub use project::asset::{Asset, AssetKind};
pub use project::effect::EffectConfig;
pub use project::ensemble::{DecoratorInstance, EffectorInstance};
pub use project::{Composite, Project, ResourceGraph};

// Re-export modules to match old structure `crate::model::property` etc.
pub use project::asset;
pub use project::effect;
pub use project::ensemble;
pub use project::property;
pub use project::style;

// ==========================================
// The Trinity: Node Type Definition
// ==========================================

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "node_type")]
pub enum Node {
    /// Composite Bus. Has no duration, groups children (Layer/Track).
    Track(Track),
    /// Time-based Object. References content and places it on the timeline.
    Layer(Layer),
}

impl Node {
    pub fn id(&self) -> Uuid {
        match self {
            Node::Track(t) => t.id,
            Node::Layer(l) => l.id,
        }
    }
}

// ==========================================
// 1. Track (The Bus / Mixer)
// ==========================================

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum BlendMode {
    Normal,
    Add,
    Multiply,
    Screen,
    Overlay,
    // ... others
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Track {
    pub id: Uuid,
    pub name: String,

    /// How to blend into the parent.
    #[serde(default = "default_blend_mode")]
    pub blend_mode: BlendMode,

    /// Properties affecting the whole track (e.g. Opacity).
    #[serde(default)]
    pub properties: PropertyMap,

    /// List of child Node IDs (Layer or Sub-Track).
    /// Rendering order is managed here (index 0 is bottom).
    #[serde(default)]
    pub children: Vec<Uuid>,

    /// Effects applied to the whole track.
    #[serde(default)]
    pub effects: Vec<EffectConfig>,

    // UI Metadata
    #[serde(default)]
    pub ui_position: [f32; 2],
}

fn default_blend_mode() -> BlendMode {
    BlendMode::Normal
}

impl Track {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            blend_mode: BlendMode::Normal,
            properties: PropertyMap::new(),
            children: Vec::new(),
            effects: Vec::new(),
            ui_position: [0.0, 0.0],
        }
    }

    /// Add a child node ID
    pub fn add_child(&mut self, child_id: Uuid) {
        self.children.push(child_id);
    }
}

// ==========================================
// 2. Layer (The Instance / Clipper)
// ==========================================

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Layer {
    pub id: Uuid,
    pub name: String,

    // --- Timeline Placement ---
    /// Start time on the parent track (seconds).
    pub start_time: OrderedFloat<f64>,
    /// Duration of this layer (seconds).
    pub duration: OrderedFloat<f64>,

    // --- Source Trimming ---
    /// Source start time (seconds), aka "In Point".
    pub trim_in: OrderedFloat<f64>,
    /// Playback speed (1.0 = normal, 2.0 = 2x speed).
    pub time_stretch: OrderedFloat<f64>,

    // --- Content ---
    /// What this layer displays.
    pub content: LayerContent,

    // --- Modifiers ---
    /// Transforms (Position, Scale, Rotation) etc.
    #[serde(default)]
    pub properties: PropertyMap,

    #[serde(default)]
    pub styles: Vec<StyleInstance>,
    #[serde(default)]
    pub effects: Vec<EffectConfig>,

    // UI Metadata
    #[serde(default)]
    pub ui_position: [f32; 2],
}

// ==========================================
// Layer Content Definitions
// ==========================================

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", content = "data")]
pub enum LayerContent {
    /// Static media file (Video, Image, Audio).
    Media(MediaContent),

    /// Procedural generation (Shape, Text, Noise).
    Generator(GeneratorContent),

    /// Reference to other Composition or Resource (Important!).
    /// Composite: Nested timeline.
    /// Resource: Global node graph (Particle etc).
    Reference(ReferenceContent),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct MediaContent {
    pub asset_id: Uuid,
    // Stream index etc, asset-specific override.
    pub stream_index: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum GeneratorContent {
    Shape {
        path: String,
        fill: String,
    },
    Text {
        text: String,
        font: String,
    },
    Solid {
        color: crate::model::frame::color::Color,
    },
    SkSL {
        shader: String,
    },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ReferenceContent {
    /// Target ID (Composition ID or Resource ID).
    pub target_id: Uuid,

    /// Time Synchronization Mode.
    /// true: Sync with project global time (for Global Resources).
    /// false: Relative time from Layer In Point (for Nested Compositions).
    pub sync_global_time: bool,

    /// Connection definition to external input ports.
    /// Key: Port Name, Value: Source Node ID (in current context).
    #[serde(default)]
    pub input_mapping: std::collections::HashMap<String, Uuid>,
}

impl Layer {
    pub fn new(name: &str, start_time: f64, duration: f64, content: LayerContent) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            start_time: OrderedFloat(start_time),
            duration: OrderedFloat(duration),
            trim_in: OrderedFloat(0.0),
            time_stretch: OrderedFloat(1.0),
            content,
            properties: PropertyMap::new(),
            styles: Vec::new(),
            effects: Vec::new(),
            ui_position: [0.0, 0.0],
        }
    }

    // Helper if frame-based management is needed
    pub fn end_time(&self) -> f64 {
        self.start_time.into_inner() + self.duration.into_inner()
    }

    /// Unified accessor for property maps (ported from old TrackClip)
    pub fn get_property_map_mut(
        &mut self,
        target: crate::model::project::property::PropertyTarget,
    ) -> Option<&mut crate::model::project::property::PropertyMap> {
        use crate::model::project::property::PropertyTarget;
        match target {
            PropertyTarget::Clip => Some(&mut self.properties), // "Clip" target maps to Layer properties
            PropertyTarget::Effect(i) => self.effects.get_mut(i).map(|e| &mut e.properties),
            PropertyTarget::Style(i) => self.styles.get_mut(i).map(|s| &mut s.properties),
            _ => None, // Effectors/Decorators removed in Trinity or need re-mapping
        }
    }
}
