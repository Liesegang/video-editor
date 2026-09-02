use ordered_float::OrderedFloat;

use crate::model::project::property::{Property, PropertyMap};

use super::{
    AuthoringProject, SourceRef, TimelineId, TimelineInterval, TimelineItem, TimelineItemId,
    TimelineTrackId,
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
        self.project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .interval = interval;
        Ok(self.finish(vec![
            range_invalidation(timeline_id, old_interval),
            range_invalidation(timeline_id, interval),
        ]))
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

    fn finish(&mut self, invalidations: Vec<ProjectInvalidation>) -> ChangeSet {
        self.revision = ProjectRevision(self.revision.0.wrapping_add(1));
        ChangeSet {
            revision: self.revision,
            invalidations,
        }
    }
}

fn range_invalidation(timeline_id: TimelineId, interval: TimelineInterval) -> ProjectInvalidation {
    ProjectInvalidation::TimelineRange {
        timeline_id,
        start: interval.start,
        duration: interval.duration,
    }
}
