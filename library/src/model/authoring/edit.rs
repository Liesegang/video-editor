use ordered_float::OrderedFloat;

use crate::model::project::property::{Property, PropertyMap};

use super::{
    AttachmentId, AttachmentOwner, AuthoringProject, MaskId, ModuleInstanceId, SourceRef,
    TimelineId, TimelineInterval, TimelineItem, TimelineItemId, TimelineTrackId,
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
                authored_properties: PropertyMap::new(),
            },
        );
        Ok((
            item_id,
            self.finish(vec![ProjectInvalidation::TimelineStructure { timeline_id }]),
        ))
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

    fn timeline_for_track(&self, track_id: TimelineTrackId) -> Result<TimelineId, String> {
        self.project
            .tracks
            .get(&track_id)
            .map(|track| track.timeline_id)
            .ok_or_else(|| format!("Missing Timeline Track {track_id}"))
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
