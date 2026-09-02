use std::collections::HashMap;

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::model::frame::color::Color;
use crate::model::project::asset::Asset;
use crate::model::project::property::PropertyMap;

use super::{
    AttachmentId, DataSource, DataSourceId, EventBinding, EventBindingId, GeneratedItem,
    GeneratedItemId, ModuleDefinition, ModuleDefinitionId, ModuleInstance, ModuleInstanceId,
    Override, OverrideId, SignalBinding, SignalBindingId, Timeline, TimelineId, TimelineItem,
    TimelineItemId, TimelineTrack, TimelineTrackId, TimelineTrackKind,
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
    pub assets: Vec<Asset>,
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
            assets: Vec::new(),
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.timelines.contains_key(&self.root_timeline_id) {
            return Err("Project root Timeline does not exist".to_string());
        }
        for timeline in self.timelines.values() {
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
