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
    ModuleInstanceId, Override, OverrideId, PublishedParameterId, SignalBinding, SignalBindingId,
    Timeline, TimelineId, TimelineItem, TimelineItemId, TimelineTrack, TimelineTrackId,
    TimelineTrackKind, Transition, TransitionId,
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
            if let Some(generated_id) = item.generated_item_id {
                if item.id.as_uuid() != generated_id.as_uuid()
                    || !self.generated_items.contains_key(&generated_id)
                {
                    return Err(format!(
                        "Item {} has invalid GeneratedItem provenance",
                        item.id
                    ));
                }
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
        for data_source in self.data_sources.values() {
            if !self.tracks.contains_key(&data_source.target_track_id) {
                return Err(format!(
                    "Data source {} has no target Track",
                    data_source.id
                ));
            }
            let instance = self
                .module_instances
                .get(&data_source.generator_id)
                .ok_or_else(|| {
                    format!("Data source {} has no Generator instance", data_source.id)
                })?;
            let definition = self
                .module_definitions
                .get(&instance.definition_id)
                .ok_or_else(|| {
                    format!("Data source {} has no Generator definition", data_source.id)
                })?;
            if !matches!(
                definition.role,
                crate::model::authoring::ModuleRole::Generator
            ) {
                return Err(format!(
                    "Data source {} refers to a non-Generator Module",
                    data_source.id
                ));
            }
            if data_source.stable_key_field.trim().is_empty() {
                return Err(format!(
                    "Data source {} has no stable key field",
                    data_source.id
                ));
            }
            let mut row_keys = std::collections::HashSet::new();
            for row in &data_source.cached_rows {
                if row.stable_key.trim().is_empty() || !row_keys.insert(&row.stable_key) {
                    return Err(format!(
                        "Data source {} has an empty or duplicate stable row key",
                        data_source.id
                    ));
                }
            }
        }
        for generated in self.generated_items.values() {
            if !self.module_instances.contains_key(&generated.generator_id) {
                return Err(format!(
                    "Generated item {} has no Generator instance",
                    generated.stable_id
                ));
            }
            if generated.stable_id
                != GeneratedItem::stable_id(generated.generator_id, &generated.source_key)
            {
                return Err(format!(
                    "Generated item {} has an unstable provenance ID",
                    generated.stable_id
                ));
            }
            if let Some(data_source_id) = generated.provenance.data_source_id {
                let data_source = self.data_sources.get(&data_source_id).ok_or_else(|| {
                    format!(
                        "Generated item {} has a missing Data source",
                        generated.stable_id
                    )
                })?;
                if data_source.generator_id != generated.generator_id {
                    return Err(format!(
                        "Generated item {} belongs to a different Generator",
                        generated.stable_id
                    ));
                }
            }
            let item_id = TimelineItemId::from_uuid(generated.stable_id.as_uuid());
            if self
                .items
                .get(&item_id)
                .and_then(|item| item.generated_item_id)
                != Some(generated.stable_id)
            {
                return Err(format!(
                    "Generated item {} is not materialized on a Timeline",
                    generated.stable_id
                ));
            }
        }
        for authored_override in self.overrides.values() {
            if !self
                .generated_items
                .contains_key(&authored_override.generated_item_id)
                && !matches!(
                    authored_override.status,
                    crate::model::authoring::OverrideStatus::Orphaned
                )
            {
                return Err(format!(
                    "Override {} has no GeneratedItem and is not Orphaned",
                    authored_override.id
                ));
            }
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
        for binding in self.signal_bindings.values() {
            self.validate_signal_source(&binding.source)?;
            self.validate_binding_target(&binding.scope, binding.target_parameter_id, None)?;
            if !binding.mapping.input_min.is_finite()
                || !binding.mapping.input_max.is_finite()
                || !binding.mapping.output_min.is_finite()
                || !binding.mapping.output_max.is_finite()
                || binding.mapping.input_min == binding.mapping.input_max
                || !binding.smoothing_seconds.is_finite()
                || binding.smoothing_seconds.into_inner() < 0.0
            {
                return Err(format!(
                    "Signal Binding {} has an invalid mapping",
                    binding.id
                ));
            }
        }
        for binding in self.event_bindings.values() {
            self.validate_event_source(&binding.source)?;
            self.validate_binding_target(
                &binding.scope,
                PublishedParameterId::from_uuid(uuid::Uuid::nil()),
                Some(binding.target_action_id),
            )?;
        }
        Ok(())
    }

    fn validate_instance_path(
        &self,
        path: &crate::model::authoring::InstancePath,
    ) -> Result<TimelineId, String> {
        let mut timeline_id = path.root_timeline_id;
        if !self.timelines.contains_key(&timeline_id) {
            return Err("Binding InstancePath has a missing root Timeline".to_string());
        }
        for item_id in &path.composition_items {
            let item = self
                .items
                .get(item_id)
                .ok_or_else(|| "Binding InstancePath has a missing item".to_string())?;
            let track = self
                .tracks
                .get(&item.track_id)
                .ok_or_else(|| "Binding InstancePath item has a missing Track".to_string())?;
            if track.timeline_id != timeline_id {
                return Err(
                    "Binding InstancePath does not follow the Timeline hierarchy".to_string(),
                );
            }
            let crate::model::authoring::SourceRef::Composition(instance) = &item.source else {
                return Err("Binding InstancePath contains a non-Composition item".to_string());
            };
            timeline_id = instance.timeline_id;
        }
        Ok(timeline_id)
    }

    fn validate_binding_target(
        &self,
        scope: &crate::model::authoring::BindingScope,
        parameter_id: PublishedParameterId,
        action_id: Option<crate::model::authoring::PublishedActionId>,
    ) -> Result<(), String> {
        let definition = match scope {
            crate::model::authoring::BindingScope::Definition { definition_id } => self
                .module_definitions
                .get(definition_id)
                .ok_or_else(|| "Binding scope has a missing Module definition".to_string())?,
            crate::model::authoring::BindingScope::Instance {
                instance_path,
                module_instance_id,
            } => {
                self.validate_instance_path(instance_path)?;
                let instance = self
                    .module_instances
                    .get(module_instance_id)
                    .ok_or_else(|| "Binding scope has a missing Module instance".to_string())?;
                self.module_definitions
                    .get(&instance.definition_id)
                    .ok_or_else(|| "Binding target has a missing Module definition".to_string())?
            }
            crate::model::authoring::BindingScope::Query {
                collection,
                predicate,
            } => {
                if collection.trim().is_empty() || predicate.trim().is_empty() {
                    return Err(
                        "Query Binding scope must name a collection and predicate".to_string()
                    );
                }
                return if action_id.is_some() {
                    self.module_definitions
                        .values()
                        .any(|definition| {
                            definition
                                .published_actions
                                .iter()
                                .any(|action| Some(action.id) == action_id)
                        })
                        .then_some(())
                        .ok_or_else(|| {
                            "Query Binding targets an unknown PublishedAction".to_string()
                        })
                } else {
                    self.module_definitions
                        .values()
                        .any(|definition| {
                            definition
                                .published_parameters
                                .iter()
                                .any(|parameter| parameter.id == parameter_id)
                        })
                        .then_some(())
                        .ok_or_else(|| {
                            "Query Binding targets an unknown PublishedParameter".to_string()
                        })
                };
            }
        };
        if let Some(action_id) = action_id {
            definition
                .published_actions
                .iter()
                .any(|action| action.id == action_id)
                .then_some(())
                .ok_or_else(|| "Event Binding targets an unknown PublishedAction".to_string())
        } else {
            definition
                .published_parameters
                .iter()
                .any(|parameter| parameter.id == parameter_id)
                .then_some(())
                .ok_or_else(|| "Signal Binding targets an unknown PublishedParameter".to_string())
        }
    }

    fn validate_signal_source(
        &self,
        source: &crate::model::authoring::SignalSource,
    ) -> Result<(), String> {
        match source {
            crate::model::authoring::SignalSource::Published {
                instance_path,
                module_instance_id,
                signal_id,
            } => {
                self.validate_instance_path(instance_path)?;
                let instance = self
                    .module_instances
                    .get(module_instance_id)
                    .ok_or_else(|| "Signal source has a missing Module instance".to_string())?;
                let definition = self
                    .module_definitions
                    .get(&instance.definition_id)
                    .ok_or_else(|| "Signal source has a missing Module definition".to_string())?;
                definition
                    .published_signals
                    .iter()
                    .any(|signal| signal.id == *signal_id)
                    .then_some(())
                    .ok_or_else(|| "Signal source targets an unknown PublishedSignal".to_string())
            }
            crate::model::authoring::SignalSource::AudioEnvelope { channel }
                if channel.trim().is_empty() =>
            {
                Err("Audio Envelope source must name a channel".to_string())
            }
            crate::model::authoring::SignalSource::MidiControl { device, .. }
                if device.trim().is_empty() =>
            {
                Err("MIDI source must name a device".to_string())
            }
            crate::model::authoring::SignalSource::DataField { data_source, field }
                if data_source.trim().is_empty() || field.trim().is_empty() =>
            {
                Err("Data source must name a source and field".to_string())
            }
            _ => Ok(()),
        }
    }

    fn validate_event_source(
        &self,
        source: &crate::model::authoring::EventSource,
    ) -> Result<(), String> {
        match source {
            crate::model::authoring::EventSource::Published {
                instance_path,
                module_instance_id,
                signal_id,
            } => self.validate_signal_source(&crate::model::authoring::SignalSource::Published {
                instance_path: instance_path.clone(),
                module_instance_id: *module_instance_id,
                signal_id: *signal_id,
            }),
            crate::model::authoring::EventSource::MidiNoteOn { device, .. }
                if device.trim().is_empty() =>
            {
                Err("MIDI event source must name a device".to_string())
            }
            crate::model::authoring::EventSource::Marker { name } if name.trim().is_empty() => {
                Err("Marker event source must have a name".to_string())
            }
            _ => Ok(()),
        }
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
