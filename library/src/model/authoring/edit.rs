use ordered_float::OrderedFloat;

use crate::model::frame::color::Color;
use crate::model::project::asset::Asset;
use crate::model::project::property::{Property, PropertyMap};

use super::{
    Attachment, AttachmentId, AttachmentOwner, AttachmentStage, AuthoringProject, Constraint,
    ConstraintId, ConstraintKind, DataSource, EventBinding, EventBindingId, GeneratedItem, Mask,
    MaskId, MaskMode, MatteRef, ModuleConnection, ModuleConnectionId, ModuleDefinition,
    ModuleDefinitionId, ModuleGraph, ModuleInstance, ModuleInstanceId, ModuleRole, Override,
    OverrideId, OverrideOperator, OverridePatch, OverridePath, OverrideStatus, PublishedParameter,
    PublishedParameterId, SignalBinding, SignalBindingId, SourceRef, Timeline, TimelineId,
    TimelineInterval, TimelineItem, TimelineItemId, TimelineTrack, TimelineTrackId,
    TimelineTrackKind, Transition, TransitionId, TransitionKind,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct ProjectRevision(u64);

impl ProjectRevision {
    pub const fn initial() -> Self {
        Self(0)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

fn binding_references_item(
    scope: &crate::model::authoring::BindingScope,
    item_id: TimelineItemId,
) -> bool {
    matches!(
        scope,
        crate::model::authoring::BindingScope::Instance { instance_path, .. }
            if instance_path.composition_items.contains(&item_id)
    )
}

fn signal_source_references_item(
    source: &crate::model::authoring::SignalSource,
    item_id: TimelineItemId,
) -> bool {
    matches!(
        source,
        crate::model::authoring::SignalSource::Published { instance_path, .. }
            if instance_path.composition_items.contains(&item_id)
    )
}

fn event_source_references_item(
    source: &crate::model::authoring::EventSource,
    item_id: TimelineItemId,
) -> bool {
    matches!(
        source,
        crate::model::authoring::EventSource::Published { instance_path, .. }
            if instance_path.composition_items.contains(&item_id)
    )
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ProjectInvalidation {
    TimelineStructure {
        timeline_id: TimelineId,
    },
    TimelineRange {
        timeline_id: TimelineId,
        start: OrderedFloat<f64>,
        duration: OrderedFloat<f64>,
    },
    ItemProperties {
        timeline_id: TimelineId,
        item_id: TimelineItemId,
    },
    Asset {
        asset_id: uuid::Uuid,
    },
    ModuleDefinition {
        definition_id: ModuleDefinitionId,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChangeSet {
    pub revision: ProjectRevision,
    pub invalidations: Vec<ProjectInvalidation>,
}

pub struct AuthoringSession {
    project: AuthoringProject,
    revision: ProjectRevision,
}

impl AuthoringSession {
    pub fn new(project: AuthoringProject) -> Result<Self, String> {
        project.validate()?;
        Ok(Self {
            project,
            revision: ProjectRevision::initial(),
        })
    }

    pub fn project(&self) -> &AuthoringProject {
        &self.project
    }

    pub fn revision(&self) -> ProjectRevision {
        self.revision
    }

    pub fn into_project(self) -> AuthoringProject {
        self.project
    }

    pub fn add_item(
        &mut self,
        track_id: TimelineTrackId,
        name: String,
        source: SourceRef,
        interval: TimelineInterval,
        layer: i64,
    ) -> Result<(TimelineItemId, ChangeSet), String> {
        let timeline_id = self.timeline_for_track(track_id)?;
        let item_id = TimelineItemId::new();
        self.project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name,
                source,
                interval,
                layer,
                parent: None,
                mask_ids: Vec::new(),
                matte: None,
                constraints: Vec::new(),
                transition_in: None,
                transition_out: None,
                generated_item_id: None,
                authored_properties: PropertyMap::new(),
            },
        );
        Ok((
            item_id,
            self.finish(vec![ProjectInvalidation::TimelineStructure { timeline_id }]),
        ))
    }

    pub fn add_timeline(
        &mut self,
        name: String,
        width: u64,
        height: u64,
        fps: f64,
        duration: f64,
    ) -> Result<(TimelineId, TimelineTrackId, ChangeSet), String> {
        validate_timeline_settings(width, height, fps, duration)?;
        let timeline_id = TimelineId::new();
        let track_id = TimelineTrackId::new();
        self.project.timelines.insert(
            timeline_id,
            Timeline {
                id: timeline_id,
                name,
                width,
                height,
                fps: OrderedFloat(fps),
                duration: OrderedFloat(duration),
                background_color: Color::black(),
                color_profile: "sRGB".to_string(),
                track_order: vec![track_id],
                authored_properties: PropertyMap::new(),
            },
        );
        self.project.tracks.insert(
            track_id,
            TimelineTrack {
                id: track_id,
                timeline_id,
                name: "Video 1".to_string(),
                kind: TimelineTrackKind::AudioVisual,
                authored_properties: PropertyMap::new(),
            },
        );
        let change = self.finish(vec![ProjectInvalidation::TimelineStructure { timeline_id }]);
        Ok((timeline_id, track_id, change))
    }

    pub fn add_track(
        &mut self,
        timeline_id: TimelineId,
        name: String,
        kind: TimelineTrackKind,
    ) -> Result<(TimelineTrackId, ChangeSet), String> {
        let timeline = self
            .project
            .timelines
            .get_mut(&timeline_id)
            .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
        let track_id = TimelineTrackId::new();
        timeline.track_order.push(track_id);
        self.project.tracks.insert(
            track_id,
            TimelineTrack {
                id: track_id,
                timeline_id,
                name,
                kind,
                authored_properties: PropertyMap::new(),
            },
        );
        Ok((
            track_id,
            self.finish(vec![ProjectInvalidation::TimelineStructure { timeline_id }]),
        ))
    }

    pub fn add_asset(&mut self, asset: Asset) -> Result<ChangeSet, String> {
        if self
            .project
            .assets
            .iter()
            .any(|current| current.id == asset.id)
        {
            return Err(format!("Asset {} already exists", asset.id));
        }
        let asset_id = asset.id;
        self.project.assets.push(asset);
        Ok(self.finish(vec![ProjectInvalidation::Asset { asset_id }]))
    }

    pub fn move_item(
        &mut self,
        item_id: TimelineItemId,
        track_id: TimelineTrackId,
        start: f64,
        layer: i64,
    ) -> Result<ChangeSet, String> {
        if !start.is_finite() || start < 0.0 {
            return Err("Timeline item start must be finite and non-negative".to_string());
        }
        let target_timeline = self.timeline_for_track(track_id)?;
        let (old_timeline, old_interval, parent) = {
            let item = self
                .project
                .items
                .get(&item_id)
                .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
            (
                self.timeline_for_track(item.track_id)?,
                item.interval,
                item.parent,
            )
        };
        if old_timeline != target_timeline && parent.is_some() {
            return Err("A parented item cannot move to another Timeline".to_string());
        }
        let item = self
            .project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        item.track_id = track_id;
        item.interval.start = OrderedFloat(start);
        item.layer = layer;
        let mut invalidations = vec![range_invalidation(old_timeline, old_interval)];
        invalidations.push(range_invalidation(target_timeline, item.interval));
        Ok(self.finish(invalidations))
    }

    pub fn trim_item(
        &mut self,
        item_id: TimelineItemId,
        interval: TimelineInterval,
    ) -> Result<ChangeSet, String> {
        let item = self
            .project
            .items
            .get(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        let timeline_id = self.timeline_for_track(item.track_id)?;
        let old_interval = item.interval;
        let mut source = item.source.clone();
        shift_source_start(
            &mut source,
            interval.start.into_inner() - old_interval.start.into_inner(),
        )?;
        let item = self
            .project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        item.interval = interval;
        item.source = source;
        Ok(self.finish(vec![
            range_invalidation(timeline_id, old_interval),
            range_invalidation(timeline_id, interval),
        ]))
    }

    pub fn split_item(
        &mut self,
        item_id: TimelineItemId,
        timeline_time: f64,
    ) -> Result<(TimelineItemId, ChangeSet), String> {
        let original = self
            .project
            .items
            .get(&item_id)
            .cloned()
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        let start = original.interval.start.into_inner();
        let end = start + original.interval.duration.into_inner();
        if !timeline_time.is_finite() || timeline_time <= start || timeline_time >= end {
            return Err("Split time must be strictly inside the Timeline item".to_string());
        }
        let timeline_id = self.timeline_for_track(original.track_id)?;
        let offset = timeline_time - start;
        let right_id = TimelineItemId::new();
        let mut right = original.clone();
        right.id = right_id;
        right.name = format!("{} (split)", original.name);
        right.interval = TimelineInterval::new(timeline_time, end - timeline_time)?;
        shift_source_start(&mut right.source, offset)?;
        right.transition_in = None;
        right.mask_ids = self.clone_masks(&original.mask_ids);

        let left = self
            .project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        left.interval.duration = OrderedFloat(offset);
        left.transition_out = None;

        if let Some(transition_id) = original.transition_out
            && let Some(transition) = self.project.transitions.get_mut(&transition_id)
        {
            if transition.from_item_id == item_id {
                transition.from_item_id = right_id;
            }
            if transition.to_item_id == item_id {
                transition.to_item_id = right_id;
            }
        }
        self.project.items.insert(right_id, right);
        self.clone_item_attachments(item_id, right_id)?;
        Ok((
            right_id,
            self.finish(vec![ProjectInvalidation::TimelineStructure { timeline_id }]),
        ))
    }

    pub fn delete_item(
        &mut self,
        item_id: TimelineItemId,
        ripple: bool,
    ) -> Result<ChangeSet, String> {
        let item = self
            .project
            .items
            .get(&item_id)
            .cloned()
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        let timeline_id = self.timeline_for_track(item.track_id)?;
        let removed_end = item.interval.start.into_inner() + item.interval.duration.into_inner();
        if ripple {
            for candidate in self.project.items.values_mut().filter(|candidate| {
                candidate.track_id == item.track_id
                    && candidate.id != item_id
                    && candidate.interval.start.into_inner() >= removed_end
            }) {
                candidate.interval.start = OrderedFloat(
                    candidate.interval.start.into_inner() - item.interval.duration.into_inner(),
                );
            }
        }
        let removed_transition_ids = self
            .project
            .transitions
            .values()
            .filter(|transition| {
                transition.from_item_id == item_id || transition.to_item_id == item_id
            })
            .map(|transition| transition.id)
            .collect::<std::collections::HashSet<_>>();
        for child in self.project.items.values_mut() {
            if child.parent == Some(item_id) {
                child.parent = None;
            }
            if child.matte.is_some_and(|matte| matte.item_id == item_id) {
                child.matte = None;
            }
            child
                .constraints
                .retain(|constraint| constraint.target_item_id != item_id);
            if child
                .transition_in
                .is_some_and(|id| removed_transition_ids.contains(&id))
            {
                child.transition_in = None;
            }
            if child
                .transition_out
                .is_some_and(|id| removed_transition_ids.contains(&id))
            {
                child.transition_out = None;
            }
        }
        for mask_id in &item.mask_ids {
            self.project.masks.remove(mask_id);
        }
        self.project.transitions.retain(|_, transition| {
            transition.from_item_id != item_id && transition.to_item_id != item_id
        });
        let removed_instances: Vec<_> = self
            .project
            .attachments
            .values()
            .filter(|attachment| attachment.owner == (AttachmentOwner::Item { item_id }))
            .map(|attachment| attachment.module_instance_id)
            .collect();
        self.project
            .attachments
            .retain(|_, attachment| attachment.owner != (AttachmentOwner::Item { item_id }));
        for instance_id in removed_instances {
            if !self
                .project
                .attachments
                .values()
                .any(|attachment| attachment.module_instance_id == instance_id)
                && !self.project.items.values().any(|candidate| {
                    matches!(candidate.source, SourceRef::Module { module_instance_id } if module_instance_id == instance_id)
                })
            {
                self.project.module_instances.remove(&instance_id);
            }
        }
        self.project.signal_bindings.retain(|_, binding| {
            !binding_references_item(&binding.scope, item_id)
                && !signal_source_references_item(&binding.source, item_id)
        });
        self.project.event_bindings.retain(|_, binding| {
            !binding_references_item(&binding.scope, item_id)
                && !event_source_references_item(&binding.source, item_id)
        });
        self.project.items.remove(&item_id);
        Ok(self.finish(vec![ProjectInvalidation::TimelineStructure { timeline_id }]))
    }

    pub fn set_item_property(
        &mut self,
        item_id: TimelineItemId,
        key: String,
        property: Property,
    ) -> Result<ChangeSet, String> {
        let item = self
            .project
            .items
            .get(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        let timeline_id = self.timeline_for_track(item.track_id)?;
        self.project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .authored_properties
            .set(key, property);
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn update_item_property_value(
        &mut self,
        item_id: TimelineItemId,
        key: String,
        time: f64,
        value: crate::model::project::property::PropertyValue,
    ) -> Result<ChangeSet, String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        let generated_item_id = self
            .project
            .items
            .get(&item_id)
            .and_then(|item| item.generated_item_id);
        self.project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .authored_properties
            .update_property_or_keyframe(&key, time, value.clone(), None);
        if let Some(generated_item_id) = generated_item_id {
            self.record_generated_override(
                generated_item_id,
                OverridePath::AuthoredProperty { key: key.clone() },
                value,
            );
        }
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn upsert_item_keyframe(
        &mut self,
        item_id: TimelineItemId,
        key: String,
        time: f64,
        value: crate::model::project::property::PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<ChangeSet, String> {
        if !time.is_finite() || time < 0.0 {
            return Err("Keyframe time must be finite and non-negative".to_string());
        }
        let timeline_id = self.timeline_for_item(item_id)?;
        self.project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .authored_properties
            .upsert_keyframe(&key, time, value, easing);
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn update_item_keyframe(
        &mut self,
        item_id: TimelineItemId,
        key: String,
        keyframe_id: crate::model::project::property::KeyframeId,
        update: crate::model::project::property::KeyframeUpdate,
    ) -> Result<ChangeSet, String> {
        if update
            .time
            .is_some_and(|time| !time.is_finite() || time < 0.0)
        {
            return Err("Keyframe time must be finite and non-negative".to_string());
        }
        let timeline_id = self.timeline_for_item(item_id)?;
        let property = self
            .project
            .items
            .get_mut(&item_id)
            .and_then(|item| item.authored_properties.get_mut(&key))
            .ok_or_else(|| format!("Missing keyframed property '{key}'"))?;
        if !property.update_keyframe_by_id(keyframe_id, update) {
            return Err(format!("Missing Keyframe {keyframe_id}"));
        }
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn remove_item_keyframe(
        &mut self,
        item_id: TimelineItemId,
        key: String,
        keyframe_id: crate::model::project::property::KeyframeId,
    ) -> Result<ChangeSet, String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        let property = self
            .project
            .items
            .get_mut(&item_id)
            .and_then(|item| item.authored_properties.get_mut(&key))
            .ok_or_else(|| format!("Missing keyframed property '{key}'"))?;
        if !property.remove_keyframe_by_id(keyframe_id) {
            return Err(format!("Missing Keyframe {keyframe_id}"));
        }
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn rename_item(
        &mut self,
        item_id: TimelineItemId,
        name: String,
    ) -> Result<ChangeSet, String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        if name.trim().is_empty() {
            return Err("Timeline item name cannot be empty".to_string());
        }
        self.project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .name = name;
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn set_text(&mut self, item_id: TimelineItemId, text: String) -> Result<ChangeSet, String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        let generated_item_id = self
            .project
            .items
            .get(&item_id)
            .and_then(|item| item.generated_item_id);
        let item = self
            .project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        let SourceRef::Text { text: current } = &mut item.source else {
            return Err("Selected Timeline item is not Text".to_string());
        };
        *current = text.clone();
        if let Some(generated_item_id) = generated_item_id {
            self.record_generated_override(
                generated_item_id,
                OverridePath::SourceText,
                crate::model::project::property::PropertyValue::String(text),
            );
        }
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn attach_effect_module(
        &mut self,
        item_id: TimelineItemId,
        name: String,
        node: crate::model::node::Node,
    ) -> Result<(ModuleInstanceId, ChangeSet), String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        let node_id = node.id;
        let mut published_parameters = Vec::new();
        for (property_name, property) in node.properties().iter() {
            let port = format!("{}{}", crate::plugin::PROPERTY_PORT_PREFIX, property_name);
            let data_type = match node.content() {
                crate::model::node::NodeContent::PluginOperation(operation) => operation
                    .declared_ports
                    .iter()
                    .find(|definition| definition.key == port)
                    .map(|definition| definition.data_type)
                    .unwrap_or(crate::model::project::PortDataType::Any),
                _ => crate::model::project::PortDataType::Any,
            };
            let Some(default_value) = property.value().cloned() else {
                continue;
            };
            published_parameters.push(PublishedParameter {
                id: PublishedParameterId::new(),
                name: property_name.clone(),
                data_type,
                default_value,
                target: super::ModulePortAddress { node_id, port },
            });
        }
        let definition_id = ModuleDefinitionId::new();
        self.project.module_definitions.insert(
            definition_id,
            ModuleDefinition {
                id: definition_id,
                name,
                role: ModuleRole::Effect,
                graph: ModuleGraph {
                    nodes: std::collections::HashMap::from([(node_id, node)]),
                    connections: Vec::new(),
                },
                published_parameters,
                published_signals: Vec::new(),
                published_actions: Vec::new(),
                version: 1,
            },
        );
        let instance_id = ModuleInstanceId::new();
        self.project.module_instances.insert(
            instance_id,
            ModuleInstance {
                id: instance_id,
                definition_id,
                parameter_overrides: std::collections::HashMap::new(),
            },
        );
        let attachment_id = AttachmentId::new();
        let order = self
            .project
            .attachments
            .values()
            .filter(|attachment| attachment.owner == AttachmentOwner::Item { item_id })
            .map(|attachment| attachment.order)
            .max()
            .unwrap_or(-1)
            .saturating_add(1);
        self.project.attachments.insert(
            attachment_id,
            Attachment {
                id: attachment_id,
                owner: AttachmentOwner::Item { item_id },
                module_instance_id: instance_id,
                stage: AttachmentStage::ItemPostTransform,
                order,
            },
        );
        Ok((
            instance_id,
            self.finish(vec![ProjectInvalidation::ItemProperties {
                timeline_id,
                item_id,
            }]),
        ))
    }

    pub fn set_module_parameter(
        &mut self,
        instance_id: ModuleInstanceId,
        parameter_id: PublishedParameterId,
        value: crate::model::project::property::PropertyValue,
    ) -> Result<ChangeSet, String> {
        let instance = self
            .project
            .module_instances
            .get_mut(&instance_id)
            .ok_or_else(|| format!("Missing Module instance {instance_id}"))?;
        let definition = self
            .project
            .module_definitions
            .get(&instance.definition_id)
            .ok_or_else(|| format!("Missing Module definition {}", instance.definition_id))?;
        if !definition
            .published_parameters
            .iter()
            .any(|parameter| parameter.id == parameter_id)
        {
            return Err("Published parameter does not belong to the Module definition".to_string());
        }
        instance.parameter_overrides.insert(parameter_id, value);
        let invalidations = self
            .project
            .attachments
            .values()
            .filter(|attachment| attachment.module_instance_id == instance_id)
            .filter_map(|attachment| match attachment.owner {
                AttachmentOwner::Item { item_id } => {
                    self.timeline_for_item(item_id).ok().map(|timeline_id| {
                        ProjectInvalidation::ItemProperties {
                            timeline_id,
                            item_id,
                        }
                    })
                }
                AttachmentOwner::Track { track_id } => self
                    .timeline_for_track(track_id)
                    .ok()
                    .map(|timeline_id| ProjectInvalidation::TimelineStructure { timeline_id }),
                AttachmentOwner::Timeline { timeline_id } => {
                    Some(ProjectInvalidation::TimelineStructure { timeline_id })
                }
            })
            .collect();
        Ok(self.finish(invalidations))
    }

    pub fn set_module_node_state(
        &mut self,
        definition_id: ModuleDefinitionId,
        node_id: uuid::Uuid,
        name: String,
        enabled: bool,
        bypassed: bool,
    ) -> Result<ChangeSet, String> {
        if name.trim().is_empty() {
            return Err("Module Node name cannot be empty".to_string());
        }
        let definition = self
            .project
            .module_definitions
            .get_mut(&definition_id)
            .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
        let node = definition
            .graph
            .nodes
            .get_mut(&node_id)
            .ok_or_else(|| format!("Missing Module Node {node_id}"))?;
        node.name = name;
        node.enabled = enabled;
        node.bypassed = bypassed;
        definition.version = definition
            .version
            .checked_add(1)
            .ok_or_else(|| "Module definition version overflow".to_string())?;
        Ok(self.finish(vec![ProjectInvalidation::ModuleDefinition {
            definition_id,
        }]))
    }

    pub fn add_effect_node_to_module(
        &mut self,
        definition_id: ModuleDefinitionId,
        node: crate::model::node::Node,
    ) -> Result<(uuid::Uuid, ChangeSet), String> {
        let mut candidate = self.project.clone();
        let definition = candidate
            .module_definitions
            .get_mut(&definition_id)
            .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
        if definition.role != ModuleRole::Effect {
            return Err("Only Effect Modules accept image-effect Nodes".to_string());
        }
        let crate::model::node::NodeContent::PluginOperation(operation) = node.content() else {
            return Err("Effect Module Nodes must be plugin operations".to_string());
        };
        if operation.category != crate::plugin::EFFECT_CATEGORY
            || operation.operation != crate::plugin::EFFECT_APPLY_OPERATION
        {
            return Err("Node is not an image Effect operation".to_string());
        }
        let node_id = node.id;
        if definition.graph.nodes.contains_key(&node_id) {
            return Err(format!("Module Node {node_id} already exists"));
        }
        let previous = definition
            .graph
            .nodes
            .keys()
            .copied()
            .filter(|node_id| {
                !definition
                    .graph
                    .connections
                    .iter()
                    .any(|connection| connection.from.node_id == *node_id)
            })
            .max();
        for (property_name, property) in node.properties().iter() {
            let port = format!("{}{}", crate::plugin::PROPERTY_PORT_PREFIX, property_name);
            let data_type = operation
                .declared_ports
                .iter()
                .find(|definition| definition.key == port)
                .map(|definition| definition.data_type)
                .unwrap_or(crate::model::project::PortDataType::Any);
            if let Some(default_value) = property.value().cloned() {
                definition.published_parameters.push(PublishedParameter {
                    id: PublishedParameterId::new(),
                    name: property_name.clone(),
                    data_type,
                    default_value,
                    target: super::ModulePortAddress { node_id, port },
                });
            }
        }
        definition.graph.nodes.insert(node_id, node);
        if let Some(previous) = previous {
            definition.graph.connections.push(ModuleConnection {
                id: ModuleConnectionId::new(),
                from: super::ModulePortAddress {
                    node_id: previous,
                    port: crate::model::project::IMAGE_OUTPUT_PORT.to_string(),
                },
                to: super::ModulePortAddress {
                    node_id,
                    port: crate::model::project::IMAGE_INPUT_PORT.to_string(),
                },
                order: 0,
            });
        }
        definition.version = definition
            .version
            .checked_add(1)
            .ok_or_else(|| "Module definition version overflow".to_string())?;
        candidate.validate()?;
        self.project = candidate;
        Ok((
            node_id,
            self.finish(vec![ProjectInvalidation::ModuleDefinition {
                definition_id,
            }]),
        ))
    }

    pub fn remove_module_node(
        &mut self,
        definition_id: ModuleDefinitionId,
        node_id: uuid::Uuid,
    ) -> Result<ChangeSet, String> {
        let mut candidate = self.project.clone();
        let definition = candidate
            .module_definitions
            .get_mut(&definition_id)
            .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
        definition
            .graph
            .nodes
            .remove(&node_id)
            .ok_or_else(|| format!("Missing Module Node {node_id}"))?;
        definition.graph.connections.retain(|connection| {
            connection.from.node_id != node_id && connection.to.node_id != node_id
        });
        let removed_parameters: std::collections::HashSet<_> = definition
            .published_parameters
            .iter()
            .filter(|parameter| parameter.target.node_id == node_id)
            .map(|parameter| parameter.id)
            .collect();
        definition
            .published_parameters
            .retain(|parameter| parameter.target.node_id != node_id);
        definition
            .published_signals
            .retain(|signal| signal.source.node_id != node_id);
        definition
            .published_actions
            .retain(|action| action.target.node_id != node_id);
        definition.version = definition
            .version
            .checked_add(1)
            .ok_or_else(|| "Module definition version overflow".to_string())?;
        for instance in candidate
            .module_instances
            .values_mut()
            .filter(|instance| instance.definition_id == definition_id)
        {
            instance
                .parameter_overrides
                .retain(|parameter_id, _| !removed_parameters.contains(parameter_id));
        }
        candidate.validate()?;
        self.project = candidate;
        Ok(self.finish(vec![ProjectInvalidation::ModuleDefinition {
            definition_id,
        }]))
    }

    pub fn connect_module_nodes(
        &mut self,
        definition_id: ModuleDefinitionId,
        from_node_id: uuid::Uuid,
        to_node_id: uuid::Uuid,
    ) -> Result<(ModuleConnectionId, ChangeSet), String> {
        let mut candidate = self.project.clone();
        let definition = candidate
            .module_definitions
            .get_mut(&definition_id)
            .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
        if !definition.graph.nodes.contains_key(&from_node_id)
            || !definition.graph.nodes.contains_key(&to_node_id)
        {
            return Err("Module connection endpoint is missing".to_string());
        }
        if definition.graph.connections.iter().any(|connection| {
            connection.from.node_id == from_node_id && connection.to.node_id == to_node_id
        }) {
            return Err("Module Nodes are already connected".to_string());
        }
        let id = ModuleConnectionId::new();
        definition.graph.connections.push(ModuleConnection {
            id,
            from: super::ModulePortAddress {
                node_id: from_node_id,
                port: crate::model::project::IMAGE_OUTPUT_PORT.to_string(),
            },
            to: super::ModulePortAddress {
                node_id: to_node_id,
                port: crate::model::project::IMAGE_INPUT_PORT.to_string(),
            },
            order: 0,
        });
        definition.version = definition
            .version
            .checked_add(1)
            .ok_or_else(|| "Module definition version overflow".to_string())?;
        candidate.validate()?;
        self.project = candidate;
        Ok((
            id,
            self.finish(vec![ProjectInvalidation::ModuleDefinition {
                definition_id,
            }]),
        ))
    }

    pub fn disconnect_module_connection(
        &mut self,
        definition_id: ModuleDefinitionId,
        connection_id: ModuleConnectionId,
    ) -> Result<ChangeSet, String> {
        let definition = self
            .project
            .module_definitions
            .get_mut(&definition_id)
            .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
        let before = definition.graph.connections.len();
        definition
            .graph
            .connections
            .retain(|connection| connection.id != connection_id);
        if definition.graph.connections.len() == before {
            return Err(format!("Missing Module connection {connection_id}"));
        }
        definition.version = definition
            .version
            .checked_add(1)
            .ok_or_else(|| "Module definition version overflow".to_string())?;
        Ok(self.finish(vec![ProjectInvalidation::ModuleDefinition {
            definition_id,
        }]))
    }

    pub fn add_signal_binding(&mut self, binding: SignalBinding) -> Result<ChangeSet, String> {
        if self.project.signal_bindings.contains_key(&binding.id) {
            return Err(format!("Signal Binding {} already exists", binding.id));
        }
        let mut candidate = self.project.clone();
        candidate
            .signal_bindings
            .insert(binding.id, binding.clone());
        candidate.validate()?;
        self.project.signal_bindings.insert(binding.id, binding);
        Ok(self.finish(vec![ProjectInvalidation::TimelineStructure {
            timeline_id: self.project.root_timeline_id,
        }]))
    }

    pub fn remove_signal_binding(
        &mut self,
        binding_id: SignalBindingId,
    ) -> Result<ChangeSet, String> {
        self.project
            .signal_bindings
            .remove(&binding_id)
            .ok_or_else(|| format!("Missing Signal Binding {binding_id}"))?;
        Ok(self.finish(vec![ProjectInvalidation::TimelineStructure {
            timeline_id: self.project.root_timeline_id,
        }]))
    }

    pub fn add_event_binding(&mut self, binding: EventBinding) -> Result<ChangeSet, String> {
        if self.project.event_bindings.contains_key(&binding.id) {
            return Err(format!("Event Binding {} already exists", binding.id));
        }
        let mut candidate = self.project.clone();
        candidate.event_bindings.insert(binding.id, binding.clone());
        candidate.validate()?;
        self.project.event_bindings.insert(binding.id, binding);
        Ok(self.finish(vec![ProjectInvalidation::TimelineStructure {
            timeline_id: self.project.root_timeline_id,
        }]))
    }

    pub fn remove_event_binding(
        &mut self,
        binding_id: EventBindingId,
    ) -> Result<ChangeSet, String> {
        self.project
            .event_bindings
            .remove(&binding_id)
            .ok_or_else(|| format!("Missing Event Binding {binding_id}"))?;
        Ok(self.finish(vec![ProjectInvalidation::TimelineStructure {
            timeline_id: self.project.root_timeline_id,
        }]))
    }

    pub fn set_parent(
        &mut self,
        item_id: TimelineItemId,
        parent: Option<TimelineItemId>,
    ) -> Result<ChangeSet, String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        if let Some(parent_id) = parent {
            if parent_id == item_id || self.timeline_for_item(parent_id)? != timeline_id {
                return Err("Parent must be another item in the same Timeline".to_string());
            }
            let mut current = Some(parent_id);
            while let Some(id) = current {
                if id == item_id {
                    return Err("Timeline parent assignment would create a cycle".to_string());
                }
                current = self.project.items.get(&id).and_then(|item| item.parent);
            }
        }
        self.project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .parent = parent;
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn set_composition_duration_policy(
        &mut self,
        item_id: TimelineItemId,
        policy: crate::model::authoring::DurationPolicy,
    ) -> Result<ChangeSet, String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        let mut candidate = self.project.clone();
        let item = candidate
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        let SourceRef::Composition(instance) = &mut item.source else {
            return Err("Selected Timeline item is not a nested Composition".to_string());
        };
        instance.duration_policy = policy;
        candidate.validate()?;
        self.project = candidate;
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn add_cross_dissolve(
        &mut self,
        to_item_id: TimelineItemId,
        duration: f64,
    ) -> Result<ChangeSet, String> {
        if !duration.is_finite() || duration <= 0.0 {
            return Err("Cross dissolve duration must be positive".to_string());
        }
        let mut candidate = self.project.clone();
        let to_item = candidate
            .items
            .get(&to_item_id)
            .cloned()
            .ok_or_else(|| format!("Missing Timeline item {to_item_id}"))?;
        let from_item = candidate
            .items
            .values()
            .filter(|item| {
                item.track_id == to_item.track_id
                    && item.id != to_item.id
                    && item.interval.start < to_item.interval.start
            })
            .max_by_key(|item| item.interval.start)
            .cloned()
            .ok_or_else(|| {
                "Cross dissolve requires a previous item on the same Track".to_string()
            })?;
        if from_item.transition_out.is_some() || to_item.transition_in.is_some() {
            return Err("One of the Timeline items already has a transition".to_string());
        }
        let duration = duration
            .min(from_item.interval.duration.into_inner())
            .min(to_item.interval.duration.into_inner());
        let overlap_start = (from_item.interval.start.into_inner()
            + from_item.interval.duration.into_inner()
            - duration)
            .max(0.0);
        candidate.items.get_mut(&to_item_id).unwrap().interval.start = OrderedFloat(overlap_start);
        let id = TransitionId::new();
        candidate.transitions.insert(
            id,
            Transition {
                id,
                from_item_id: from_item.id,
                to_item_id,
                duration: OrderedFloat(duration),
                kind: TransitionKind::CrossDissolve,
                authored_properties: PropertyMap::new(),
            },
        );
        candidate
            .items
            .get_mut(&from_item.id)
            .unwrap()
            .transition_out = Some(id);
        candidate.items.get_mut(&to_item_id).unwrap().transition_in = Some(id);
        candidate.validate()?;
        self.project = candidate;
        let timeline_id = self.timeline_for_track(to_item.track_id)?;
        Ok(self.finish(vec![ProjectInvalidation::TimelineStructure { timeline_id }]))
    }

    pub fn add_constraint(
        &mut self,
        item_id: TimelineItemId,
        target_item_id: TimelineItemId,
        kind: ConstraintKind,
    ) -> Result<ChangeSet, String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        let mut candidate = self.project.clone();
        let id = ConstraintId::new();
        let item_duration = candidate
            .items
            .get(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .interval
            .duration
            .into_inner();
        if kind == ConstraintKind::FollowPath {
            let target = candidate
                .items
                .get(&target_item_id)
                .ok_or_else(|| format!("Missing Timeline item {target_item_id}"))?;
            if !matches!(
                &target.source,
                SourceRef::Shape { shape }
                    if shape.shape_kind == super::ShapeKind::Path
                        && matches!(shape.parameters.get("path"), Some(crate::model::project::property::PropertyValue::Path(_)))
            ) {
                return Err("Follow Path requires a Path Shape target".to_string());
            }
        }
        let mut parameters = PropertyMap::new();
        if kind == ConstraintKind::FollowPath {
            let number =
                |value| crate::model::project::property::PropertyValue::Number(OrderedFloat(value));
            parameters.set(
                "progress".to_string(),
                Property::keyframe(vec![
                    crate::model::project::property::Keyframe::new(
                        0.0,
                        number(0.0),
                        crate::model::animation::EasingFunction::Linear,
                    ),
                    crate::model::project::property::Keyframe::new(
                        item_duration,
                        number(1.0),
                        crate::model::animation::EasingFunction::Linear,
                    ),
                ]),
            );
            parameters.set(
                "auto_orient".to_string(),
                Property::constant(crate::model::project::property::PropertyValue::Boolean(
                    false,
                )),
            );
        }
        candidate
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .constraints
            .push(Constraint {
                id,
                target_item_id,
                kind,
                influence: Property::constant(
                    crate::model::project::property::PropertyValue::Number(OrderedFloat(1.0)),
                ),
                parameters,
            });
        candidate.validate()?;
        self.project = candidate;
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn add_mask(
        &mut self,
        item_id: TimelineItemId,
        path: crate::model::path::PathValue,
        mode: MaskMode,
    ) -> Result<(MaskId, ChangeSet), String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        let mut candidate = self.project.clone();
        let id = MaskId::new();
        candidate.masks.insert(
            id,
            Mask {
                id,
                path,
                mode,
                inverted: false,
                feather: Property::constant(
                    crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
                ),
                opacity: Property::constant(
                    crate::model::project::property::PropertyValue::Number(OrderedFloat(1.0)),
                ),
            },
        );
        candidate
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .mask_ids
            .push(id);
        candidate.validate()?;
        self.project = candidate;
        Ok((
            id,
            self.finish(vec![ProjectInvalidation::ItemProperties {
                timeline_id,
                item_id,
            }]),
        ))
    }

    pub fn update_mask(
        &mut self,
        item_id: TimelineItemId,
        mask_id: MaskId,
        _time: f64,
        mode: MaskMode,
        inverted: bool,
        feather: f64,
        opacity: f64,
    ) -> Result<ChangeSet, String> {
        if !feather.is_finite() || !opacity.is_finite() {
            return Err("Mask controls must be finite".to_string());
        }
        let timeline_id = self.timeline_for_item(item_id)?;
        let mut candidate = self.project.clone();
        let item = candidate
            .items
            .get(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        if !item.mask_ids.contains(&mask_id) {
            return Err(format!(
                "Mask {mask_id} is not owned by Timeline item {item_id}"
            ));
        }
        let mask = candidate
            .masks
            .get_mut(&mask_id)
            .ok_or_else(|| format!("Missing Mask {mask_id}"))?;
        mask.mode = mode;
        mask.inverted = inverted;
        mask.feather = Property::constant(crate::model::project::property::PropertyValue::Number(
            OrderedFloat(feather.max(0.0)),
        ));
        mask.opacity = Property::constant(crate::model::project::property::PropertyValue::Number(
            OrderedFloat(opacity.clamp(0.0, 1.0)),
        ));
        candidate.validate()?;
        self.project = candidate;
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn set_matte(
        &mut self,
        item_id: TimelineItemId,
        matte: Option<MatteRef>,
    ) -> Result<ChangeSet, String> {
        let timeline_id = self.timeline_for_item(item_id)?;
        let mut candidate = self.project.clone();
        candidate
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .matte = matte;
        candidate.validate()?;
        self.project = candidate;
        Ok(self.finish(vec![ProjectInvalidation::ItemProperties {
            timeline_id,
            item_id,
        }]))
    }

    pub fn replace_data_source_generation(
        &mut self,
        track_id: TimelineTrackId,
        data_source: DataSource,
        definition: Option<ModuleDefinition>,
        instance: Option<ModuleInstance>,
        generated: Vec<GeneratedItem>,
    ) -> Result<ChangeSet, String> {
        let timeline_id = self.timeline_for_track(track_id)?;
        let mut candidate = self.project.clone();
        if let Some(definition) = definition {
            candidate
                .module_definitions
                .insert(definition.id, definition);
        }
        if let Some(instance) = instance {
            candidate.module_instances.insert(instance.id, instance);
        }
        let generator_id = data_source.generator_id;
        candidate.data_sources.insert(data_source.id, data_source);
        crate::core::generator_runtime::reconcile_and_materialize(
            &mut candidate,
            track_id,
            generator_id,
            generated,
        )?;
        self.project = candidate;
        Ok(self.finish(vec![ProjectInvalidation::TimelineStructure { timeline_id }]))
    }

    pub fn remove_generated_override(
        &mut self,
        override_id: OverrideId,
    ) -> Result<ChangeSet, String> {
        let mut candidate = self.project.clone();
        let authored_override = candidate
            .overrides
            .remove(&override_id)
            .ok_or_else(|| format!("Missing generated Override {override_id}"))?;
        let mut timeline_id = candidate.root_timeline_id;
        if let Some(generated) = candidate
            .generated_items
            .get(&authored_override.generated_item_id)
            .cloned()
        {
            let item_id = TimelineItemId::from_uuid(generated.stable_id.as_uuid());
            let track_id = candidate
                .items
                .get(&item_id)
                .map(|item| item.track_id)
                .ok_or_else(|| {
                    format!("Generated item {} is not materialized", generated.stable_id)
                })?;
            timeline_id = candidate.tracks[&track_id].timeline_id;
            let regenerated = candidate
                .generated_items
                .values()
                .filter(|item| item.generator_id == generated.generator_id)
                .cloned()
                .collect();
            crate::core::generator_runtime::reconcile_and_materialize(
                &mut candidate,
                track_id,
                generated.generator_id,
                regenerated,
            )?;
        } else {
            candidate.validate()?;
        }
        self.project = candidate;
        Ok(self.finish(vec![ProjectInvalidation::TimelineStructure { timeline_id }]))
    }

    fn record_generated_override(
        &mut self,
        generated_item_id: crate::model::authoring::GeneratedItemId,
        path: OverridePath,
        value: crate::model::project::property::PropertyValue,
    ) {
        if let Some(authored_override) = self
            .project
            .overrides
            .values_mut()
            .find(|authored_override| authored_override.generated_item_id == generated_item_id)
        {
            if let Some(patch) = authored_override
                .patch
                .iter_mut()
                .find(|patch| patch.path == path)
            {
                patch.operator = OverrideOperator::Replace;
                patch.value = value;
            } else {
                authored_override.patch.push(OverridePatch {
                    path,
                    operator: OverrideOperator::Replace,
                    value,
                });
            }
            authored_override.status = OverrideStatus::Active;
            return;
        }
        let id = OverrideId::new();
        self.project.overrides.insert(
            id,
            Override {
                id,
                generated_item_id,
                patch: vec![OverridePatch {
                    path,
                    operator: OverrideOperator::Replace,
                    value,
                }],
                status: OverrideStatus::Active,
            },
        );
    }

    fn timeline_for_track(&self, track_id: TimelineTrackId) -> Result<TimelineId, String> {
        self.project
            .tracks
            .get(&track_id)
            .map(|track| track.timeline_id)
            .ok_or_else(|| format!("Missing Timeline Track {track_id}"))
    }

    fn timeline_for_item(&self, item_id: TimelineItemId) -> Result<TimelineId, String> {
        let track_id = self
            .project
            .items
            .get(&item_id)
            .map(|item| item.track_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
        self.timeline_for_track(track_id)
    }

    fn clone_masks(&mut self, mask_ids: &[MaskId]) -> Vec<MaskId> {
        let masks: Vec<_> = mask_ids
            .iter()
            .filter_map(|mask_id| self.project.masks.get(mask_id).cloned())
            .collect();
        masks
            .into_iter()
            .map(|mut mask| {
                let id = MaskId::new();
                mask.id = id;
                self.project.masks.insert(id, mask);
                id
            })
            .collect()
    }

    fn clone_item_attachments(
        &mut self,
        source_item_id: TimelineItemId,
        target_item_id: TimelineItemId,
    ) -> Result<(), String> {
        let attachments: Vec<_> = self
            .project
            .attachments
            .values()
            .filter(|attachment| {
                attachment.owner
                    == (AttachmentOwner::Item {
                        item_id: source_item_id,
                    })
            })
            .cloned()
            .collect();
        for mut attachment in attachments {
            let instance = self
                .project
                .module_instances
                .get(&attachment.module_instance_id)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "Attachment {} refers to a missing Module instance",
                        attachment.id
                    )
                })?;
            let mut cloned_instance = instance;
            let instance_id = ModuleInstanceId::new();
            cloned_instance.id = instance_id;
            self.project
                .module_instances
                .insert(instance_id, cloned_instance);
            attachment.id = AttachmentId::new();
            attachment.owner = AttachmentOwner::Item {
                item_id: target_item_id,
            };
            attachment.module_instance_id = instance_id;
            self.project.attachments.insert(attachment.id, attachment);
        }
        Ok(())
    }

    fn finish(&mut self, invalidations: Vec<ProjectInvalidation>) -> ChangeSet {
        self.revision = ProjectRevision(self.revision.0.wrapping_add(1));
        ChangeSet {
            revision: self.revision,
            invalidations,
        }
    }
}

fn shift_source_start(source: &mut SourceRef, timeline_delta: f64) -> Result<(), String> {
    let time_map = match source {
        SourceRef::Asset { time_map, .. } => Some(time_map),
        SourceRef::Composition(instance) => Some(&mut instance.time_map),
        SourceRef::Text { .. }
        | SourceRef::Shape { .. }
        | SourceRef::Solid { .. }
        | SourceRef::Module { .. } => None,
    };
    let Some(time_map) = time_map else {
        return Ok(());
    };
    let source_start =
        time_map.source_start.into_inner() + timeline_delta * time_map.playback_rate.into_inner();
    if !source_start.is_finite() || source_start < 0.0 {
        return Err("Trim would move source time before zero".to_string());
    }
    time_map.source_start = OrderedFloat(source_start);
    Ok(())
}

fn range_invalidation(timeline_id: TimelineId, interval: TimelineInterval) -> ProjectInvalidation {
    ProjectInvalidation::TimelineRange {
        timeline_id,
        start: interval.start,
        duration: interval.duration,
    }
}

fn validate_timeline_settings(
    width: u64,
    height: u64,
    fps: f64,
    duration: f64,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("Timeline dimensions must be greater than zero".to_string());
    }
    if !fps.is_finite() || fps <= 0.0 {
        return Err("Timeline FPS must be finite and greater than zero".to_string());
    }
    if !duration.is_finite() || duration < 0.0 {
        return Err("Timeline duration must be finite and non-negative".to_string());
    }
    Ok(())
}
