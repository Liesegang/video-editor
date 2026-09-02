use std::collections::HashMap;

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::model::frame::color::Color;
use crate::model::project::asset::Asset;
use crate::model::project::property::PropertyMap;
use crate::model::project::{
    ColorManagementConfig, ColorManagementIssue, ExportConfig, RequestedColorManagementConfig,
    ResolvedColorManagementConfig,
};

use super::{
    AttachmentId, DataSource, DataSourceId, EventBinding, EventBindingId, GeneratedItem,
    GeneratedItemId, Mask, MaskId, ModuleDefinition, ModuleDefinitionId, ModuleInstance,
    ModuleInstanceId, Override, OverrideId, SignalBinding, SignalBindingId, Timeline, TimelineId,
    TimelineItem, TimelineItemId, TimelineTrack, TimelineTrackId, TimelineTrackKind, Transition,
    TransitionId,
};

pub const PROJECT_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ProjectDocument {
    pub format_version: u32,
    pub project: AuthoringProject,
}

impl ProjectDocument {
    pub fn new(project: AuthoringProject) -> Self {
        Self {
            format_version: PROJECT_FORMAT_VERSION,
            project,
        }
    }

    pub fn from_json(source: &str) -> Result<Self, String> {
        let document: Self = serde_json::from_str(source)
            .map_err(|error| format!("Unsupported Project format: {error}"))?;
        if document.format_version != PROJECT_FORMAT_VERSION {
            return Err(format!(
                "Unsupported Project format version {}; expected {}",
                document.format_version, PROJECT_FORMAT_VERSION
            ));
        }
        document.project.validate()?;
        Ok(document)
    }

    pub fn to_json(&self) -> Result<String, String> {
        if self.format_version != PROJECT_FORMAT_VERSION {
            return Err(format!(
                "Cannot save Project format version {}; expected {}",
                self.format_version, PROJECT_FORMAT_VERSION
            ));
        }
        self.project.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|error| format!("Cannot serialize Project: {error}"))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct AuthoringProject {
    pub name: String,
    pub root_timeline_id: TimelineId,
    pub timelines: HashMap<TimelineId, Timeline>,
    pub tracks: HashMap<TimelineTrackId, TimelineTrack>,
    pub items: HashMap<TimelineItemId, TimelineItem>,
    pub module_definitions: HashMap<ModuleDefinitionId, ModuleDefinition>,
    pub module_instances: HashMap<ModuleInstanceId, ModuleInstance>,
    pub attachments: HashMap<AttachmentId, Attachment>,
    pub signal_bindings: HashMap<SignalBindingId, SignalBinding>,
    pub event_bindings: HashMap<EventBindingId, EventBinding>,
    pub data_sources: HashMap<DataSourceId, DataSource>,
    pub generated_items: HashMap<GeneratedItemId, GeneratedItem>,
    pub overrides: HashMap<OverrideId, Override>,
    pub masks: HashMap<MaskId, Mask>,
    pub transitions: HashMap<TransitionId, Transition>,
    pub assets: Vec<Asset>,
    pub(crate) color_management: RequestedColorManagementConfig,
    pub export: ExportConfig,
}

impl AuthoringProject {
    pub fn new(
        name: impl Into<String>,
        width: u64,
        height: u64,
        fps: f64,
        duration: f64,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("Timeline dimensions must be greater than zero".to_string());
        }
        if !fps.is_finite() || fps <= 0.0 {
            return Err("Timeline FPS must be finite and greater than zero".to_string());
        }
        if !duration.is_finite() || duration < 0.0 {
            return Err("Timeline duration must be finite and non-negative".to_string());
        }
        let timeline_id = TimelineId::new();
        let track_id = TimelineTrackId::new();
        Ok(Self {
            name: name.into(),
            root_timeline_id: timeline_id,
            timelines: HashMap::from([(
                timeline_id,
                Timeline {
                    id: timeline_id,
                    name: "Main".to_string(),
                    width,
                    height,
                    fps: OrderedFloat(fps),
                    duration: OrderedFloat(duration),
                    background_color: Color::black(),
                    color_profile: "sRGB".to_string(),
                    track_order: vec![track_id],
                    authored_properties: PropertyMap::new(),
                },
            )]),
            tracks: HashMap::from([(
                track_id,
                TimelineTrack {
                    id: track_id,
                    timeline_id,
                    name: "Video 1".to_string(),
                    kind: TimelineTrackKind::AudioVisual,
                    authored_properties: PropertyMap::new(),
                },
            )]),
            items: HashMap::new(),
            module_definitions: HashMap::new(),
            module_instances: HashMap::new(),
            attachments: HashMap::new(),
            signal_bindings: HashMap::new(),
            event_bindings: HashMap::new(),
            data_sources: HashMap::new(),
            generated_items: HashMap::new(),
            overrides: HashMap::new(),
            masks: HashMap::new(),
            transitions: HashMap::new(),
            assets: Vec::new(),
            color_management: RequestedColorManagementConfig::default(),
            export: ExportConfig::default(),
        })
    }

    pub fn requested_color_management(&self) -> &RequestedColorManagementConfig {
        &self.color_management
    }

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

    pub fn resolved_color_management(&self) -> ResolvedColorManagementConfig {
        crate::model::project::color_management::resolve_color_management(
            &self.color_management,
            &self.assets,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.timelines.contains_key(&self.root_timeline_id) {
            return Err("Project root Timeline does not exist".to_string());
        }
        for timeline in self.timelines.values() {
            if timeline.width == 0 || timeline.height == 0 {
                return Err(format!(
                    "Timeline {} dimensions must be greater than zero",
                    timeline.id
                ));
            }
            if !timeline.fps.is_finite() || timeline.fps.into_inner() <= 0.0 {
                return Err(format!("Timeline {} has invalid FPS", timeline.id));
            }
            if !timeline.duration.is_finite() || timeline.duration.into_inner() < 0.0 {
                return Err(format!("Timeline {} has invalid duration", timeline.id));
            }
            if timeline.color_profile.trim().is_empty() {
                return Err(format!("Timeline {} has no color profile", timeline.id));
            }
            let mut seen_tracks = std::collections::HashSet::new();
            for track_id in &timeline.track_order {
                if !seen_tracks.insert(*track_id) {
                    return Err(format!("Timeline {} lists a Track twice", timeline.id));
                }
                let Some(track) = self.tracks.get(track_id) else {
                    return Err(format!("Timeline {} lists a missing Track", timeline.id));
                };
                if track.timeline_id != timeline.id {
                    return Err(format!(
                        "Timeline {} lists a Track owned elsewhere",
                        timeline.id
                    ));
                }
            }
        }
        for track in self.tracks.values() {
            if !self.timelines.contains_key(&track.timeline_id) {
                return Err(format!("Track {} refers to a missing Timeline", track.id));
            }
        }
        for item in self.items.values() {
            let Some(track) = self.tracks.get(&item.track_id) else {
                return Err(format!("Item {} refers to a missing Track", item.id));
            };
            if !item.interval.start.is_finite()
                || item.interval.start.into_inner() < 0.0
                || !item.interval.duration.is_finite()
                || item.interval.duration.into_inner() < 0.0
            {
                return Err(format!("Item {} has an invalid interval", item.id));
            }
            if let Some(parent) = item.parent {
                let Some(parent_item) = self.items.get(&parent) else {
                    return Err(format!("Item {} refers to a missing parent", item.id));
                };
                let Some(parent_track) = self.tracks.get(&parent_item.track_id) else {
                    return Err(format!("Item {} has a parent on a missing Track", item.id));
                };
                if parent_track.timeline_id != track.timeline_id {
                    return Err(format!("Item {} has a parent in another Timeline", item.id));
                }
            }
            if let Some(matte) = item.matte {
                let matte_item = self
                    .items
                    .get(&matte.item_id)
                    .ok_or_else(|| format!("Item {} has a missing Matte item", item.id))?;
                let matte_track = self
                    .tracks
                    .get(&matte_item.track_id)
                    .ok_or_else(|| format!("Item {} has a Matte on a missing Track", item.id))?;
                if matte_item.id == item.id || matte_track.timeline_id != track.timeline_id {
                    return Err(format!("Item {} has an invalid Matte item", item.id));
                }
            }
            for constraint in &item.constraints {
                let target = self.items.get(&constraint.target_item_id).ok_or_else(|| {
                    format!("Item {} has a Constraint with a missing target", item.id)
                })?;
                let target_track = self.tracks.get(&target.track_id).ok_or_else(|| {
                    format!(
                        "Item {} has a Constraint target on a missing Track",
                        item.id
                    )
                })?;
                if target.id == item.id || target_track.timeline_id != track.timeline_id {
                    return Err(format!("Item {} has an invalid Constraint target", item.id));
                }
            }
            match &item.source {
                crate::model::authoring::SourceRef::Asset { asset_id, time_map } => {
                    if !self.assets.iter().any(|asset| asset.id == *asset_id) {
                        return Err(format!("Item {} refers to a missing Asset", item.id));
                    }
                    validate_time_map(item.id, time_map)?;
                }
                crate::model::authoring::SourceRef::Composition(instance) => {
                    let nested = self.timelines.get(&instance.timeline_id).ok_or_else(|| {
                        format!("Item {} refers to a missing nested Timeline", item.id)
                    })?;
                    validate_time_map(item.id, &instance.time_map)?;
                    if let crate::model::authoring::DurationPolicy::Responsive {
                        intro_end,
                        outro_start,
                    } = &instance.duration_policy
                    {
                        let definition_duration = nested.duration.into_inner();
                        let minimum =
                            intro_end.into_inner() + definition_duration - outro_start.into_inner();
                        if intro_end.into_inner() < 0.0
                            || intro_end > outro_start
                            || outro_start.into_inner() > definition_duration
                            || item.interval.duration.into_inner() < minimum
                        {
                            return Err(format!(
                                "Item {} has invalid Responsive duration markers",
                                item.id
                            ));
                        }
                    }
                }
                crate::model::authoring::SourceRef::Module { module_instance_id } => {
                    if !self.module_instances.contains_key(module_instance_id) {
                        return Err(format!(
                            "Item {} refers to a missing Module instance",
                            item.id
                        ));
                    }
                }
                crate::model::authoring::SourceRef::Text { .. }
                | crate::model::authoring::SourceRef::Shape { .. }
                | crate::model::authoring::SourceRef::Solid { .. } => {}
            }
        }
        self.validate_parent_cycles()?;
        let mut referenced_masks = std::collections::HashSet::new();
        for item in self.items.values() {
            for mask_id in &item.mask_ids {
                if !self.masks.contains_key(mask_id) {
                    return Err(format!("Item {} lists a missing Mask", item.id));
                }
                if !referenced_masks.insert(*mask_id) {
                    return Err(format!("Mask {mask_id} is owned by multiple items"));
                }
            }
        }
        if referenced_masks.len() != self.masks.len() {
            return Err("Project contains a Mask without a Timeline item owner".to_string());
        }
        for transition in self.transitions.values() {
            if !transition.duration.is_finite() || transition.duration.into_inner() < 0.0 {
                return Err(format!(
                    "Transition {} has an invalid duration",
                    transition.id
                ));
            }
            let from = self
                .items
                .get(&transition.from_item_id)
                .ok_or_else(|| format!("Transition {} has a missing source item", transition.id))?;
            let to = self
                .items
                .get(&transition.to_item_id)
                .ok_or_else(|| format!("Transition {} has a missing target item", transition.id))?;
            if from.id == to.id || from.track_id != to.track_id {
                return Err(format!(
                    "Transition {} must join two items on one Track",
                    transition.id
                ));
            }
        }
        for definition in self.module_definitions.values() {
            definition.validate()?;
        }
        for instance in self.module_instances.values() {
            if !self
                .module_definitions
                .contains_key(&instance.definition_id)
            {
                return Err(format!(
                    "Module instance {} refers to a missing definition",
                    instance.id
                ));
            }
        }
        for attachment in self.attachments.values() {
            if !self
                .module_instances
                .contains_key(&attachment.module_instance_id)
            {
                return Err(format!(
                    "Attachment {} refers to a missing Module instance",
                    attachment.id
                ));
            }
            let owner_exists = match &attachment.owner {
                AttachmentOwner::Timeline { timeline_id } => {
                    self.timelines.contains_key(timeline_id)
                }
                AttachmentOwner::Track { track_id } => self.tracks.contains_key(track_id),
                AttachmentOwner::Item { item_id } => self.items.contains_key(item_id),
            };
            if !owner_exists {
                return Err(format!("Attachment {} has a missing owner", attachment.id));
            }
        }
        Ok(())
    }

    fn validate_parent_cycles(&self) -> Result<(), String> {
        for item in self.items.values() {
            let mut active = std::collections::HashSet::new();
            let mut current = Some(item.id);
            while let Some(item_id) = current {
                if !active.insert(item_id) {
                    return Err(format!("Parent cycle reaches Timeline item {item_id}"));
                }
                current = self.items.get(&item_id).and_then(|item| item.parent);
            }
        }
        Ok(())
    }
}

fn validate_time_map(
    item_id: TimelineItemId,
    time_map: &crate::model::authoring::TimeMap,
) -> Result<(), String> {
    if !time_map.source_start.is_finite()
        || time_map.source_start.into_inner() < 0.0
        || !time_map.playback_rate.is_finite()
    {
        return Err(format!("Item {item_id} has an invalid time map"));
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub id: AttachmentId,
    pub owner: AttachmentOwner,
    pub module_instance_id: ModuleInstanceId,
    pub stage: AttachmentStage,
    pub order: i64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachmentOwner {
    Timeline { timeline_id: TimelineId },
    Track { track_id: TimelineTrackId },
    Item { item_id: TimelineItemId },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[serde(rename_all = "snake_case")]
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
