use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

use super::{BlendMode, Clip, Node, NodeContent, Track};
use crate::model::frame::color::Color;
use crate::model::project::asset::Asset;
use crate::model::project::property::PropertyMap;

pub mod asset;
pub mod clip_helpers;
pub mod connection;
mod output_binding;
pub mod property;

pub use connection::{
    AUDIO_OUTPUT_PORT, ContainerAudioSource, ContainerAudioSourceKind, ContainerGraphSemantics,
    ContainerImageSource, ContainerImageSourceKind, DURATION_PORT, EvalOutput, EvalResult,
    EvaluationError, FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, FPS_PORT, FRAME_PORT,
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT,
    NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT, PortAddress, PortDataType, PortDefinition,
    PortDirection, PortExposure, PortMultiplicity, PortOwner, PortSide, ProjectConnection,
    RESOLUTION_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Project {
    pub name: String,
    pub compositions: Vec<Composition>,
    #[serde(default)]
    pub tracks: HashMap<Uuid, Track>,
    #[serde(default)]
    pub clips: HashMap<Uuid, Clip>,
    #[serde(default)]
    pub nodes: HashMap<Uuid, Node>,
    #[serde(default)]
    pub resources: HashMap<Uuid, ResourceGraph>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub export: ExportConfig,
    #[serde(default)]
    pub connections: Vec<ProjectConnection>,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct ExportConfig {
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub pixel_format: Option<String>,
    #[serde(default)]
    pub width: Option<u64>,
    #[serde(default)]
    pub height: Option<u64>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub video_bitrate: Option<u64>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub audio_bitrate: Option<u64>,
    #[serde(default)]
    pub audio_channels: Option<u16>,
    #[serde(default)]
    pub audio_sample_rate: Option<u32>,
    #[serde(default)]
    pub crf: Option<u8>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub parameters: HashMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionSettingsError {
    #[error("width must be at least 1")]
    WidthZero,
    #[error("height must be at least 1")]
    HeightZero,
    #[error("FPS must be finite and greater than 0")]
    InvalidFps,
    #[error("duration must be finite and non-negative")]
    InvalidDuration,
    #[error("FPS multiplied by duration must be a finite frame count representable as u64")]
    FrameCountOutOfRange,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub id: Uuid,
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: f64,
    pub duration: f64,
    pub background_color: Color,
    #[serde(default = "default_color_profile")]
    pub color_profile: String,
    #[serde(default)]
    pub work_area_in: u64,
    #[serde(default)]
    pub work_area_out: u64,
    #[serde(default)]
    pub blend_mode: BlendMode,
    #[serde(default)]
    pub properties: PropertyMap,
    #[serde(default)]
    pub track_ids: Vec<Uuid>,
    #[serde(default)]
    pub node_ids: Vec<Uuid>,
    #[serde(default)]
    pub output_node_id: Option<Uuid>,
    /// Explicit graph result for the Composition audio output. This is
    /// independent from the image binding above.
    #[serde(deserialize_with = "deserialize_required_audio_output_node_id")]
    pub audio_output_node_id: Option<Uuid>,
    #[serde(default)]
    pub ui_position: [f32; 2],
    #[serde(default = "default_composition_ui_size")]
    pub ui_size: [f32; 2],
    #[serde(default)]
    pub ui_collapsed: bool,
}

fn default_composition_ui_size() -> [f32; 2] {
    [1280.0, 860.0]
}

fn deserialize_required_audio_output_node_id<'de, D>(
    deserializer: D,
) -> Result<Option<Uuid>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Uuid>::deserialize(deserializer)
}

fn default_color_profile() -> String {
    "sRGB".to_string()
}

impl Composition {
    pub fn validate_settings(
        width: u64,
        height: u64,
        fps: f64,
        duration: f64,
    ) -> Result<u64, CompositionSettingsError> {
        if width == 0 {
            return Err(CompositionSettingsError::WidthZero);
        }
        if height == 0 {
            return Err(CompositionSettingsError::HeightZero);
        }
        if !fps.is_finite() || fps <= 0.0 {
            return Err(CompositionSettingsError::InvalidFps);
        }
        if !duration.is_finite() || duration < 0.0 {
            return Err(CompositionSettingsError::InvalidDuration);
        }
        Self::checked_frame_count(fps, duration)
            .ok_or(CompositionSettingsError::FrameCountOutOfRange)
    }

    pub fn checked_frame_count(fps: f64, duration: f64) -> Option<u64> {
        let frame_count = (fps * duration).ceil();
        // `u64::MAX as f64` rounds to 2^64, the first value that Rust's
        // float-to-integer cast would saturate instead of represent exactly.
        (frame_count.is_finite() && frame_count >= 0.0 && frame_count < u64::MAX as f64)
            .then_some(frame_count as u64)
    }

    pub fn frame_count(&self) -> Result<u64, CompositionSettingsError> {
        Self::validate_settings(self.width, self.height, self.fps, self.duration)
    }

    pub fn new(name: &str, width: u64, height: u64, fps: f64, duration: f64) -> (Self, Track) {
        let first_track = Track::new("Track 1");
        (
            Self {
                id: Uuid::new_v4(),
                name: name.to_string(),
                width,
                height,
                fps,
                duration,
                background_color: Color::black(),
                color_profile: default_color_profile(),
                work_area_in: 0,
                work_area_out: Self::checked_frame_count(fps, duration).unwrap_or_default(),
                blend_mode: BlendMode::Normal,
                properties: PropertyMap::new(),
                track_ids: vec![first_track.id],
                node_ids: Vec::new(),
                output_node_id: None,
                audio_output_node_id: None,
                ui_position: [0.0, 0.0],
                ui_size: default_composition_ui_size(),
                ui_collapsed: false,
            },
            first_track,
        )
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ResourceGraph {
    pub id: Uuid,
    pub name: String,
    pub kind: ResourceKind,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum ResourceKind {
    ParticleSystem,
    PhysicsWorld,
    ProceduralTexture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeContainer {
    Composition(Uuid),
    Track(Uuid),
    Clip(Uuid),
}

/// A detached set of Nodes and canonical connections that can be inserted
/// into one Composition, Track, or Clip as a single Project transaction.
///
/// `output_node_id` is optional because helper-only graphs (for example a
/// detached style operation) need not replace a container's current image
/// output. When present, it must identify one of `nodes` and declare an Image
/// output port.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeGraphBundle {
    pub nodes: Vec<Node>,
    pub connections: Vec<ProjectConnection>,
    pub output_node_id: Option<Uuid>,
}

impl NodeGraphBundle {
    pub fn new(
        nodes: Vec<Node>,
        connections: Vec<ProjectConnection>,
        output_node_id: Option<Uuid>,
    ) -> Self {
        Self {
            nodes,
            connections,
            output_node_id,
        }
    }

    pub fn with_output_node(node: Node) -> Self {
        let output_node_id = Some(node.id);
        Self::new(vec![node], Vec::new(), output_node_id)
    }

    pub fn output_node(&self) -> Option<&Node> {
        let output_node_id = self.output_node_id?;
        self.nodes.iter().find(|node| node.id == output_node_id)
    }

    pub fn output_node_mut(&mut self) -> Option<&mut Node> {
        let output_node_id = self.output_node_id?;
        self.nodes.iter_mut().find(|node| node.id == output_node_id)
    }
}

impl NodeContainer {
    pub fn id(self) -> Uuid {
        match self {
            Self::Composition(id) | Self::Track(id) | Self::Clip(id) => id,
        }
    }
}

#[derive(Clone, Debug)]
struct ConnectionSourceRemap {
    connection_index: usize,
    original: PortAddress,
    replacement: PortAddress,
}

#[derive(Clone, Debug)]
struct ContainerNodeState {
    container: NodeContainer,
    node_ids: Vec<Uuid>,
    output_node_id: Option<Uuid>,
    audio_output_node_id: Option<Uuid>,
}

/// A side-effect-free Project integrity diagnostic. The adjacent JSON
/// representation is part of the QA API contract: `code` is stable and all
/// variant fields remain available as structured `context`.
#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "context", rename_all = "snake_case")]
pub enum ProjectGraphError {
    #[error("composition id {0} occurs more than once")]
    DuplicateCompositionId(Uuid),
    #[error("track map key {key} does not match entity id {entity_id}")]
    TrackKeyMismatch { key: Uuid, entity_id: Uuid },
    #[error("clip map key {key} does not match entity id {entity_id}")]
    ClipKeyMismatch { key: Uuid, entity_id: Uuid },
    #[error("node map key {key} does not match entity id {entity_id}")]
    NodeKeyMismatch { key: Uuid, entity_id: Uuid },
    #[error("resource map key {key} does not match entity id {entity_id}")]
    ResourceKeyMismatch { key: Uuid, entity_id: Uuid },
    #[error("asset id {0} occurs more than once")]
    DuplicateAssetId(Uuid),
    #[error("connection id {0} occurs more than once")]
    DuplicateConnectionId(Uuid),
    #[error("composition {composition_id} has invalid settings: {reason}")]
    InvalidCompositionSettings {
        composition_id: Uuid,
        reason: CompositionSettingsError,
    },
    #[error(
        "composition {composition_id} has invalid half-open work area {work_area_in}..{work_area_out}; valid frames are 0..{frame_count}"
    )]
    InvalidCompositionWorkArea {
        composition_id: Uuid,
        work_area_in: u64,
        work_area_out: u64,
        frame_count: u64,
    },
    #[error("node {0} does not exist")]
    NodeNotFound(Uuid),
    #[error("node graph bundle is empty")]
    EmptyNodeGraph,
    #[error("node graph bundle contains node id {0} more than once")]
    DuplicateNodeGraphNodeId(Uuid),
    #[error("node graph bundle node {0} already exists in the Project")]
    NodeGraphNodeAlreadyExists(Uuid),
    #[error("node graph bundle contains connection id {0} more than once")]
    DuplicateNodeGraphConnectionId(Uuid),
    #[error("node graph bundle connection {0} already exists in the Project")]
    NodeGraphConnectionAlreadyExists(Uuid),
    #[error("node graph bundle connection {0} does not touch a bundled node")]
    NodeGraphConnectionOutsideBundle(Uuid),
    #[error("node graph output {0} is not one of the bundled nodes")]
    NodeGraphOutputNotBundled(Uuid),
    #[error("clip {0} does not exist")]
    ClipNotFound(Uuid),
    #[error("track {0} does not exist")]
    TrackNotFound(Uuid),
    #[error("composition {0} does not exist")]
    CompositionNotFound(Uuid),
    #[error("track {track_id} is not a top-level child of composition {composition_id}")]
    TrackNotInComposition {
        track_id: Uuid,
        composition_id: Uuid,
    },
    #[error("track {track_id} is contained by more than one composition (first: {composition_id})")]
    TrackAlreadyContained {
        track_id: Uuid,
        composition_id: Uuid,
    },
    #[error("track {0} is not contained by a composition")]
    TrackHasNoComposition(Uuid),
    #[error("clip {clip_id} is not a child of track {track_id}")]
    ClipNotInTrack { clip_id: Uuid, track_id: Uuid },
    #[error("clip {clip_id} is contained by more than one track (first: {track_id})")]
    ClipAlreadyContained { clip_id: Uuid, track_id: Uuid },
    #[error("clip {0} is not contained by a track")]
    ClipHasNoTrack(Uuid),
    #[error("node {node_id} is already contained by {container:?}")]
    NodeAlreadyContained {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("node {node_id} is not directly contained by {container:?}")]
    NodeNotInContainer {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("node {0} is not contained by a composition, track, or clip")]
    NodeHasNoContainer(Uuid),
    #[error("output node {node_id} is not directly contained by {container:?}")]
    OutputNodeOutsideContainer {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("output node {node_id} in {container:?} does not declare an image output port")]
    OutputNodeHasNoImagePort {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("audio output node {node_id} in {container:?} does not declare an audio output port")]
    OutputNodeHasNoAudioPort {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("plugin operation node {node_id} has an empty {field} identifier")]
    EmptyPluginOperationIdentifier { node_id: Uuid, field: String },
    #[error("plugin operation node {node_id} declares an empty {direction:?} port key")]
    EmptyNodePortKey {
        node_id: Uuid,
        direction: PortDirection,
    },
    #[error("plugin operation node {node_id} declares {direction:?} port '{key}' more than once")]
    DuplicateNodePort {
        node_id: Uuid,
        key: String,
        direction: PortDirection,
    },
    #[error(
        "plugin operation node {node_id} port '{key}' uses container-only exposure {exposure:?}"
    )]
    InvalidNodePortExposure {
        node_id: Uuid,
        key: String,
        exposure: PortExposure,
    },
    #[error("plugin operation node {node_id} output port '{key}' cannot be variadic")]
    VariadicNodeOutput { node_id: Uuid, key: String },
    #[error("port does not exist: {0:?}")]
    PortNotFound(PortAddress),
    #[error("connection {0} does not exist")]
    ConnectionNotFound(Uuid),
    #[error(
        "connection {connection_id} cannot use {blend_mode:?}: non-normal blending requires an Image source"
    )]
    ConnectionBlendRequiresImageSource {
        connection_id: Uuid,
        blend_mode: BlendMode,
    },
    #[error(
        "connection {connection_id} cannot use {blend_mode:?}: target {target:?} is not a Merge images input"
    )]
    ConnectionBlendRequiresMergeImagesInput {
        connection_id: Uuid,
        blend_mode: BlendMode,
        target: PortAddress,
    },
    #[error("cannot splice through occupied single input: {target:?}")]
    SpliceInputOccupied { target: PortAddress },
    #[error("cannot connect {source_type:?} to {target_type:?}")]
    IncompatiblePortTypes {
        source_type: PortDataType,
        target_type: PortDataType,
    },
    #[error("internal port on {source_owner:?} cannot target {target_owner:?}")]
    InternalPortEscapesContainer {
        source_owner: PortOwner,
        target_owner: PortOwner,
    },
    #[error("single input port has more than one source: {target:?}")]
    DuplicateInputConnection { target: PortAddress },
    #[error("variadic input {target:?} has duplicate order {order}")]
    DuplicateConnectionOrder { target: PortAddress, order: i64 },
    #[error("connection {connection_id} uses unsupported render-time type {data_type:?}")]
    UnsupportedConnectionType {
        connection_id: Uuid,
        data_type: PortDataType,
    },
    #[error("graph connection would create an evaluation cycle from {from:?} to {to:?}")]
    ConnectionCycle { from: PortOwner, to: PortOwner },
}

impl Project {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            compositions: Vec::new(),
            tracks: HashMap::new(),
            clips: HashMap::new(),
            nodes: HashMap::new(),
            resources: HashMap::new(),
            assets: Vec::new(),
            export: ExportConfig::default(),
            connections: Vec::new(),
        }
    }

    pub fn load(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn save(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn add_composition(&mut self, composition: Composition) {
        self.compositions.push(composition);
    }

    pub fn add_track(&mut self, track: Track) {
        self.tracks.insert(track.id, track);
    }

    pub fn add_clip(&mut self, clip: Clip) {
        self.clips.insert(clip.id, clip);
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id, node);
    }

    /// Insert a detached Node graph into one container as a single Project
    /// transaction. The receiver is unchanged if identity, containment, port,
    /// connection, cycle, or output validation fails.
    pub fn insert_node_graph(
        &mut self,
        container: NodeContainer,
        graph: NodeGraphBundle,
    ) -> Result<(), ProjectGraphError> {
        self.insert_node_graph_at(container, graph, None)
    }

    /// Variant of [`Project::insert_node_graph`] that inserts the bundled
    /// Nodes at a stable position while preserving their bundle order.
    pub fn insert_node_graph_at(
        &mut self,
        container: NodeContainer,
        graph: NodeGraphBundle,
        insert_index: Option<usize>,
    ) -> Result<(), ProjectGraphError> {
        if graph.nodes.is_empty() {
            return Err(ProjectGraphError::EmptyNodeGraph);
        }
        match container {
            NodeContainer::Composition(id) if self.get_composition(id).is_none() => {
                return Err(ProjectGraphError::CompositionNotFound(id));
            }
            NodeContainer::Track(id) if self.get_track(id).is_none() => {
                return Err(ProjectGraphError::TrackNotFound(id));
            }
            NodeContainer::Clip(id) if self.get_clip(id).is_none() => {
                return Err(ProjectGraphError::ClipNotFound(id));
            }
            _ => {}
        }

        let mut node_ids = HashSet::new();
        for node in &graph.nodes {
            if !node_ids.insert(node.id) {
                return Err(ProjectGraphError::DuplicateNodeGraphNodeId(node.id));
            }
            if self.nodes.contains_key(&node.id) {
                return Err(ProjectGraphError::NodeGraphNodeAlreadyExists(node.id));
            }
        }
        if let Some(output_node_id) = graph.output_node_id
            && !node_ids.contains(&output_node_id)
        {
            return Err(ProjectGraphError::NodeGraphOutputNotBundled(output_node_id));
        }

        let existing_connection_ids = self
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<HashSet<_>>();
        let mut connection_ids = HashSet::new();
        for connection in &graph.connections {
            if !connection_ids.insert(connection.id) {
                return Err(ProjectGraphError::DuplicateNodeGraphConnectionId(
                    connection.id,
                ));
            }
            if existing_connection_ids.contains(&connection.id) {
                return Err(ProjectGraphError::NodeGraphConnectionAlreadyExists(
                    connection.id,
                ));
            }
            let touches_bundled_node = [connection.from.owner, connection.to.owner]
                .into_iter()
                .any(|owner| {
                    matches!(owner, PortOwner::Node(node_id) if node_ids.contains(&node_id))
                });
            if !touches_bundled_node {
                return Err(ProjectGraphError::NodeGraphConnectionOutsideBundle(
                    connection.id,
                ));
            }
        }

        let validation_baseline = self.validate_connections();
        let mut candidate = self.clone();
        let bundled_node_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        for node in graph.nodes {
            candidate.nodes.insert(node.id, node);
        }
        let container_node_ids =
            candidate
                .container_node_ids_mut(container)
                .ok_or(match container {
                    NodeContainer::Composition(id) => ProjectGraphError::CompositionNotFound(id),
                    NodeContainer::Track(id) => ProjectGraphError::TrackNotFound(id),
                    NodeContainer::Clip(id) => ProjectGraphError::ClipNotFound(id),
                })?;
        let insert_index = insert_index
            .unwrap_or(container_node_ids.len())
            .min(container_node_ids.len());
        container_node_ids.splice(insert_index..insert_index, bundled_node_ids);
        candidate.connections.extend(graph.connections);
        if let Some(output_node_id) = graph.output_node_id {
            candidate.set_output_node(container, Some(output_node_id))?;
        }

        if let Some(error) = first_new_project_validation_error(
            &validation_baseline,
            candidate.validate_connections(),
        ) {
            return Err(error);
        }
        *self = candidate;
        Ok(())
    }

    pub fn get_composition(&self, id: Uuid) -> Option<&Composition> {
        self.compositions
            .iter()
            .find(|composition| composition.id == id)
    }

    pub fn get_composition_mut(&mut self, id: Uuid) -> Option<&mut Composition> {
        self.compositions
            .iter_mut()
            .find(|composition| composition.id == id)
    }

    pub fn get_track(&self, id: Uuid) -> Option<&Track> {
        self.tracks.get(&id)
    }

    pub fn get_track_mut(&mut self, id: Uuid) -> Option<&mut Track> {
        self.tracks.get_mut(&id)
    }

    pub fn get_clip(&self, id: Uuid) -> Option<&Clip> {
        self.clips.get(&id)
    }

    pub fn get_clip_mut(&mut self, id: Uuid) -> Option<&mut Clip> {
        self.clips.get_mut(&id)
    }

    pub fn get_node(&self, id: Uuid) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn get_node_mut(&mut self, id: Uuid) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    pub fn get_asset(&self, id: Uuid) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.id == id)
    }

    pub fn find_composition_for_track(&self, track_id: Uuid) -> Option<Uuid> {
        self.compositions
            .iter()
            .find(|composition| composition.track_ids.contains(&track_id))
            .map(|composition| composition.id)
    }

    pub fn find_track_for_clip(&self, clip_id: Uuid) -> Option<Uuid> {
        self.tracks
            .iter()
            .find(|(_, track)| track.clip_ids.contains(&clip_id))
            .map(|(track_id, _)| *track_id)
    }

    pub fn find_node_container(&self, node_id: Uuid) -> Option<NodeContainer> {
        self.compositions
            .iter()
            .find(|composition| composition.node_ids.contains(&node_id))
            .map(|composition| NodeContainer::Composition(composition.id))
            .or_else(|| {
                self.tracks
                    .iter()
                    .find(|(_, track)| track.node_ids.contains(&node_id))
                    .map(|(track_id, _)| NodeContainer::Track(*track_id))
            })
            .or_else(|| {
                self.clips
                    .iter()
                    .find(|(_, clip)| clip.node_ids.contains(&node_id))
                    .map(|(clip_id, _)| NodeContainer::Clip(*clip_id))
            })
    }

    pub fn find_parent_clip(&self, node_id: Uuid) -> Option<Uuid> {
        match self.find_node_container(node_id) {
            Some(NodeContainer::Clip(id)) => Some(id),
            _ => None,
        }
    }

    pub fn find_parent_track(&self, id: Uuid) -> Option<Uuid> {
        if let Some(track_id) = self.find_track_for_clip(id) {
            return Some(track_id);
        }
        match self.find_node_container(id) {
            Some(NodeContainer::Track(track_id)) => Some(track_id),
            Some(NodeContainer::Clip(clip_id)) => self.find_track_for_clip(clip_id),
            _ => None,
        }
    }

    pub fn find_containing_composition(&self, id: Uuid) -> Option<Uuid> {
        if self.get_composition(id).is_some() {
            return Some(id);
        }
        if self.get_track(id).is_some() {
            return self.find_composition_for_track(id);
        }
        if self.get_clip(id).is_some() {
            return self
                .find_track_for_clip(id)
                .and_then(|track_id| self.find_composition_for_track(track_id));
        }
        match self.find_node_container(id)? {
            NodeContainer::Composition(composition_id) => Some(composition_id),
            NodeContainer::Track(track_id) => self.find_composition_for_track(track_id),
            NodeContainer::Clip(clip_id) => self
                .find_track_for_clip(clip_id)
                .and_then(|track_id| self.find_composition_for_track(track_id)),
        }
    }

    pub fn attach_track_to_composition(
        &mut self,
        composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<(), ProjectGraphError> {
        self.attach_track_to_composition_at(composition_id, track_id, None)
    }

    pub fn attach_track_to_composition_at(
        &mut self,
        composition_id: Uuid,
        track_id: Uuid,
        index: Option<usize>,
    ) -> Result<(), ProjectGraphError> {
        if !self.tracks.contains_key(&track_id) {
            return Err(ProjectGraphError::TrackNotFound(track_id));
        }
        if self.get_composition(composition_id).is_none() {
            return Err(ProjectGraphError::CompositionNotFound(composition_id));
        }

        let validation_baseline = self.validate_connections();
        let old_parent = self.find_composition_for_track(track_id);
        let remaps = old_parent
            .filter(|old_parent| *old_parent != composition_id)
            .map(|old_parent| {
                self.internal_metadata_reparent_plan(
                    PortOwner::Composition(old_parent),
                    PortOwner::Composition(composition_id),
                    PortOwner::Track(track_id),
                )
            })
            .transpose()?
            .unwrap_or_default();
        let containment_backup = self
            .compositions
            .iter()
            .filter(|composition| {
                composition.id == composition_id || composition.track_ids.contains(&track_id)
            })
            .map(|composition| (composition.id, composition.track_ids.clone()))
            .collect::<Vec<_>>();

        self.detach_track(track_id);
        let composition = self
            .get_composition_mut(composition_id)
            .ok_or(ProjectGraphError::CompositionNotFound(composition_id))?;
        let index = index
            .unwrap_or(composition.track_ids.len())
            .min(composition.track_ids.len());
        composition.track_ids.insert(index, track_id);
        self.apply_connection_source_remaps(&remaps);

        if let Some(error) =
            first_new_project_validation_error(&validation_baseline, self.validate_connections())
        {
            for (composition_id, track_ids) in containment_backup {
                if let Some(composition) = self.get_composition_mut(composition_id) {
                    composition.track_ids = track_ids;
                }
            }
            self.rollback_connection_source_remaps(&remaps);
            return Err(error);
        }
        Ok(())
    }

    pub fn move_track_within_composition(
        &mut self,
        composition_id: Uuid,
        track_id: Uuid,
        destination_index: usize,
    ) -> Result<bool, ProjectGraphError> {
        if !self.tracks.contains_key(&track_id) {
            return Err(ProjectGraphError::TrackNotFound(track_id));
        }
        let composition = self
            .get_composition_mut(composition_id)
            .ok_or(ProjectGraphError::CompositionNotFound(composition_id))?;
        let source_index = composition
            .track_ids
            .iter()
            .position(|candidate| *candidate == track_id)
            .ok_or(ProjectGraphError::TrackNotInComposition {
                track_id,
                composition_id,
            })?;
        let destination_index = destination_index.min(composition.track_ids.len() - 1);
        if source_index == destination_index {
            return Ok(false);
        }
        let track = composition.track_ids.remove(source_index);
        composition.track_ids.insert(destination_index, track);
        Ok(true)
    }

    pub fn detach_track(&mut self, track_id: Uuid) -> bool {
        let mut removed = false;
        for composition in &mut self.compositions {
            let old_len = composition.track_ids.len();
            composition.track_ids.retain(|id| *id != track_id);
            removed |= composition.track_ids.len() != old_len;
        }
        removed
    }

    pub fn attach_clip_to_track(
        &mut self,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), ProjectGraphError> {
        self.attach_clip_to_track_at(track_id, clip_id, None)
    }

    pub fn attach_clip_to_track_at(
        &mut self,
        track_id: Uuid,
        clip_id: Uuid,
        index: Option<usize>,
    ) -> Result<(), ProjectGraphError> {
        if !self.clips.contains_key(&clip_id) {
            return Err(ProjectGraphError::ClipNotFound(clip_id));
        }
        if !self.tracks.contains_key(&track_id) {
            return Err(ProjectGraphError::TrackNotFound(track_id));
        }

        let validation_baseline = self.validate_connections();
        let old_parent = self.find_track_for_clip(clip_id);
        let remaps = old_parent
            .filter(|old_parent| *old_parent != track_id)
            .map(|old_parent| {
                self.internal_metadata_reparent_plan(
                    PortOwner::Track(old_parent),
                    PortOwner::Track(track_id),
                    PortOwner::Clip(clip_id),
                )
            })
            .transpose()?
            .unwrap_or_default();
        let containment_backup = self
            .tracks
            .iter()
            .filter(|(candidate_id, track)| {
                **candidate_id == track_id || track.clip_ids.contains(&clip_id)
            })
            .map(|(candidate_id, track)| (*candidate_id, track.clip_ids.clone()))
            .collect::<Vec<_>>();

        self.detach_clip(clip_id);
        let track = self
            .tracks
            .get_mut(&track_id)
            .ok_or(ProjectGraphError::TrackNotFound(track_id))?;
        let index = index
            .unwrap_or(track.clip_ids.len())
            .min(track.clip_ids.len());
        track.clip_ids.insert(index, clip_id);
        self.apply_connection_source_remaps(&remaps);

        if let Some(error) =
            first_new_project_validation_error(&validation_baseline, self.validate_connections())
        {
            for (track_id, clip_ids) in containment_backup {
                if let Some(track) = self.get_track_mut(track_id) {
                    track.clip_ids = clip_ids;
                }
            }
            self.rollback_connection_source_remaps(&remaps);
            return Err(error);
        }
        Ok(())
    }

    pub fn detach_clip(&mut self, clip_id: Uuid) -> bool {
        let mut removed = false;
        for track in self.tracks.values_mut() {
            let old_len = track.clip_ids.len();
            track.clip_ids.retain(|id| *id != clip_id);
            removed |= track.clip_ids.len() != old_len;
        }
        removed
    }

    pub fn attach_node_to_container(
        &mut self,
        container: NodeContainer,
        node_id: Uuid,
    ) -> Result<(), ProjectGraphError> {
        self.attach_node_to_container_at(container, node_id, None)
    }

    pub fn attach_node_to_container_at(
        &mut self,
        container: NodeContainer,
        node_id: Uuid,
        index: Option<usize>,
    ) -> Result<(), ProjectGraphError> {
        if !self.nodes.contains_key(&node_id) {
            return Err(ProjectGraphError::NodeNotFound(node_id));
        }
        let missing_container = match container {
            NodeContainer::Composition(id) => self.get_composition(id).is_none(),
            NodeContainer::Track(id) => !self.tracks.contains_key(&id),
            NodeContainer::Clip(id) => !self.clips.contains_key(&id),
        };
        if missing_container {
            return Err(match container {
                NodeContainer::Composition(id) => ProjectGraphError::CompositionNotFound(id),
                NodeContainer::Track(id) => ProjectGraphError::TrackNotFound(id),
                NodeContainer::Clip(id) => ProjectGraphError::ClipNotFound(id),
            });
        }

        let validation_baseline = self.validate_connections();
        let old_container = self.find_node_container(node_id);
        let new_parent = port_owner_for_container(container);
        let remaps = old_container
            .filter(|old_container| *old_container != container)
            .map(|old_container| {
                self.internal_metadata_reparent_plan(
                    port_owner_for_container(old_container),
                    new_parent,
                    PortOwner::Node(node_id),
                )
            })
            .transpose()?
            .unwrap_or_default();
        let containment_backup = self
            .container_views()
            .into_iter()
            .filter(|(candidate, ids, image_output, audio_output)| {
                *candidate == container
                    || ids.contains(&node_id)
                    || *image_output == Some(node_id)
                    || *audio_output == Some(node_id)
            })
            .map(
                |(candidate, ids, output_node_id, audio_output_node_id)| ContainerNodeState {
                    container: candidate,
                    node_ids: ids.clone(),
                    output_node_id,
                    audio_output_node_id,
                },
            )
            .collect::<Vec<_>>();

        self.detach_node(node_id);
        let ids = self
            .container_node_ids_mut(container)
            .ok_or(match container {
                NodeContainer::Composition(id) => ProjectGraphError::CompositionNotFound(id),
                NodeContainer::Track(id) => ProjectGraphError::TrackNotFound(id),
                NodeContainer::Clip(id) => ProjectGraphError::ClipNotFound(id),
            })?;
        let index = index.unwrap_or(ids.len()).min(ids.len());
        ids.insert(index, node_id);
        if let Some(output_node_id) = containment_backup
            .iter()
            .find(|state| state.container == container)
            .map(|state| state.output_node_id)
        {
            self.set_container_output_node_unchecked(container, output_node_id);
        }
        if let Some(audio_output_node_id) = containment_backup
            .iter()
            .find(|state| state.container == container)
            .map(|state| state.audio_output_node_id)
        {
            self.set_container_audio_output_node_unchecked(container, audio_output_node_id);
        }
        self.apply_connection_source_remaps(&remaps);

        if let Some(error) =
            first_new_project_validation_error(&validation_baseline, self.validate_connections())
        {
            for state in containment_backup {
                self.restore_container_node_state(state);
            }
            self.rollback_connection_source_remaps(&remaps);
            return Err(error);
        }
        Ok(())
    }

    pub fn detach_node(&mut self, node_id: Uuid) -> bool {
        let mut removed = false;
        for composition in &mut self.compositions {
            removed |= remove_node_id(&mut composition.node_ids, node_id);
            clear_output_node(&mut composition.output_node_id, node_id);
            clear_output_node(&mut composition.audio_output_node_id, node_id);
        }
        for track in self.tracks.values_mut() {
            removed |= remove_node_id(&mut track.node_ids, node_id);
            clear_output_node(&mut track.output_node_id, node_id);
            clear_output_node(&mut track.audio_output_node_id, node_id);
        }
        for clip in self.clips.values_mut() {
            removed |= remove_node_id(&mut clip.node_ids, node_id);
            clear_output_node(&mut clip.output_node_id, node_id);
            clear_output_node(&mut clip.audio_output_node_id, node_id);
        }
        removed
    }

    pub fn validate_containment(&self) -> Vec<ProjectGraphError> {
        let mut errors = Vec::new();
        let mut composition_ids = std::collections::HashSet::new();
        for composition in &self.compositions {
            if !composition_ids.insert(composition.id) {
                errors.push(ProjectGraphError::DuplicateCompositionId(composition.id));
            }
            match composition.frame_count() {
                Ok(frame_count) => {
                    if composition.work_area_in > composition.work_area_out
                        || composition.work_area_out > frame_count
                    {
                        errors.push(ProjectGraphError::InvalidCompositionWorkArea {
                            composition_id: composition.id,
                            work_area_in: composition.work_area_in,
                            work_area_out: composition.work_area_out,
                            frame_count,
                        });
                    }
                }
                Err(reason) => errors.push(ProjectGraphError::InvalidCompositionSettings {
                    composition_id: composition.id,
                    reason,
                }),
            }
        }
        for (key, track) in &self.tracks {
            if *key != track.id {
                errors.push(ProjectGraphError::TrackKeyMismatch {
                    key: *key,
                    entity_id: track.id,
                });
            }
        }
        for (key, clip) in &self.clips {
            if *key != clip.id {
                errors.push(ProjectGraphError::ClipKeyMismatch {
                    key: *key,
                    entity_id: clip.id,
                });
            }
        }
        for (key, node) in &self.nodes {
            if *key != node.id {
                errors.push(ProjectGraphError::NodeKeyMismatch {
                    key: *key,
                    entity_id: node.id,
                });
            }
        }
        for (key, resource) in &self.resources {
            if *key != resource.id {
                errors.push(ProjectGraphError::ResourceKeyMismatch {
                    key: *key,
                    entity_id: resource.id,
                });
            }
        }
        let mut asset_ids = std::collections::HashSet::new();
        for asset in &self.assets {
            if !asset_ids.insert(asset.id) {
                errors.push(ProjectGraphError::DuplicateAssetId(asset.id));
            }
        }
        let mut connection_ids = std::collections::HashSet::new();
        for connection in &self.connections {
            if !connection_ids.insert(connection.id) {
                errors.push(ProjectGraphError::DuplicateConnectionId(connection.id));
            }
        }

        let mut track_owners = HashMap::new();
        for composition in &self.compositions {
            for track_id in &composition.track_ids {
                if !self.tracks.contains_key(track_id) {
                    errors.push(ProjectGraphError::TrackNotFound(*track_id));
                }
                if let Some(composition_id) = track_owners.insert(*track_id, composition.id) {
                    errors.push(ProjectGraphError::TrackAlreadyContained {
                        track_id: *track_id,
                        composition_id,
                    });
                }
            }
        }
        for track_id in self.tracks.keys() {
            if !track_owners.contains_key(track_id) {
                errors.push(ProjectGraphError::TrackHasNoComposition(*track_id));
            }
        }

        let mut clip_owners = HashMap::new();
        for track in self.tracks.values() {
            for clip_id in &track.clip_ids {
                if !self.clips.contains_key(clip_id) {
                    errors.push(ProjectGraphError::ClipNotFound(*clip_id));
                }
                if let Some(track_id) = clip_owners.insert(*clip_id, track.id) {
                    errors.push(ProjectGraphError::ClipAlreadyContained {
                        clip_id: *clip_id,
                        track_id,
                    });
                }
            }
        }
        for clip_id in self.clips.keys() {
            if !clip_owners.contains_key(clip_id) {
                errors.push(ProjectGraphError::ClipHasNoTrack(*clip_id));
            }
        }

        let mut owners = HashMap::new();
        for (container, ids, image_output, audio_output) in self.container_views() {
            for node_id in ids {
                if !self.nodes.contains_key(node_id) {
                    errors.push(ProjectGraphError::NodeNotFound(*node_id));
                }
                if let Some(previous) = owners.insert(*node_id, container) {
                    errors.push(ProjectGraphError::NodeAlreadyContained {
                        node_id: *node_id,
                        container: previous,
                    });
                }
            }
            for output_node_id in [image_output, audio_output].into_iter().flatten() {
                if !ids.contains(&output_node_id) {
                    errors.push(ProjectGraphError::OutputNodeOutsideContainer {
                        node_id: output_node_id,
                        container,
                    });
                }
            }
        }
        for node_id in self.nodes.keys() {
            if !owners.contains_key(node_id) {
                errors.push(ProjectGraphError::NodeHasNoContainer(*node_id));
            }
        }
        errors
    }

    pub fn remove_node(&mut self, node_id: Uuid) -> Option<Node> {
        self.detach_node(node_id);
        self.connections.retain(|connection| {
            connection.from.owner != PortOwner::Node(node_id)
                && connection.to.owner != PortOwner::Node(node_id)
        });
        self.nodes.remove(&node_id)
    }

    pub fn remove_clip(&mut self, clip_id: Uuid) -> Option<Clip> {
        let clip = self.clips.remove(&clip_id)?;
        self.detach_clip(clip_id);
        for node_id in clip.node_ids.clone() {
            self.remove_node(node_id);
        }
        self.connections.retain(|connection| {
            connection.from.owner != PortOwner::Clip(clip_id)
                && connection.to.owner != PortOwner::Clip(clip_id)
        });
        Some(clip)
    }

    pub fn remove_track(&mut self, track_id: Uuid) -> Option<Track> {
        let track = self.tracks.remove(&track_id)?;
        self.detach_track(track_id);
        for clip_id in track.clip_ids.clone() {
            self.remove_clip(clip_id);
        }
        for node_id in track.node_ids.clone() {
            self.remove_node(node_id);
        }
        self.connections.retain(|connection| {
            connection.from.owner != PortOwner::Track(track_id)
                && connection.to.owner != PortOwner::Track(track_id)
        });
        Some(track)
    }

    pub fn remove_composition(&mut self, composition_id: Uuid) -> Option<Composition> {
        let index = self
            .compositions
            .iter()
            .position(|item| item.id == composition_id)?;
        let composition = self.compositions.remove(index);
        for track_id in composition.track_ids.clone() {
            self.remove_track(track_id);
        }
        for node_id in composition.node_ids.clone() {
            self.remove_node(node_id);
        }
        let references = self
            .nodes
            .values()
            .filter(|node| matches!(node.content(), NodeContent::Reference(reference) if reference.target_id == composition_id))
            .map(|node| node.id)
            .collect::<Vec<_>>();
        for node_id in references {
            self.remove_node(node_id);
        }
        self.connections.retain(|connection| {
            connection.from.owner != PortOwner::Composition(composition_id)
                && connection.to.owner != PortOwner::Composition(composition_id)
        });
        Some(composition)
    }

    fn internal_metadata_reparent_plan(
        &self,
        old_parent: PortOwner,
        new_parent: PortOwner,
        moved_child: PortOwner,
    ) -> Result<Vec<ConnectionSourceRemap>, ProjectGraphError> {
        let mut remaps = Vec::new();
        for (connection_index, connection) in self.connections.iter().enumerate() {
            if connection.from.owner != old_parent || connection.to.owner != moved_child {
                continue;
            }
            let source = self
                .port_definition(&connection.from, PortDirection::Output)
                .ok_or_else(|| ProjectGraphError::PortNotFound(connection.from.clone()))?;
            if source.exposure != PortExposure::Internal {
                continue;
            }

            let replacement = PortAddress::new(new_parent, connection.from.port.clone());
            let replacement_definition = self
                .port_definition(&replacement, PortDirection::Output)
                .filter(|definition| definition.exposure == PortExposure::Internal)
                .ok_or_else(|| ProjectGraphError::PortNotFound(replacement.clone()))?;
            if replacement_definition.data_type != source.data_type {
                return Err(ProjectGraphError::IncompatiblePortTypes {
                    source_type: source.data_type,
                    target_type: replacement_definition.data_type,
                });
            }
            let target = self
                .port_definition(&connection.to, PortDirection::Input)
                .ok_or_else(|| ProjectGraphError::PortNotFound(connection.to.clone()))?;
            if !target.data_type.accepts(replacement_definition.data_type) {
                return Err(ProjectGraphError::IncompatiblePortTypes {
                    source_type: replacement_definition.data_type,
                    target_type: target.data_type,
                });
            }

            remaps.push(ConnectionSourceRemap {
                connection_index,
                original: connection.from.clone(),
                replacement,
            });
        }
        Ok(remaps)
    }

    fn apply_connection_source_remaps(&mut self, remaps: &[ConnectionSourceRemap]) {
        for remap in remaps {
            if let Some(connection) = self.connections.get_mut(remap.connection_index) {
                connection.from = remap.replacement.clone();
            }
        }
    }

    fn rollback_connection_source_remaps(&mut self, remaps: &[ConnectionSourceRemap]) {
        for remap in remaps {
            if let Some(connection) = self.connections.get_mut(remap.connection_index) {
                connection.from = remap.original.clone();
            }
        }
    }

    fn restore_container_node_state(&mut self, state: ContainerNodeState) {
        match state.container {
            NodeContainer::Composition(id) => {
                if let Some(composition) = self.get_composition_mut(id) {
                    composition.node_ids = state.node_ids;
                    composition.output_node_id = state.output_node_id;
                    composition.audio_output_node_id = state.audio_output_node_id;
                }
            }
            NodeContainer::Track(id) => {
                if let Some(track) = self.get_track_mut(id) {
                    track.node_ids = state.node_ids;
                    track.output_node_id = state.output_node_id;
                    track.audio_output_node_id = state.audio_output_node_id;
                }
            }
            NodeContainer::Clip(id) => {
                if let Some(clip) = self.get_clip_mut(id) {
                    clip.node_ids = state.node_ids;
                    clip.output_node_id = state.output_node_id;
                    clip.audio_output_node_id = state.audio_output_node_id;
                }
            }
        }
    }

    fn container_node_ids_mut(&mut self, container: NodeContainer) -> Option<&mut Vec<Uuid>> {
        match container {
            NodeContainer::Composition(id) => {
                self.get_composition_mut(id).map(|item| &mut item.node_ids)
            }
            NodeContainer::Track(id) => self.get_track_mut(id).map(|item| &mut item.node_ids),
            NodeContainer::Clip(id) => self.get_clip_mut(id).map(|item| &mut item.node_ids),
        }
    }

    fn container_views(&self) -> Vec<(NodeContainer, &Vec<Uuid>, Option<Uuid>, Option<Uuid>)> {
        self.compositions
            .iter()
            .map(|item| {
                (
                    NodeContainer::Composition(item.id),
                    &item.node_ids,
                    item.output_node_id,
                    item.audio_output_node_id,
                )
            })
            .chain(self.tracks.values().map(|item| {
                (
                    NodeContainer::Track(item.id),
                    &item.node_ids,
                    item.output_node_id,
                    item.audio_output_node_id,
                )
            }))
            .chain(self.clips.values().map(|item| {
                (
                    NodeContainer::Clip(item.id),
                    &item.node_ids,
                    item.output_node_id,
                    item.audio_output_node_id,
                )
            }))
            .collect()
    }
}

fn port_owner_for_container(container: NodeContainer) -> PortOwner {
    match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}

fn first_new_project_validation_error(
    baseline: &[ProjectGraphError],
    current: Vec<ProjectGraphError>,
) -> Option<ProjectGraphError> {
    let mut unmatched_baseline = baseline.to_vec();
    current.into_iter().find(|error| {
        let Some(index) = unmatched_baseline
            .iter()
            .position(|baseline_error| baseline_error == error)
        else {
            return true;
        };
        unmatched_baseline.remove(index);
        false
    })
}

fn remove_node_id(ids: &mut Vec<Uuid>, node_id: Uuid) -> bool {
    let old_len = ids.len();
    ids.retain(|id| *id != node_id);
    old_len != ids.len()
}

fn clear_output_node(output: &mut Option<Uuid>, node_id: Uuid) {
    if *output == Some(node_id) {
        *output = None;
    }
}
