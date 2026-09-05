use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use super::{BlendMode, Clip, Node, Track};
use crate::model::frame::color::Color;
use crate::model::project::asset::Asset;
use crate::model::project::property::PropertyMap;
pub mod asset;
pub mod clip_helpers;
pub(crate) mod color_management;
pub mod connection;
mod error;
mod node_graph_bundle;
mod output_binding;
mod path_effect_stack;
pub mod property;
mod removal;
mod structural_merge;
mod transaction;
mod validation;
use transaction::{first_new_project_validation_error, port_owner_for_container};

pub use color_management::{
    ColorConfigIdentity, ColorManagementConfig, ColorManagementField, ColorManagementIssue,
    ColorManagementStructureIssue, DEFAULT_BUNDLED_COLOR_CONFIG_ID, DEFAULT_OUTPUT_COLOR_SPACE,
    DEFAULT_PREVIEW_DISPLAY, DEFAULT_PREVIEW_SURFACE_ENCODING, DEFAULT_PREVIEW_VIEW,
    DEFAULT_WORKING_COLOR_SPACE, ExportColorConfig, HdrColorField, HdrColorSettings,
    HdrColorSettingsError, LEGACY_BUNDLED_COLOR_CONFIG_V1_ID, ModelValidatedColorManagementConfig,
    PqLinearizationPolicy, PreviewColorConfig, PreviewSurfaceEncoding,
    RequestedColorManagementConfig, ResolvedColorManagementConfig, SrgbSurfaceColorSpaceBinding,
};
pub use connection::{
    ANALYSIS_HOP_MS_PROPERTY, ANALYSIS_SAMPLE_RATE_PROPERTY, ANALYSIS_WINDOW_MS_PROPERTY,
    APPEARANCE_STYLES_PORT, AUDIO_OUTPUT_PORT, BACKGROUND_SHAPE_INPUT_PORT, BAND_HIGH_HZ_PROPERTY,
    BAND_LOW_HZ_PROPERTY, ContainerAudioSource, ContainerAudioSourceKind, ContainerGraphSemantics,
    ContainerImageSource, ContainerImageSourceKind, DURATION_PORT, EvalOutput, EvalResult,
    EvaluationError, FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, FPS_PORT, FRAME_PORT,
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT,
    NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT, PortAddress,
    PortDataType, PortDefinition, PortDirection, PortExposure, PortMultiplicity, PortOwner,
    PortSide, ProjectConnection, RESOLUTION_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
    SOUND_INPUT_PORT, SPECTRUM_INPUT_PORT, SPECTRUM_OUTPUT_PORT, STYLE_OUTPUT_PORT, TIME_PORT,
    TRANSITION_FROM_INPUT_PORT, TRANSITION_PROGRESS_INPUT_PORT, TRANSITION_PROGRESS_PROPERTY,
    TRANSITION_TO_INPUT_PORT,
};
pub use error::ProjectGraphError;
pub use node_graph_bundle::NodeGraphBundle;

/// Compact model-owned defaults used before the Node Editor has ever opened.
/// UI auto-layout may refine these positions, but startup state must already
/// be contained, non-overlapping, and left-to-right routable.
pub(crate) const DEFAULT_GRAPH_CONTENT_INSET: f32 = 56.0;
pub(crate) const DEFAULT_GRAPH_ITEM_GAP: f32 = 24.0;

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
    color_management: RequestedColorManagementConfig,
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
    /// Stable identity of the ordinary Merge Node that receives direct Track
    /// image outputs. This required annotation is independent from the
    /// editable downstream output binding.
    pub structural_merge_node_id: Uuid,
    /// Stable native Sound Merge receiving direct Track Sound outputs.
    pub structural_sound_merge_node_id: Uuid,
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
        let mut first_track = Track::new("Track 1");
        first_track.ui_position = [DEFAULT_GRAPH_CONTENT_INSET, DEFAULT_GRAPH_CONTENT_INSET];
        let structural_merge_node_id = Uuid::new_v4();
        let structural_sound_merge_node_id = Uuid::new_v4();
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
                node_ids: vec![structural_merge_node_id, structural_sound_merge_node_id],
                structural_merge_node_id,
                structural_sound_merge_node_id,
                output_node_id: Some(structural_merge_node_id),
                audio_output_node_id: Some(structural_sound_merge_node_id),
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

#[derive(Clone, Copy)]
struct ContainerView<'a> {
    container: NodeContainer,
    node_ids: &'a [Uuid],
    image_output_node_id: Option<Uuid>,
    audio_output_node_id: Option<Uuid>,
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
            color_management: RequestedColorManagementConfig::default(),
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

    /// Persisted color intent exactly as requested by the Project.
    ///
    /// Runtime consumers should use [`Project::resolved_color_management`] so
    /// an unavailable external config cannot silently reinterpret space names.
    pub fn requested_color_management(&self) -> &RequestedColorManagementConfig {
        &self.color_management
    }

    /// Parsed color intent, or `None` when the raw persisted value is
    /// structurally malformed and retained for repair.
    pub fn requested_color_management_config(&self) -> Option<&ColorManagementConfig> {
        self.color_management.as_config()
    }

    /// Replace the Project color intent only when its identifiers are pinned
    /// and its external config asset has a matching import-time identity.
    /// Asset source-space assignments owned by another config remain persisted
    /// and diagnosed per Asset; they do not block changing the Project config.
    /// Backend and current filesystem availability remain runtime checks.
    pub fn set_color_management(
        &mut self,
        color_management: ColorManagementConfig,
    ) -> Result<(), Vec<ColorManagementIssue>> {
        let diagnostics = color_management.blocking_diagnostics(&self.assets);
        if diagnostics.is_empty() {
            self.color_management = RequestedColorManagementConfig::from_config(color_management);
            Ok(())
        } else {
            Err(diagnostics)
        }
    }

    /// Non-fatal diagnostics for persisted Project intent and Asset-local
    /// source-space assignments.
    ///
    /// These are not included in [`Project::validation_issues`], because a
    /// missing external config must not prevent a user from opening and
    /// repairing the Project.
    pub fn color_management_diagnostics(&self) -> Vec<ColorManagementIssue> {
        self.color_management.diagnostics(&self.assets)
    }

    /// Model-validate color settings without substituting a different config.
    /// Callers must handle [`ResolvedColorManagementConfig::Unavailable`]
    /// explicitly. A Ready result still requires backend/resource verification;
    /// no linear-sRGB fallback is returned from this API.
    pub fn resolved_color_management(&self) -> ResolvedColorManagementConfig {
        color_management::resolve_color_management(&self.color_management, &self.assets)
    }

    pub fn add_composition(&mut self, composition: Composition) -> Result<(), ProjectGraphError> {
        self.insert_composition_with_structural_merge(composition)
    }

    pub fn add_track(&mut self, track: Track) -> Result<(), ProjectGraphError> {
        self.insert_track_with_structural_merge(track)
    }

    pub fn add_clip(&mut self, clip: Clip) {
        self.insert_clip_with_structural_edges(clip);
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id, node);
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
        let mut candidate = self.clone();
        for composition in &mut candidate.compositions {
            composition.track_ids.retain(|id| *id != track_id);
        }
        let composition = candidate
            .get_composition_mut(composition_id)
            .ok_or(ProjectGraphError::CompositionNotFound(composition_id))?;
        let index = index
            .unwrap_or(composition.track_ids.len())
            .min(composition.track_ids.len());
        composition.track_ids.insert(index, track_id);
        candidate.apply_connection_source_remaps(&remaps);
        candidate.transition_structural_child(
            old_parent.map(NodeContainer::Composition),
            NodeContainer::Composition(composition_id),
            PortOwner::Track(track_id),
        );

        if let Some(error) = first_new_project_validation_error(
            &validation_baseline,
            candidate.validate_connections(),
        ) {
            return Err(error);
        }
        *self = candidate;
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
        let validation_baseline = self.validate_connections();
        let mut candidate = self.clone();
        let composition = candidate
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
        candidate.reorder_structural_children(NodeContainer::Composition(composition_id), None);
        if let Some(error) = first_new_project_validation_error(
            &validation_baseline,
            candidate.validate_connections(),
        ) {
            return Err(error);
        }
        *self = candidate;
        Ok(true)
    }

    pub fn detach_track(&mut self, track_id: Uuid) -> bool {
        let parents = self
            .compositions
            .iter()
            .filter(|composition| composition.track_ids.contains(&track_id))
            .map(|composition| NodeContainer::Composition(composition.id))
            .collect::<Vec<_>>();
        let mut removed = false;
        for composition in &mut self.compositions {
            let old_len = composition.track_ids.len();
            composition.track_ids.retain(|id| *id != track_id);
            removed |= composition.track_ids.len() != old_len;
        }
        self.disconnect_structural_child(&parents, PortOwner::Track(track_id));
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
        let mut candidate = self.clone();
        for track in candidate.tracks.values_mut() {
            track.clip_ids.retain(|id| *id != clip_id);
        }
        let track = candidate
            .tracks
            .get_mut(&track_id)
            .ok_or(ProjectGraphError::TrackNotFound(track_id))?;
        let index = index
            .unwrap_or(track.clip_ids.len())
            .min(track.clip_ids.len());
        track.clip_ids.insert(index, clip_id);
        candidate.apply_connection_source_remaps(&remaps);
        candidate.transition_structural_child(
            old_parent.map(NodeContainer::Track),
            NodeContainer::Track(track_id),
            PortOwner::Clip(clip_id),
        );

        if let Some(error) = first_new_project_validation_error(
            &validation_baseline,
            candidate.validate_connections(),
        ) {
            return Err(error);
        }
        *self = candidate;
        Ok(())
    }

    pub fn detach_clip(&mut self, clip_id: Uuid) -> bool {
        let parents = self
            .tracks
            .values()
            .filter(|track| track.clip_ids.contains(&clip_id))
            .map(|track| NodeContainer::Track(track.id))
            .collect::<Vec<_>>();
        let mut removed = false;
        for track in self.tracks.values_mut() {
            let old_len = track.clip_ids.len();
            track.clip_ids.retain(|id| *id != clip_id);
            removed |= track.clip_ids.len() != old_len;
        }
        self.disconnect_structural_child(&parents, PortOwner::Clip(clip_id));
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
            .filter(|view| {
                view.container == container
                    || view.node_ids.contains(&node_id)
                    || view.image_output_node_id == Some(node_id)
                    || view.audio_output_node_id == Some(node_id)
            })
            .map(|view| ContainerNodeState {
                container: view.container,
                node_ids: view.node_ids.to_vec(),
                output_node_id: view.image_output_node_id,
                audio_output_node_id: view.audio_output_node_id,
            })
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

    fn container_views(&self) -> Vec<ContainerView<'_>> {
        self.compositions
            .iter()
            .map(|item| ContainerView {
                container: NodeContainer::Composition(item.id),
                node_ids: &item.node_ids,
                image_output_node_id: item.output_node_id,
                audio_output_node_id: item.audio_output_node_id,
            })
            .chain(self.tracks.values().map(|item| ContainerView {
                container: NodeContainer::Track(item.id),
                node_ids: &item.node_ids,
                image_output_node_id: item.output_node_id,
                audio_output_node_id: item.audio_output_node_id,
            }))
            .chain(self.clips.values().map(|item| ContainerView {
                container: NodeContainer::Clip(item.id),
                node_ids: &item.node_ids,
                image_output_node_id: item.output_node_id,
                audio_output_node_id: item.audio_output_node_id,
            }))
            .collect()
    }
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
