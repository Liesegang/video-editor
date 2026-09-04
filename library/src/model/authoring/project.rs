use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::frame::color::Color;
use crate::model::project::asset::{Asset, AssetKind};
use crate::model::project::property::PropertyMap;
use crate::model::project::{
    ColorManagementConfig, ColorManagementIssue, ExportConfig, RequestedColorManagementConfig,
    ResolvedColorManagementConfig,
};

use super::{
    Attachment, AttachmentId, AttachmentOwner, AttachmentProcessor, AttachmentStage,
    CompositionParameterTarget, InstanceLocator, InstancePath, MediaInputBinding, MediaOutputKind,
    MediaTime, ModuleDefinition, ModuleDefinitionId, ModuleDefinitionSharing, ModuleInstance,
    ModuleInstanceId, ModuleInvocation, PublishedMediaInput, RationalRate, SourceRef, Timeline,
    TimelineId, TimelineItem, TimelineItemId, TimelineTrack, TimelineTrackId, TimelineTrackKind,
    property_value_type,
};

mod validation;
use validation::*;

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

/// The only persisted editing model in format v1.
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
    pub assets: Vec<Asset>,
    pub(crate) color_management: RequestedColorManagementConfig,
    pub export: ExportConfig,
}

impl AuthoringProject {
    pub fn new(
        name: impl Into<String>,
        width: u64,
        height: u64,
        fps: RationalRate,
        duration: MediaTime,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("Timeline dimensions must be greater than zero".to_string());
        }
        if !fps.is_positive() {
            return Err("Timeline FPS must be greater than zero".to_string());
        }
        if duration.is_negative() {
            return Err("Timeline duration must be non-negative".to_string());
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
                    fps,
                    duration,
                    background_color: Color::black(),
                    color_profile: "sRGB".to_string(),
                    track_order: vec![track_id],
                    authored_properties: PropertyMap::new(),
                    published_parameters: Vec::new(),
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
            assets: Vec::new(),
            color_management: RequestedColorManagementConfig::default(),
            export: ExportConfig::default(),
        })
    }

    /// Explicit floating-point boundary for UI/import callers.
    pub fn new_from_seconds(
        name: impl Into<String>,
        width: u64,
        height: u64,
        fps: f64,
        duration_seconds: f64,
    ) -> Result<Self, String> {
        Self::new(
            name,
            width,
            height,
            RationalRate::from_f64(fps, 1_000_000)?,
            MediaTime::from_seconds_f64(duration_seconds, 1_000_000)?,
        )
    }

    pub fn requested_color_management(&self) -> &RequestedColorManagementConfig {
        &self.color_management
    }

    pub fn requested_color_management_config(&self) -> Option<&ColorManagementConfig> {
        self.color_management.as_config()
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

    pub fn color_management_diagnostics(&self) -> Vec<ColorManagementIssue> {
        self.color_management.diagnostics(&self.assets)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Project name must not be empty".to_string());
        }
        if !self.timelines.contains_key(&self.root_timeline_id) {
            return Err("Project root Timeline does not exist".to_string());
        }
        self.validate_timelines_and_tracks()?;
        self.validate_assets()?;
        self.validate_definitions_and_instances()?;
        self.validate_items()?;
        self.validate_attachments()?;
        self.validate_instance_ownership()?;
        self.validate_parent_cycles()?;
        self.validate_composition_cycles()?;
        self.validate_media_input_cycles()?;
        Ok(())
    }

    fn validate_timelines_and_tracks(&self) -> Result<(), String> {
        let mut ordered_tracks = HashSet::new();
        for (timeline_id, timeline) in &self.timelines {
            if *timeline_id != timeline.id {
                return Err("Timeline map key does not match its ID".to_string());
            }
            if timeline.name.trim().is_empty()
                || timeline.width == 0
                || timeline.height == 0
                || !timeline.fps.is_positive()
                || timeline.duration.is_negative()
                || timeline.color_profile.trim().is_empty()
            {
                return Err(format!("Timeline {} has invalid settings", timeline.id));
            }
            validate_authored_properties(
                &timeline.authored_properties,
                &format!("Timeline {}", timeline.id),
            )?;
            self.validate_composition_parameters(timeline)?;
            let mut local = HashSet::new();
            for track_id in &timeline.track_order {
                if !local.insert(*track_id) || !ordered_tracks.insert(*track_id) {
                    return Err(format!(
                        "Timeline {} lists a Track more than once",
                        timeline.id
                    ));
                }
                let track = self
                    .tracks
                    .get(track_id)
                    .ok_or_else(|| format!("Timeline {} lists a missing Track", timeline.id))?;
                if track.timeline_id != timeline.id {
                    return Err(format!(
                        "Timeline {} lists a Track owned elsewhere",
                        timeline.id
                    ));
                }
            }
        }
        for (track_id, track) in &self.tracks {
            if *track_id != track.id {
                return Err("Track map key does not match its ID".to_string());
            }
            if !ordered_tracks.contains(track_id) {
                return Err(format!("Track {} is not ordered by its Timeline", track.id));
            }
            if track.name.trim().is_empty() {
                return Err(format!("Track {} has no name", track.id));
            }
            validate_authored_properties(
                &track.authored_properties,
                &format!("Track {}", track.id),
            )?;
        }
        Ok(())
    }

    fn validate_composition_parameters(&self, timeline: &Timeline) -> Result<(), String> {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        let mut targets = HashSet::new();
        for parameter in &timeline.published_parameters {
            if !ids.insert(parameter.id) {
                return Err(format!(
                    "Timeline {} repeats a Composition parameter ID",
                    timeline.id
                ));
            }
            let normalized_name = parameter.name.trim().to_lowercase();
            if normalized_name.is_empty() || !names.insert(normalized_name) {
                return Err(format!(
                    "Timeline {} has an invalid or duplicate Composition parameter name",
                    timeline.id
                ));
            }
            if !targets.insert(parameter.target.clone()) {
                return Err(format!(
                    "Timeline {} publishes the same internal control more than once",
                    timeline.id
                ));
            }
            validate_composition_parameter_value(parameter, &parameter.default_value)?;
            let target_item = self.items.get(&parameter.target.item_id()).ok_or_else(|| {
                format!(
                    "Composition parameter {} targets a missing Timeline item",
                    parameter.id
                )
            })?;
            let target_timeline_id = self
                .tracks
                .get(&target_item.track_id)
                .ok_or_else(|| {
                    format!(
                        "Composition parameter {} targets an item with no Track",
                        parameter.id
                    )
                })?
                .timeline_id;
            if target_timeline_id != timeline.id {
                return Err(format!(
                    "Composition parameter {} targets another Timeline",
                    parameter.id
                ));
            }
            match &parameter.target {
                CompositionParameterTarget::TextContent { .. } => {
                    if !matches!(target_item.source, SourceRef::Text { .. })
                        || parameter.data_type != crate::model::project::PortDataType::String
                    {
                        return Err(format!(
                            "Composition parameter {} has an invalid Text target",
                            parameter.id
                        ));
                    }
                }
                CompositionParameterTarget::ItemProperty { property_key, .. } => {
                    if property_key.trim().is_empty() {
                        return Err(format!(
                            "Composition parameter {} has an empty Property key",
                            parameter.id
                        ));
                    }
                    if let Some(value) = target_item
                        .authored_properties
                        .get(property_key)
                        .and_then(|property| property.value())
                        && !parameter.data_type.accepts(property_value_type(value))
                    {
                        return Err(format!(
                            "Composition parameter {} does not match its authored Property",
                            parameter.id
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_assets(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for asset in &self.assets {
            if !ids.insert(asset.id) {
                return Err(format!("Project repeats Asset ID {}", asset.id));
            }
            if asset.name.trim().is_empty() || asset.path.trim().is_empty() {
                return Err(format!("Asset {} has an invalid name or path", asset.id));
            }
        }
        Ok(())
    }

    fn validate_definitions_and_instances(&self) -> Result<(), String> {
        for (definition_id, definition) in &self.module_definitions {
            if *definition_id != definition.id {
                return Err("Module definition map key does not match its ID".to_string());
            }
            definition.validate()?;
            for node in definition.graph.nodes.values() {
                if node
                    .properties()
                    .iter()
                    .any(|(_, property)| property.evaluator == "keyframe")
                {
                    return Err(format!(
                        "Module definition {} contains a Timeline Keyframe",
                        definition.id
                    ));
                }
            }
        }
        for (instance_id, instance) in &self.module_instances {
            if *instance_id != instance.id {
                return Err("Module instance map key does not match its ID".to_string());
            }
            let definition = self
                .module_definitions
                .get(&instance.definition_id)
                .ok_or_else(|| format!("Module instance {} has no definition", instance.id))?;
            for (parameter_id, value) in &instance.parameter_overrides {
                let parameter = definition
                    .interface
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == *parameter_id)
                    .ok_or_else(|| {
                        format!(
                            "Module instance {} overrides an unpublished parameter",
                            instance.id
                        )
                    })?;
                validate_parameter_value(parameter, value)?;
            }
        }
        Ok(())
    }

    fn validate_items(&self) -> Result<(), String> {
        let asset_ids = self
            .assets
            .iter()
            .map(|asset| asset.id)
            .collect::<HashSet<_>>();
        for (item_id, item) in &self.items {
            if *item_id != item.id {
                return Err("Timeline item map key does not match its ID".to_string());
            }
            if item.name.trim().is_empty() {
                return Err(format!("Timeline item {} has no name", item.id));
            }
            let track = self
                .tracks
                .get(&item.track_id)
                .ok_or_else(|| format!("Item {} refers to a missing Track", item.id))?;
            if item.interval.start.is_negative()
                || item.interval.duration.is_negative()
                || item.time_map.source_start.is_negative()
                || item.time_map.playback_rate.numerator() < 0
            {
                return Err(format!("Item {} has invalid timing", item.id));
            }
            item.interval.end()?;
            validate_authored_properties(
                &item.authored_properties,
                &format!("Timeline item {}", item.id),
            )?;
            if let Some(parent_id) = item.parent {
                let parent = self
                    .items
                    .get(&parent_id)
                    .ok_or_else(|| format!("Item {} has a missing parent", item.id))?;
                let parent_track = self
                    .tracks
                    .get(&parent.track_id)
                    .ok_or_else(|| format!("Parent of item {} has a missing Track", item.id))?;
                if parent.id == item.id || parent_track.timeline_id != track.timeline_id {
                    return Err(format!("Item {} has an invalid parent", item.id));
                }
            }
            match &item.source {
                SourceRef::Asset { asset_id } if !asset_ids.contains(asset_id) => {
                    return Err(format!("Item {} refers to a missing Asset", item.id));
                }
                SourceRef::Composition(instance) => {
                    let nested = self.timelines.get(&instance.timeline_id).ok_or_else(|| {
                        format!("Item {} refers to a missing nested Timeline", item.id)
                    })?;
                    validate_duration_policy(item, nested.duration, &instance.duration_policy)?;
                    for (parameter_id, value) in &instance.parameter_overrides {
                        let parameter = nested
                            .published_parameters
                            .iter()
                            .find(|parameter| parameter.id == *parameter_id)
                            .ok_or_else(|| {
                                format!(
                                    "Composition item {} overrides an unpublished parameter",
                                    item.id
                                )
                            })?;
                        validate_composition_parameter_value(parameter, value)?;
                    }
                }
                SourceRef::Module(invocation) => {
                    self.validate_invocation(invocation, Some(item), track.timeline_id, false)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_attachments(&self) -> Result<(), String> {
        let mut stack_orders: HashMap<(AttachmentOwner, AttachmentStage), Vec<i64>> =
            HashMap::new();
        for (attachment_id, attachment) in &self.attachments {
            if *attachment_id != attachment.id {
                return Err("Attachment map key does not match its ID".to_string());
            }
            if attachment.order < 0 || !self.attachment_owner_exists(&attachment.owner) {
                return Err(format!(
                    "Attachment {} has an invalid owner or order",
                    attachment.id
                ));
            }
            validate_attachment_stage(&attachment.owner, attachment.stage)?;
            stack_orders
                .entry((attachment.owner.clone(), attachment.stage))
                .or_default()
                .push(attachment.order);
            match &attachment.processor {
                AttachmentProcessor::BuiltinEffect(effect) => {
                    validate_builtin_effect(effect, attachment.stage)?;
                }
                AttachmentProcessor::Module(invocation) => {
                    self.validate_invocation(
                        invocation,
                        None,
                        self.attachment_owner_timeline(&attachment.owner)?,
                        true,
                    )?;
                    self.validate_module_attachment_contract(invocation, attachment.stage)?;
                }
            }
        }
        for ((owner, stage), mut orders) in stack_orders {
            orders.sort_unstable();
            if orders
                .iter()
                .enumerate()
                .any(|(expected, actual)| *actual != expected as i64)
            {
                return Err(format!(
                    "Attachment stack {owner:?}/{stage:?} has invalid order"
                ));
            }
        }
        Ok(())
    }

    fn validate_module_attachment_contract(
        &self,
        invocation: &ModuleInvocation,
        stage: AttachmentStage,
    ) -> Result<(), String> {
        let expected = attachment_media_type(stage)?;
        let instance = self
            .module_instances
            .get(&invocation.instance_id)
            .ok_or_else(|| "Module Attachment has a missing instance".to_string())?;
        let definition = self
            .module_definitions
            .get(&instance.definition_id)
            .ok_or_else(|| "Module Attachment has a missing definition".to_string())?;
        let output = definition
            .interface
            .media_outputs
            .iter()
            .find(|output| output.id == invocation.output_id)
            .ok_or_else(|| "Module Attachment has a missing output".to_string())?;
        if output.data_type != expected {
            return Err("Module Attachment output is incompatible with its Stage".to_string());
        }
        let primary = definition
            .interface
            .media_inputs
            .iter()
            .find(|input| input.primary)
            .ok_or_else(|| "Module Attachment requires one primary media input".to_string())?;
        if primary.data_type != expected {
            return Err(
                "Module Attachment primary input is incompatible with its Stage".to_string(),
            );
        }
        Ok(())
    }

    fn validate_invocation(
        &self,
        invocation: &ModuleInvocation,
        host_item: Option<&TimelineItem>,
        host_timeline_id: TimelineId,
        primary_input_is_implicit: bool,
    ) -> Result<(), String> {
        let instance = self
            .module_instances
            .get(&invocation.instance_id)
            .ok_or_else(|| {
                format!(
                    "Invocation has missing Module instance {}",
                    invocation.instance_id
                )
            })?;
        let definition = self
            .module_definitions
            .get(&instance.definition_id)
            .ok_or_else(|| {
                format!(
                    "Invocation has missing Module definition {}",
                    instance.definition_id
                )
            })?;
        definition
            .interface
            .media_outputs
            .iter()
            .find(|output| output.id == invocation.output_id)
            .ok_or_else(|| "Invocation selects an unknown Published media output".to_string())?;
        for input_id in invocation.input_bindings.keys() {
            if !definition
                .interface
                .media_inputs
                .iter()
                .any(|input| input.id == *input_id)
            {
                return Err("Invocation binds an unknown Published media input".to_string());
            }
        }
        for input in &definition.interface.media_inputs {
            if input.required
                && !(primary_input_is_implicit && input.primary)
                && !invocation.input_bindings.contains_key(&input.id)
            {
                return Err(format!(
                    "Invocation leaves required media input {} unbound",
                    input.id
                ));
            }
            if let Some(binding) = invocation.input_bindings.get(&input.id) {
                self.validate_media_binding(host_item, host_timeline_id, input, binding)?;
            }
        }
        for (parameter_id, track) in &invocation.automation_tracks {
            let parameter = definition
                .interface
                .parameters
                .iter()
                .find(|parameter| parameter.id == *parameter_id)
                .ok_or_else(|| "Invocation automates an unpublished parameter".to_string())?;
            validate_automation(track, parameter)?;
        }
        Ok(())
    }

    fn validate_media_binding(
        &self,
        host_item: Option<&TimelineItem>,
        host_timeline_id: TimelineId,
        input: &PublishedMediaInput,
        binding: &MediaInputBinding,
    ) -> Result<(), String> {
        let MediaInputBinding::TimelineItemOutput {
            locator,
            item_id,
            output,
            ..
        } = binding;
        let source = self
            .items
            .get(item_id)
            .ok_or_else(|| "Media input binding refers to a missing Timeline item".to_string())?;
        if host_item.is_some_and(|host| host.id == source.id) {
            return Err("A Node Clip cannot bind its own output as an input".to_string());
        }
        let source_timeline_id = self
            .tracks
            .get(&source.track_id)
            .ok_or_else(|| "Media input source has a missing Track".to_string())?
            .timeline_id;
        if matches!(locator, InstanceLocator::SameTimeline)
            && host_timeline_id != source_timeline_id
        {
            return Err("Same-Timeline media binding crosses a Timeline boundary".to_string());
        }
        if let InstanceLocator::Exact(path) = locator {
            let path_timeline_id = self.validate_instance_path(path)?;
            if path_timeline_id != source_timeline_id {
                return Err(
                    "Exact media binding source does not belong to its InstancePath".to_string(),
                );
            }
        }
        let output_type = match output {
            MediaOutputKind::Image => crate::model::project::PortDataType::Image,
            MediaOutputKind::Audio => crate::model::project::PortDataType::Audio,
        };
        if input.data_type != output_type || !self.item_supports_output(source, *output)? {
            return Err("Media input binding has an incompatible output type".to_string());
        }
        Ok(())
    }

    fn item_supports_output(
        &self,
        item: &TimelineItem,
        output: MediaOutputKind,
    ) -> Result<bool, String> {
        Ok(match &item.source {
            SourceRef::Asset { asset_id } => self
                .assets
                .iter()
                .find(|asset| asset.id == *asset_id)
                .is_some_and(|asset| match output {
                    MediaOutputKind::Image => {
                        matches!(asset.kind, AssetKind::Image | AssetKind::Video)
                    }
                    MediaOutputKind::Audio => {
                        matches!(asset.kind, AssetKind::Audio | AssetKind::Video)
                    }
                }),
            SourceRef::Text { .. } | SourceRef::Shape { .. } | SourceRef::Solid { .. } => {
                output == MediaOutputKind::Image
            }
            SourceRef::Composition(instance) => match output {
                MediaOutputKind::Image => true,
                MediaOutputKind::Audio => self.tracks.values().any(|track| {
                    track.timeline_id == instance.timeline_id
                        && track.kind != TimelineTrackKind::Visual
                }),
            },
            SourceRef::Module(invocation) => {
                let instance = self
                    .module_instances
                    .get(&invocation.instance_id)
                    .ok_or_else(|| "Media source has a missing Module instance".to_string())?;
                let definition = self
                    .module_definitions
                    .get(&instance.definition_id)
                    .ok_or_else(|| "Media source has a missing Module definition".to_string())?;
                definition
                    .interface
                    .media_outputs
                    .iter()
                    .find(|candidate| candidate.id == invocation.output_id)
                    .is_some_and(|candidate| {
                        candidate.data_type
                            == match output {
                                MediaOutputKind::Image => {
                                    crate::model::project::PortDataType::Image
                                }
                                MediaOutputKind::Audio => {
                                    crate::model::project::PortDataType::Audio
                                }
                            }
                    })
            }
        })
    }

    fn validate_instance_ownership(&self) -> Result<(), String> {
        let mut owners = HashMap::<ModuleInstanceId, usize>::new();
        for item in self.items.values() {
            if let SourceRef::Module(invocation) = &item.source {
                *owners.entry(invocation.instance_id).or_default() += 1;
            }
        }
        for attachment in self.attachments.values() {
            if let AttachmentProcessor::Module(invocation) = &attachment.processor {
                *owners.entry(invocation.instance_id).or_default() += 1;
            }
        }
        for instance_id in self.module_instances.keys() {
            if owners.get(instance_id) != Some(&1) {
                return Err(format!(
                    "Module instance {instance_id} must have exactly one Timeline host"
                ));
            }
        }
        let mut definition_instances = HashMap::<ModuleDefinitionId, usize>::new();
        for instance in self.module_instances.values() {
            *definition_instances
                .entry(instance.definition_id)
                .or_default() += 1;
        }
        for definition in self.module_definitions.values() {
            let count = definition_instances
                .get(&definition.id)
                .copied()
                .unwrap_or(0);
            match definition.sharing {
                ModuleDefinitionSharing::Private if count != 1 => {
                    return Err(format!(
                        "Private Module definition {} must belong to exactly one instance",
                        definition.id
                    ));
                }
                ModuleDefinitionSharing::SharedLocal if count == 0 => {
                    return Err(format!(
                        "Locally shared Module definition {} has no instance",
                        definition.id
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_instance_path(&self, path: &InstancePath) -> Result<TimelineId, String> {
        if path.root_timeline_id != self.root_timeline_id {
            return Err("InstancePath must start at the Project root Timeline".to_string());
        }
        let mut timeline_id = path.root_timeline_id;
        if !self.timelines.contains_key(&timeline_id) {
            return Err("InstancePath has a missing root Timeline".to_string());
        }
        for item_id in &path.composition_items {
            let item = self
                .items
                .get(item_id)
                .ok_or_else(|| "InstancePath has a missing item".to_string())?;
            let item_timeline_id = self
                .tracks
                .get(&item.track_id)
                .ok_or_else(|| "InstancePath item has a missing Track".to_string())?
                .timeline_id;
            if item_timeline_id != timeline_id {
                return Err("InstancePath does not follow the Timeline hierarchy".to_string());
            }
            let SourceRef::Composition(instance) = &item.source else {
                return Err("InstancePath contains a non-Composition item".to_string());
            };
            timeline_id = instance.timeline_id;
        }
        Ok(timeline_id)
    }

    fn attachment_owner_exists(&self, owner: &AttachmentOwner) -> bool {
        match owner {
            AttachmentOwner::Timeline { timeline_id } => self.timelines.contains_key(timeline_id),
            AttachmentOwner::Track { track_id } => self.tracks.contains_key(track_id),
            AttachmentOwner::Item { item_id } => self.items.contains_key(item_id),
        }
    }

    fn attachment_owner_timeline(&self, owner: &AttachmentOwner) -> Result<TimelineId, String> {
        match owner {
            AttachmentOwner::Timeline { timeline_id } => self
                .timelines
                .contains_key(timeline_id)
                .then_some(*timeline_id)
                .ok_or_else(|| "Attachment owner Timeline does not exist".to_string()),
            AttachmentOwner::Track { track_id } => self
                .tracks
                .get(track_id)
                .map(|track| track.timeline_id)
                .ok_or_else(|| "Attachment owner Track does not exist".to_string()),
            AttachmentOwner::Item { item_id } => {
                let item = self
                    .items
                    .get(item_id)
                    .ok_or_else(|| "Attachment owner item does not exist".to_string())?;
                self.tracks
                    .get(&item.track_id)
                    .map(|track| track.timeline_id)
                    .ok_or_else(|| "Attachment owner item has a missing Track".to_string())
            }
        }
    }

    fn validate_parent_cycles(&self) -> Result<(), String> {
        for item in self.items.values() {
            let mut active = HashSet::new();
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

    fn validate_composition_cycles(&self) -> Result<(), String> {
        fn visit(
            project: &AuthoringProject,
            timeline_id: TimelineId,
            visiting: &mut HashSet<TimelineId>,
            visited: &mut HashSet<TimelineId>,
        ) -> Result<(), String> {
            if visited.contains(&timeline_id) {
                return Ok(());
            }
            if !visiting.insert(timeline_id) {
                return Err(format!("Nested Timeline cycle reaches {timeline_id}"));
            }
            for item in project.items.values() {
                let item_timeline_id = project
                    .tracks
                    .get(&item.track_id)
                    .ok_or_else(|| format!("Timeline item {} has a missing Track", item.id))?
                    .timeline_id;
                if item_timeline_id != timeline_id {
                    continue;
                }
                if let SourceRef::Composition(instance) = &item.source {
                    visit(project, instance.timeline_id, visiting, visited)?;
                }
            }
            visiting.remove(&timeline_id);
            visited.insert(timeline_id);
            Ok(())
        }
        let mut visited = HashSet::new();
        for timeline_id in self.timelines.keys().copied() {
            visit(self, timeline_id, &mut HashSet::new(), &mut visited)?;
        }
        Ok(())
    }

    fn validate_media_input_cycles(&self) -> Result<(), String> {
        let mut edges: HashMap<TimelineItemId, Vec<TimelineItemId>> = HashMap::new();
        for item in self.items.values() {
            if let SourceRef::Module(invocation) = &item.source {
                edges.insert(item.id, invocation_input_items(invocation));
            }
        }
        fn visit(
            item_id: TimelineItemId,
            edges: &HashMap<TimelineItemId, Vec<TimelineItemId>>,
            visiting: &mut HashSet<TimelineItemId>,
            visited: &mut HashSet<TimelineItemId>,
        ) -> Result<(), String> {
            if visited.contains(&item_id) {
                return Ok(());
            }
            if !visiting.insert(item_id) {
                return Err(format!("Media input cycle reaches Timeline item {item_id}"));
            }
            for source in edges.get(&item_id).into_iter().flatten() {
                visit(*source, edges, visiting, visited)?;
            }
            visiting.remove(&item_id);
            visited.insert(item_id);
            Ok(())
        }
        let mut visited = HashSet::new();
        for item_id in edges.keys().copied() {
            visit(item_id, &edges, &mut HashSet::new(), &mut visited)?;
        }
        Ok(())
    }
}
