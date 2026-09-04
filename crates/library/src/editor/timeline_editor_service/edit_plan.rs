use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use super::*;

/// One exact-time Timeline placement edit requested against an immutable
/// Project revision.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimelineEditRequest {
    pub base_revision: ProjectRevision,
    pub operation: TimelineEditOperation,
}

impl TimelineEditRequest {
    pub const fn move_item(
        base_revision: ProjectRevision,
        item_id: TimelineItemId,
        track_id: TimelineTrackId,
        start: MediaTime,
        layer: i64,
    ) -> Self {
        Self {
            base_revision,
            operation: TimelineEditOperation::MoveItem {
                item_id,
                track_id,
                start,
                layer,
            },
        }
    }

    pub const fn trim_item(
        base_revision: ProjectRevision,
        item_id: TimelineItemId,
        interval: TimelineInterval,
    ) -> Self {
        Self {
            base_revision,
            operation: TimelineEditOperation::TrimItem { item_id, interval },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimelineEditOperation {
    MoveItem {
        item_id: TimelineItemId,
        track_id: TimelineTrackId,
        start: MediaTime,
        layer: i64,
    },
    TrimItem {
        item_id: TimelineItemId,
        interval: TimelineInterval,
    },
}

/// Public editor name for the authoring model's non-persisted placement view.
pub type TimelineItemEditState = crate::model::authoring::TimelineItemPlacementState;
pub type EditPlanValidationScope = crate::model::authoring::TimelineEditValidationScope;

/// Pure, non-persisted result of planning a Timeline edit.
///
/// `expected_items` records every placement read while planning. The public
/// projection contains only changed placements, while commit checks the full
/// read set before touching the authoritative Project.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EditPlan {
    request: TimelineEditRequest,
    expected_items: BTreeMap<TimelineItemId, TimelineItemEditState>,
    expected_tracks: BTreeMap<TimelineTrackId, TimelineId>,
    projected_items: BTreeMap<TimelineItemId, TimelineItemEditState>,
    validation_scope: EditPlanValidationScope,
}

impl EditPlan {
    pub const fn request(&self) -> TimelineEditRequest {
        self.request
    }

    pub fn is_noop(&self) -> bool {
        self.projected_items.is_empty()
    }

    pub fn changed_item_count(&self) -> usize {
        self.projected_items.len()
    }

    pub const fn validation_scope(&self) -> EditPlanValidationScope {
        self.validation_scope
    }
}

/// Sparse preview state produced from an [`EditPlan`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EditProjection {
    base_revision: ProjectRevision,
    items: BTreeMap<TimelineItemId, TimelineItemEditState>,
}

impl EditProjection {
    pub const fn base_revision(&self) -> ProjectRevision {
        self.base_revision
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn item(&self, item_id: TimelineItemId) -> Option<&TimelineItemEditState> {
        self.items.get(&item_id)
    }

    pub fn items(
        &self,
    ) -> impl ExactSizeIterator<Item = (TimelineItemId, &TimelineItemEditState)> + '_ {
        self.items.iter().map(|(item_id, state)| (*item_id, state))
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TimelineEditError {
    StaleRevision {
        base_revision: ProjectRevision,
        current_revision: ProjectRevision,
    },
    MissingItem(TimelineItemId),
    MissingTrack(TimelineTrackId),
    ExpectedItemChanged(TimelineItemId),
    ExpectedTrackChanged(TimelineTrackId),
    InvalidRequest(String),
    CommitRejected(String),
    SessionUnavailable(String),
}

impl Display for TimelineEditError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleRevision {
                base_revision,
                current_revision,
            } => write!(
                formatter,
                "Timeline edit revision {} is stale; current revision is {}",
                base_revision.get(),
                current_revision.get()
            ),
            Self::MissingItem(item_id) => write!(formatter, "Missing Timeline item {item_id}"),
            Self::MissingTrack(track_id) => write!(formatter, "Missing Timeline Track {track_id}"),
            Self::ExpectedItemChanged(item_id) => write!(
                formatter,
                "Timeline item {item_id} no longer matches the state used to build the edit plan"
            ),
            Self::ExpectedTrackChanged(track_id) => write!(
                formatter,
                "Timeline Track {track_id} no longer matches the state used to build the edit plan"
            ),
            Self::InvalidRequest(message) => formatter.write_str(message),
            Self::CommitRejected(message) => {
                write!(formatter, "Timeline edit was rejected: {message}")
            }
            Self::SessionUnavailable(message) => formatter.write_str(message),
        }
    }
}

impl Error for TimelineEditError {}

impl From<TimelineEditError> for LibraryError {
    fn from(error: TimelineEditError) -> Self {
        match error {
            TimelineEditError::SessionUnavailable(message) => LibraryError::Runtime(message),
            other => LibraryError::Validation(other.to_string()),
        }
    }
}

/// Builds an edit plan without cloning or mutating `project`.
pub fn plan_timeline_edit(
    project: &AuthoringProject,
    current_revision: ProjectRevision,
    index: &TimelineEditPlanningIndex,
    request: TimelineEditRequest,
) -> Result<EditPlan, TimelineEditError> {
    require_revision(request.base_revision, current_revision)?;
    require_index_revision(index, current_revision)?;
    let mut plan = match request.operation {
        TimelineEditOperation::MoveItem {
            item_id,
            track_id,
            start,
            layer,
        } => plan_move(project, index, request, item_id, track_id, start, layer),
        TimelineEditOperation::TrimItem { item_id, interval } => {
            plan_trim(project, request, item_id, interval)
        }
    }?;
    plan.validation_scope = validate_projected_state(project, index, &plan)?;
    Ok(plan)
}

/// Verifies the plan's exact read set and returns only states that a preview
/// needs to overlay on its immutable Project snapshot.
pub fn project_edit_plan(
    project: &AuthoringProject,
    current_revision: ProjectRevision,
    index: &TimelineEditPlanningIndex,
    plan: &EditPlan,
) -> Result<EditProjection, TimelineEditError> {
    require_revision(plan.request.base_revision, current_revision)?;
    require_index_revision(index, current_revision)?;
    verify_expected_state(project, plan)?;
    validate_projected_state(project, index, plan)?;
    Ok(EditProjection {
        base_revision: plan.request.base_revision,
        items: plan.projected_items.clone(),
    })
}

impl TimelineEditorService {
    /// Returns the immutable dependency index shared by every plan built from
    /// the current revision, including repeated pointer-move previews.
    pub fn timeline_edit_planning_index(
        &self,
    ) -> Result<Arc<TimelineEditPlanningIndex>, TimelineEditError> {
        let session = self
            .read_session()
            .map_err(|error| TimelineEditError::SessionUnavailable(error.to_string()))?;
        self.planning_index_for(session.project(), session.revision())
    }

    pub fn plan_timeline_edit(
        &self,
        request: TimelineEditRequest,
    ) -> Result<EditPlan, TimelineEditError> {
        let session = self
            .read_session()
            .map_err(|error| TimelineEditError::SessionUnavailable(error.to_string()))?;
        require_revision(request.base_revision, session.revision())?;
        let index = self.planning_index_for(session.project(), session.revision())?;
        plan_timeline_edit(session.project(), session.revision(), &index, request)
    }

    pub fn project_edit_plan(&self, plan: &EditPlan) -> Result<EditProjection, TimelineEditError> {
        let session = self
            .read_session()
            .map_err(|error| TimelineEditError::SessionUnavailable(error.to_string()))?;
        require_revision(plan.request.base_revision, session.revision())?;
        let index = self.planning_index_for(session.project(), session.revision())?;
        project_edit_plan(session.project(), session.revision(), &index, plan)
    }

    /// Atomically commits the exact states shown by [`Self::project_edit_plan`].
    ///
    /// The authoritative Project is cloned only by `AuthoringSession::transact`
    /// and only after revision and expected-before checks have succeeded.
    pub fn commit_edit_plan(&self, plan: &EditPlan) -> Result<ChangeSet, TimelineEditError> {
        let mut session = self
            .write_session()
            .map_err(|error| TimelineEditError::SessionUnavailable(error.to_string()))?;
        require_revision(plan.request.base_revision, session.revision())?;
        let index = self.planning_index_for(session.project(), session.revision())?;
        let projection = project_edit_plan(session.project(), session.revision(), &index, plan)?;
        if projection.is_empty() {
            return Ok(ChangeSet {
                revision: session.revision(),
                invalidations: Vec::new(),
            });
        }
        let invalidations = affected_range_invalidations(session.project(), &projection)?;
        session
            .transact(invalidations, |project| {
                for (item_id, state) in projection.items() {
                    let item = project
                        .items
                        .get_mut(&item_id)
                        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
                    state.apply_to(item);
                }
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(TimelineEditError::CommitRejected)
    }

    pub(super) fn plan_current_timeline_edit(
        &self,
        operation: TimelineEditOperation,
    ) -> Result<EditPlan, TimelineEditError> {
        let session = self
            .read_session()
            .map_err(|error| TimelineEditError::SessionUnavailable(error.to_string()))?;
        let request = TimelineEditRequest {
            base_revision: session.revision(),
            operation,
        };
        let index = self.planning_index_for(session.project(), session.revision())?;
        plan_timeline_edit(session.project(), session.revision(), &index, request)
    }

    fn planning_index_for(
        &self,
        project: &AuthoringProject,
        revision: ProjectRevision,
    ) -> Result<Arc<TimelineEditPlanningIndex>, TimelineEditError> {
        if let Some(index) = self
            .timeline_edit_index
            .read()
            .map_err(|_| {
                TimelineEditError::SessionUnavailable(
                    "Timeline edit planning index lock poisoned".to_string(),
                )
            })?
            .as_ref()
            .filter(|index| index.revision() == revision)
        {
            return Ok(Arc::clone(index));
        }
        let built = Arc::new(
            TimelineEditPlanningIndex::build(project, revision)
                .map_err(TimelineEditError::InvalidRequest)?,
        );
        let mut cached = self.timeline_edit_index.write().map_err(|_| {
            TimelineEditError::SessionUnavailable(
                "Timeline edit planning index lock poisoned".to_string(),
            )
        })?;
        if let Some(index) = cached.as_ref().filter(|index| index.revision() == revision) {
            return Ok(Arc::clone(index));
        }
        *cached = Some(Arc::clone(&built));
        Ok(built)
    }
}

fn plan_move(
    project: &AuthoringProject,
    index: &TimelineEditPlanningIndex,
    request: TimelineEditRequest,
    item_id: TimelineItemId,
    track_id: TimelineTrackId,
    start: MediaTime,
    requested_layer: i64,
) -> Result<EditPlan, TimelineEditError> {
    let item = project
        .items
        .get(&item_id)
        .ok_or(TimelineEditError::MissingItem(item_id))?;
    let old_track_id = item.track_id;
    let mut expected_tracks = BTreeMap::new();
    expected_tracks.insert(old_track_id, timeline_id_for_track(project, old_track_id)?);
    expected_tracks.insert(track_id, timeline_id_for_track(project, track_id)?);
    TimelineInterval::new(start, item.interval.duration)
        .map_err(TimelineEditError::InvalidRequest)?;

    let target_items = index.track_items(track_id);
    let destination_base_len = target_items
        .len()
        .saturating_sub(usize::from(old_track_id == track_id));
    let destination_index = usize::try_from(requested_layer.max(0))
        .unwrap_or(usize::MAX)
        .min(destination_base_len);
    let current_index = index
        .track_position(item_id)
        .ok_or(TimelineEditError::MissingItem(item_id))?;
    let reorders_layers = old_track_id != track_id
        || !index.track_layers_are_canonical(track_id)
        || current_index != destination_index;

    let mut relevant_ids = BTreeSet::from([item_id]);
    if reorders_layers {
        relevant_ids.extend(index.track_items(old_track_id).iter().copied());
        relevant_ids.extend(target_items.iter().copied());
    }
    let expected_items = item_states(project, relevant_ids)?;
    let mut desired_items = expected_items.clone();
    let moving = desired_items
        .get_mut(&item_id)
        .ok_or(TimelineEditError::MissingItem(item_id))?;
    moving.track_id = track_id;
    moving.interval.start = start;

    if reorders_layers {
        let mut destination_order = target_items
            .iter()
            .copied()
            .filter(|candidate| *candidate != item_id)
            .collect::<Vec<_>>();
        destination_order.insert(destination_index, item_id);
        assign_layers(&mut desired_items, destination_order)?;
        if old_track_id != track_id {
            assign_layers(
                &mut desired_items,
                index
                    .track_items(old_track_id)
                    .iter()
                    .copied()
                    .filter(|candidate| *candidate != item_id)
                    .collect(),
            )?;
        }
    }
    Ok(finish_plan(
        request,
        expected_items,
        expected_tracks,
        desired_items,
    ))
}

fn plan_trim(
    project: &AuthoringProject,
    request: TimelineEditRequest,
    item_id: TimelineItemId,
    interval: TimelineInterval,
) -> Result<EditPlan, TimelineEditError> {
    TimelineInterval::new(interval.start, interval.duration)
        .map_err(TimelineEditError::InvalidRequest)?;
    let item = project
        .items
        .get(&item_id)
        .ok_or(TimelineEditError::MissingItem(item_id))?;
    let before = TimelineItemEditState::from(item);
    let mut after = before;
    after.time_map.source_start = before
        .time_map
        .local_time(before.interval, interval.start)
        .map_err(TimelineEditError::InvalidRequest)?;
    after.interval = interval;
    let expected_items = BTreeMap::from([(item_id, before)]);
    let desired_items = BTreeMap::from([(item_id, after)]);
    let expected_tracks = BTreeMap::from([(
        before.track_id,
        timeline_id_for_track(project, before.track_id)?,
    )]);
    Ok(finish_plan(
        request,
        expected_items,
        expected_tracks,
        desired_items,
    ))
}

fn finish_plan(
    request: TimelineEditRequest,
    expected_items: BTreeMap<TimelineItemId, TimelineItemEditState>,
    expected_tracks: BTreeMap<TimelineTrackId, TimelineId>,
    desired_items: BTreeMap<TimelineItemId, TimelineItemEditState>,
) -> EditPlan {
    let projected_items = desired_items
        .into_iter()
        .filter(|(item_id, after)| expected_items.get(item_id) != Some(after))
        .collect();
    EditPlan {
        request,
        expected_items,
        expected_tracks,
        projected_items,
        validation_scope: EditPlanValidationScope::default(),
    }
}

fn item_states(
    project: &AuthoringProject,
    item_ids: BTreeSet<TimelineItemId>,
) -> Result<BTreeMap<TimelineItemId, TimelineItemEditState>, TimelineEditError> {
    item_ids
        .into_iter()
        .map(|item_id| {
            project
                .items
                .get(&item_id)
                .map(|item| (item_id, TimelineItemEditState::from(item)))
                .ok_or(TimelineEditError::MissingItem(item_id))
        })
        .collect()
}

fn assign_layers(
    states: &mut BTreeMap<TimelineItemId, TimelineItemEditState>,
    item_ids: Vec<TimelineItemId>,
) -> Result<(), TimelineEditError> {
    for (layer, item_id) in item_ids.into_iter().enumerate() {
        states
            .get_mut(&item_id)
            .ok_or(TimelineEditError::MissingItem(item_id))?
            .layer = i64::try_from(layer).map_err(|_| {
            TimelineEditError::InvalidRequest("Timeline layer overflow".to_string())
        })?;
    }
    Ok(())
}

fn verify_expected_state(
    project: &AuthoringProject,
    plan: &EditPlan,
) -> Result<(), TimelineEditError> {
    for (track_id, expected_timeline_id) in &plan.expected_tracks {
        let current_timeline_id = timeline_id_for_track(project, *track_id)?;
        if current_timeline_id != *expected_timeline_id {
            return Err(TimelineEditError::ExpectedTrackChanged(*track_id));
        }
    }
    for (item_id, expected) in &plan.expected_items {
        let item = project
            .items
            .get(item_id)
            .ok_or(TimelineEditError::MissingItem(*item_id))?;
        if TimelineItemEditState::from(item) != *expected {
            return Err(TimelineEditError::ExpectedItemChanged(*item_id));
        }
    }
    Ok(())
}

fn validate_projected_state(
    project: &AuthoringProject,
    index: &TimelineEditPlanningIndex,
    plan: &EditPlan,
) -> Result<EditPlanValidationScope, TimelineEditError> {
    project
        .validate_timeline_item_placement_overlay(index, &plan.projected_items)
        .map_err(TimelineEditError::InvalidRequest)
}

fn affected_range_invalidations(
    project: &AuthoringProject,
    projection: &EditProjection,
) -> Result<Vec<ProjectInvalidation>, TimelineEditError> {
    let mut ranges = BTreeMap::<TimelineId, (MediaTime, MediaTime)>::new();
    for (item_id, after) in projection.items() {
        let before = project
            .items
            .get(&item_id)
            .map(TimelineItemEditState::from)
            .ok_or(TimelineEditError::MissingItem(item_id))?;
        include_interval(
            &mut ranges,
            timeline_id_for_track(project, before.track_id)?,
            before.interval,
        )?;
        include_interval(
            &mut ranges,
            timeline_id_for_track(project, after.track_id)?,
            after.interval,
        )?;
    }
    ranges
        .into_iter()
        .map(|(timeline_id, (start, end))| {
            Ok(ProjectInvalidation::TimelineRange {
                timeline_id,
                start,
                duration: end
                    .checked_sub(start)
                    .map_err(TimelineEditError::InvalidRequest)?,
            })
        })
        .collect()
}

fn include_interval(
    ranges: &mut BTreeMap<TimelineId, (MediaTime, MediaTime)>,
    timeline_id: TimelineId,
    interval: TimelineInterval,
) -> Result<(), TimelineEditError> {
    let end = interval.end().map_err(TimelineEditError::InvalidRequest)?;
    ranges
        .entry(timeline_id)
        .and_modify(|range| {
            range.0 = range.0.min(interval.start);
            range.1 = range.1.max(end);
        })
        .or_insert((interval.start, end));
    Ok(())
}

fn timeline_id_for_track(
    project: &AuthoringProject,
    track_id: TimelineTrackId,
) -> Result<TimelineId, TimelineEditError> {
    project
        .tracks
        .get(&track_id)
        .map(|track| track.timeline_id)
        .ok_or(TimelineEditError::MissingTrack(track_id))
}

fn require_revision(
    base_revision: ProjectRevision,
    current_revision: ProjectRevision,
) -> Result<(), TimelineEditError> {
    if base_revision == current_revision {
        Ok(())
    } else {
        Err(TimelineEditError::StaleRevision {
            base_revision,
            current_revision,
        })
    }
}

fn require_index_revision(
    index: &TimelineEditPlanningIndex,
    current_revision: ProjectRevision,
) -> Result<(), TimelineEditError> {
    if index.revision() == current_revision {
        Ok(())
    } else {
        Err(TimelineEditError::InvalidRequest(format!(
            "Timeline edit planning index revision {} does not match current revision {}",
            index.revision().get(),
            current_revision.get()
        )))
    }
}
