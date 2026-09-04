use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::Excluded;

use super::super::{
    AttachmentId, AttachmentOwner, AttachmentProcessor, AuthoringProject, CompositionParameter,
    CompositionParameterId, InstanceLocator, MediaInputBinding, ModuleInvocation, ProjectRevision,
    SourceRef, TimelineId, TimelineInterval, TimelineItem, TimelineItemId,
    TimelineItemPlacementState, TimelineTrackId, Transition, TransitionId,
};
use super::validation::{
    validate_duration_policy, validate_transition, validate_transition_participant_conflict,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum InvocationOwner {
    Item(TimelineItemId),
    Attachment(AttachmentId),
    Transition(TransitionId),
}

type TimelineItemOrderKey = (i64, super::super::MediaTime, TimelineItemId);

#[derive(Clone, Debug, Default)]
pub(super) struct TimelineItemOrderIndex {
    ordered_items_by_track: BTreeMap<TimelineTrackId, Vec<TimelineItemId>>,
    track_position_by_item: BTreeMap<TimelineItemId, usize>,
    order_keys_by_track: BTreeMap<TimelineTrackId, BTreeSet<TimelineItemOrderKey>>,
    noncanonical_layer_tracks: BTreeSet<TimelineTrackId>,
}

impl TimelineItemOrderIndex {
    pub(super) fn build(project: &AuthoringProject, placements: &ItemPlacementOverlay<'_>) -> Self {
        let mut index = Self::default();
        for item in project.items.values() {
            let placement = placements.state(item);
            index
                .ordered_items_by_track
                .entry(placement.track_id)
                .or_default()
                .push(item.id);
            index
                .order_keys_by_track
                .entry(placement.track_id)
                .or_default()
                .insert(item_order_key(item.id, placement));
        }
        for (track_id, item_ids) in &mut index.ordered_items_by_track {
            item_ids.sort_by_key(|item_id| {
                item_order_key(*item_id, placements.state(&project.items[item_id]))
            });
            for (position, item_id) in item_ids.iter().copied().enumerate() {
                index.track_position_by_item.insert(item_id, position);
                if placements.state(&project.items[&item_id]).layer != position as i64 {
                    index.noncanonical_layer_tracks.insert(*track_id);
                }
            }
        }
        index
    }

    fn track_items(&self, track_id: TimelineTrackId) -> &[TimelineItemId] {
        self.ordered_items_by_track
            .get(&track_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn track_position(&self, item_id: TimelineItemId) -> Option<usize> {
        self.track_position_by_item.get(&item_id).copied()
    }

    fn track_layers_are_canonical(&self, track_id: TimelineTrackId) -> bool {
        !self.noncanonical_layer_tracks.contains(&track_id)
    }

    pub(super) fn participants_have_clear_layer_span(
        &self,
        project: &AuthoringProject,
        placements: &ItemPlacementOverlay<'_>,
        transition: &Transition,
    ) -> bool {
        let Some(from) = project.items.get(&transition.from_item_id) else {
            return false;
        };
        let Some(to) = project.items.get(&transition.to_item_id) else {
            return false;
        };
        if placements.state(from).track_id != placements.state(to).track_id {
            return false;
        }
        let (Some(from_position), Some(to_position)) = (
            self.track_position(transition.from_item_id),
            self.track_position(transition.to_item_id),
        ) else {
            return false;
        };
        if from_position == to_position {
            return false;
        }
        let Ok(transition_interval) = transition.interval() else {
            return false;
        };
        let track_items = self.track_items(placements.state(from).track_id);
        let (lower, upper) = if from_position < to_position {
            (from_position, to_position)
        } else {
            (to_position, from_position)
        };
        track_items[lower + 1..upper].iter().all(|item_id| {
            project.items.get(item_id).is_some_and(|item| {
                !intervals_overlap_or_are_invalid(
                    placements.state(item).interval,
                    transition_interval,
                )
            })
        })
    }

    fn participants_have_clear_layer_span_after_sparse_overlay(
        &self,
        project: &AuthoringProject,
        placements: &ItemPlacementOverlay<'_>,
        transition: &Transition,
    ) -> bool {
        let Some(from) = project.items.get(&transition.from_item_id) else {
            return false;
        };
        let Some(to) = project.items.get(&transition.to_item_id) else {
            return false;
        };
        let from_placement = placements.state(from);
        let to_placement = placements.state(to);
        if from_placement.track_id != to_placement.track_id {
            return false;
        }
        let track_id = from_placement.track_id;
        let from_key = item_order_key(from.id, from_placement);
        let to_key = item_order_key(to.id, to_placement);
        if from_key == to_key {
            return false;
        }
        let (lower, upper) = if from_key < to_key {
            (from_key, to_key)
        } else {
            (to_key, from_key)
        };
        let Ok(transition_interval) = transition.interval() else {
            return false;
        };
        if self
            .order_keys_by_track
            .get(&track_id)
            .into_iter()
            .flat_map(|keys| keys.range((Excluded(lower), Excluded(upper))))
            .any(|key| {
                key.2 != from.id
                    && key.2 != to.id
                    && !placements.is_replaced(key.2)
                    && project.items.get(&key.2).is_none_or(|item| {
                        intervals_overlap_or_are_invalid(
                            placements.state(item).interval,
                            transition_interval,
                        )
                    })
            })
        {
            return false;
        }
        !placements.replacements().any(|(item_id, placement)| {
            *item_id != from.id
                && *item_id != to.id
                && placement.track_id == track_id
                && lower < item_order_key(*item_id, *placement)
                && item_order_key(*item_id, *placement) < upper
                && intervals_overlap_or_are_invalid(placement.interval, transition_interval)
        })
    }
}

fn item_order_key(
    item_id: TimelineItemId,
    placement: TimelineItemPlacementState,
) -> TimelineItemOrderKey {
    (placement.layer, placement.interval.start, item_id)
}

fn intervals_overlap_or_are_invalid(left: TimelineInterval, right: TimelineInterval) -> bool {
    let (Ok(left_end), Ok(right_end)) = (left.end(), right.end()) else {
        return true;
    };
    left.start < right_end && right.start < left_end
}

/// Immutable reverse-dependency index reused for every pointer update based
/// on one Project revision.
#[derive(Clone, Debug)]
pub struct TimelineEditPlanningIndex {
    revision: ProjectRevision,
    indexed_item_count: usize,
    children_by_parent: BTreeMap<TimelineItemId, Vec<TimelineItemId>>,
    transitions_by_item: BTreeMap<TimelineItemId, Vec<TransitionId>>,
    transition_layer_span_dependents_by_item: BTreeMap<TimelineItemId, Vec<TransitionId>>,
    parameters_by_item: BTreeMap<TimelineItemId, Vec<(TimelineId, CompositionParameterId)>>,
    attachments_by_item: BTreeMap<TimelineItemId, Vec<AttachmentId>>,
    binding_dependents_by_source: BTreeMap<TimelineItemId, Vec<InvocationOwner>>,
    exact_path_dependents: BTreeMap<TimelineItemId, Vec<InvocationOwner>>,
    composition_items_by_timeline: BTreeMap<TimelineId, Vec<TimelineItemId>>,
    item_order: TimelineItemOrderIndex,
}

impl TimelineEditPlanningIndex {
    pub fn build(project: &AuthoringProject, revision: ProjectRevision) -> Result<Self, String> {
        let mut index = Self {
            revision,
            indexed_item_count: project.items.len(),
            children_by_parent: BTreeMap::new(),
            transitions_by_item: BTreeMap::new(),
            transition_layer_span_dependents_by_item: BTreeMap::new(),
            parameters_by_item: BTreeMap::new(),
            attachments_by_item: BTreeMap::new(),
            binding_dependents_by_source: BTreeMap::new(),
            exact_path_dependents: BTreeMap::new(),
            composition_items_by_timeline: BTreeMap::new(),
            item_order: TimelineItemOrderIndex::build(project, &ItemPlacementOverlay::empty()),
        };
        for item in project.items.values() {
            if let Some(parent_id) = item.parent {
                index
                    .children_by_parent
                    .entry(parent_id)
                    .or_default()
                    .push(item.id);
            }
            match &item.source {
                SourceRef::Module(invocation) => {
                    index.index_invocation(InvocationOwner::Item(item.id), invocation);
                }
                SourceRef::Composition(_) => {
                    let timeline_id = project
                        .tracks
                        .get(&item.track_id)
                        .ok_or_else(|| format!("Item {} has a missing Track", item.id))?
                        .timeline_id;
                    index
                        .composition_items_by_timeline
                        .entry(timeline_id)
                        .or_default()
                        .push(item.id);
                }
                _ => {}
            }
        }
        for transition in project.transitions.values() {
            index
                .transitions_by_item
                .entry(transition.from_item_id)
                .or_default()
                .push(transition.id);
            index
                .transitions_by_item
                .entry(transition.to_item_id)
                .or_default()
                .push(transition.id);
            let from_item = project.items.get(&transition.from_item_id);
            let to_item = project.items.get(&transition.to_item_id);
            let from_position = index.item_order.track_position(transition.from_item_id);
            let to_position = index.item_order.track_position(transition.to_item_id);
            if let (Some(from_item), Some(to_item), Some(from_position), Some(to_position)) =
                (from_item, to_item, from_position, to_position)
                && from_item.track_id == to_item.track_id
            {
                let (lower, upper) = if from_position < to_position {
                    (from_position, to_position)
                } else {
                    (to_position, from_position)
                };
                let layer_span_items =
                    index.item_order.track_items(from_item.track_id)[lower + 1..upper].to_vec();
                for item_id in layer_span_items {
                    index
                        .transition_layer_span_dependents_by_item
                        .entry(item_id)
                        .or_default()
                        .push(transition.id);
                }
            }
            if let Some(module) = transition.processor.module_processor() {
                index.index_bindings(
                    InvocationOwner::Transition(transition.id),
                    &module.input_bindings,
                );
            }
        }
        for timeline in project.timelines.values() {
            for parameter in &timeline.published_parameters {
                index
                    .parameters_by_item
                    .entry(parameter.target.item_id())
                    .or_default()
                    .push((timeline.id, parameter.id));
            }
        }
        for attachment in project.attachments.values() {
            if let AttachmentOwner::Item { item_id } = attachment.owner {
                index
                    .attachments_by_item
                    .entry(item_id)
                    .or_default()
                    .push(attachment.id);
            }
            if let AttachmentProcessor::Module(invocation) = &attachment.processor {
                index.index_invocation(InvocationOwner::Attachment(attachment.id), invocation);
            }
        }
        index.sort_and_deduplicate();
        Ok(index)
    }

    pub const fn revision(&self) -> ProjectRevision {
        self.revision
    }

    pub const fn indexed_item_count(&self) -> usize {
        self.indexed_item_count
    }

    pub(crate) fn track_items(&self, track_id: TimelineTrackId) -> &[TimelineItemId] {
        self.item_order.track_items(track_id)
    }

    pub(crate) fn track_position(&self, item_id: TimelineItemId) -> Option<usize> {
        self.item_order.track_position(item_id)
    }

    pub(crate) fn track_layers_are_canonical(&self, track_id: TimelineTrackId) -> bool {
        self.item_order.track_layers_are_canonical(track_id)
    }

    fn transition_participants_have_clear_layer_span(
        &self,
        project: &AuthoringProject,
        placements: &ItemPlacementOverlay<'_>,
        transition: &Transition,
    ) -> bool {
        self.item_order
            .participants_have_clear_layer_span_after_sparse_overlay(
                project, placements, transition,
            )
    }

    fn index_invocation(&mut self, owner: InvocationOwner, invocation: &ModuleInvocation) {
        self.index_bindings(owner, &invocation.input_bindings);
    }

    fn index_bindings(
        &mut self,
        owner: InvocationOwner,
        bindings: &std::collections::HashMap<
            super::super::PublishedMediaInputId,
            MediaInputBinding,
        >,
    ) {
        for binding in bindings.values() {
            let MediaInputBinding::TimelineItemOutput {
                locator, item_id, ..
            } = binding;
            self.binding_dependents_by_source
                .entry(*item_id)
                .or_default()
                .push(owner);
            if let InstanceLocator::Exact(path) = locator {
                for composition_item_id in &path.composition_items {
                    self.exact_path_dependents
                        .entry(*composition_item_id)
                        .or_default()
                        .push(owner);
                }
            }
        }
    }

    fn sort_and_deduplicate(&mut self) {
        for values in self.children_by_parent.values_mut() {
            values.sort_unstable();
            values.dedup();
        }
        for values in self.transitions_by_item.values_mut() {
            values.sort_unstable();
            values.dedup();
        }
        for values in self.transition_layer_span_dependents_by_item.values_mut() {
            values.sort_unstable();
            values.dedup();
        }
        for values in self.parameters_by_item.values_mut() {
            values.sort_unstable();
            values.dedup();
        }
        for values in self.attachments_by_item.values_mut() {
            values.sort_unstable();
            values.dedup();
        }
        for values in self.binding_dependents_by_source.values_mut() {
            values.sort_unstable();
            values.dedup();
        }
        for values in self.exact_path_dependents.values_mut() {
            values.sort_unstable();
            values.dedup();
        }
        for values in self.composition_items_by_timeline.values_mut() {
            values.sort_unstable();
            values.dedup();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TimelineEditValidationScope {
    pub items: usize,
    pub transitions: usize,
    pub attachments: usize,
    pub composition_parameters: usize,
    pub moved_compositions: usize,
}

/// Read-only Project view with sparse placement replacements.
pub(super) struct ItemPlacementOverlay<'a> {
    replacements: Option<&'a BTreeMap<TimelineItemId, TimelineItemPlacementState>>,
}

impl ItemPlacementOverlay<'_> {
    pub(super) const fn empty() -> Self {
        Self { replacements: None }
    }

    pub(super) fn state(&self, item: &TimelineItem) -> TimelineItemPlacementState {
        self.replacements
            .and_then(|replacements| replacements.get(&item.id).copied())
            .unwrap_or_else(|| TimelineItemPlacementState::from(item))
    }

    fn is_replaced(&self, item_id: TimelineItemId) -> bool {
        self.replacements
            .is_some_and(|replacements| replacements.contains_key(&item_id))
    }

    fn replacements(&self) -> impl Iterator<Item = (&TimelineItemId, &TimelineItemPlacementState)> {
        self.replacements.into_iter().flat_map(BTreeMap::iter)
    }
}

impl<'a> ItemPlacementOverlay<'a> {
    pub(super) const fn new(
        replacements: &'a BTreeMap<TimelineItemId, TimelineItemPlacementState>,
    ) -> Self {
        Self {
            replacements: Some(replacements),
        }
    }
}

impl AuthoringProject {
    /// Validates only the changed placements and their indexed dependents.
    pub(crate) fn validate_timeline_item_placement_overlay(
        &self,
        index: &TimelineEditPlanningIndex,
        replacements: &BTreeMap<TimelineItemId, TimelineItemPlacementState>,
    ) -> Result<TimelineEditValidationScope, String> {
        let placements = ItemPlacementOverlay::new(replacements);
        let mut item_ids = replacements.keys().copied().collect::<BTreeSet<_>>();
        let mut transition_ids = BTreeSet::new();
        let mut attachment_ids = BTreeSet::new();
        let mut parameter_ids = BTreeSet::new();
        for item_id in replacements.keys() {
            if !self.items.contains_key(item_id) {
                return Err(format!("Timeline item {item_id} does not exist"));
            }
            item_ids.extend(
                index
                    .children_by_parent
                    .get(item_id)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
            transition_ids.extend(
                index
                    .transitions_by_item
                    .get(item_id)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
            transition_ids.extend(
                index
                    .transition_layer_span_dependents_by_item
                    .get(item_id)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
            attachment_ids.extend(
                index
                    .attachments_by_item
                    .get(item_id)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
            parameter_ids.extend(
                index
                    .parameters_by_item
                    .get(item_id)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
            for owner in index
                .binding_dependents_by_source
                .get(item_id)
                .into_iter()
                .flatten()
                .chain(
                    index
                        .exact_path_dependents
                        .get(item_id)
                        .into_iter()
                        .flatten(),
                )
            {
                match owner {
                    InvocationOwner::Item(item_id) => {
                        item_ids.insert(*item_id);
                    }
                    InvocationOwner::Attachment(attachment_id) => {
                        attachment_ids.insert(*attachment_id);
                    }
                    InvocationOwner::Transition(transition_id) => {
                        transition_ids.insert(*transition_id);
                    }
                }
            }
        }

        for item_id in &item_ids {
            let item = self
                .items
                .get(item_id)
                .ok_or_else(|| format!("Timeline item {item_id} does not exist"))?;
            self.validate_item_placement_constraints(item, &placements)?;
        }
        for attachment_id in &attachment_ids {
            let attachment = self
                .attachments
                .get(attachment_id)
                .ok_or_else(|| format!("Attachment {attachment_id} does not exist"))?;
            self.validate_attachment_placement_constraints(attachment, &placements)?;
        }
        for (timeline_id, parameter_id) in &parameter_ids {
            let timeline = self
                .timelines
                .get(timeline_id)
                .ok_or_else(|| format!("Timeline {timeline_id} does not exist"))?;
            let parameter = timeline
                .published_parameters
                .iter()
                .find(|parameter| parameter.id == *parameter_id)
                .ok_or_else(|| format!("Composition parameter {parameter_id} does not exist"))?;
            self.validate_composition_parameter_placement(timeline, parameter, &placements)?;
        }
        for transition_id in &transition_ids {
            let transition = self
                .transitions
                .get(transition_id)
                .ok_or_else(|| format!("Transition {transition_id} does not exist"))?;
            validate_transition(
                self,
                transition,
                &placements,
                index.transition_participants_have_clear_layer_span(self, &placements, transition),
            )?;
            for item_id in [transition.from_item_id, transition.to_item_id] {
                for other_id in index
                    .transitions_by_item
                    .get(&item_id)
                    .into_iter()
                    .flatten()
                {
                    let other = self
                        .transitions
                        .get(other_id)
                        .ok_or_else(|| format!("Transition {other_id} does not exist"))?;
                    validate_transition_participant_conflict(transition, other)?;
                }
            }
        }
        let moved_compositions =
            self.validate_changed_composition_cycles(index, replacements, &placements)?;
        Ok(TimelineEditValidationScope {
            items: item_ids.len(),
            transitions: transition_ids.len(),
            attachments: attachment_ids.len(),
            composition_parameters: parameter_ids.len(),
            moved_compositions,
        })
    }

    pub(super) fn validate_item_placement_constraints(
        &self,
        item: &TimelineItem,
        placements: &ItemPlacementOverlay<'_>,
    ) -> Result<(), String> {
        let placement = placements.state(item);
        let track = self
            .tracks
            .get(&placement.track_id)
            .ok_or_else(|| format!("Item {} refers to a missing Track", item.id))?;
        if placement.interval.start.is_negative()
            || placement.interval.duration.is_negative()
            || placement.time_map.source_start.is_negative()
            || placement.time_map.playback_rate.numerator() < 0
        {
            return Err(format!("Item {} has invalid timing", item.id));
        }
        placement.interval.end()?;
        if let Some(parent_id) = item.parent {
            let parent = self
                .items
                .get(&parent_id)
                .ok_or_else(|| format!("Item {} has a missing parent", item.id))?;
            let parent_track = self
                .tracks
                .get(&placements.state(parent).track_id)
                .ok_or_else(|| format!("Parent of item {} has a missing Track", item.id))?;
            if parent.id == item.id || parent_track.timeline_id != track.timeline_id {
                return Err(format!("Item {} has an invalid parent", item.id));
            }
        }
        match &item.source {
            SourceRef::Composition(instance) => {
                let nested = self
                    .timelines
                    .get(&instance.timeline_id)
                    .ok_or_else(|| format!("Item {} has a missing nested Timeline", item.id))?;
                validate_duration_policy(
                    item.id,
                    placement.interval,
                    nested.duration,
                    &instance.duration_policy,
                )?;
            }
            SourceRef::Module(invocation) => self.validate_invocation(
                invocation,
                Some(item.id),
                track.timeline_id,
                false,
                placements,
            )?,
            _ => {}
        }
        Ok(())
    }

    pub(super) fn validate_composition_parameter_placement(
        &self,
        timeline: &super::super::Timeline,
        parameter: &CompositionParameter,
        placements: &ItemPlacementOverlay<'_>,
    ) -> Result<(), String> {
        let target_item = self.items.get(&parameter.target.item_id()).ok_or_else(|| {
            format!(
                "Composition parameter {} targets a missing Timeline item",
                parameter.id
            )
        })?;
        let target_timeline_id = self
            .tracks
            .get(&placements.state(target_item).track_id)
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
        Ok(())
    }

    pub(super) fn validate_attachment_placement_constraints(
        &self,
        attachment: &super::super::Attachment,
        placements: &ItemPlacementOverlay<'_>,
    ) -> Result<(), String> {
        if let AttachmentProcessor::Module(invocation) = &attachment.processor {
            self.validate_invocation(
                invocation,
                None,
                self.attachment_owner_timeline(&attachment.owner, placements)?,
                true,
                placements,
            )?;
        }
        Ok(())
    }

    fn validate_changed_composition_cycles(
        &self,
        index: &TimelineEditPlanningIndex,
        replacements: &BTreeMap<TimelineItemId, TimelineItemPlacementState>,
        placements: &ItemPlacementOverlay<'_>,
    ) -> Result<usize, String> {
        let changed = replacements
            .keys()
            .filter(|item_id| {
                self.items
                    .get(item_id)
                    .is_some_and(|item| matches!(item.source, SourceRef::Composition(_)))
            })
            .copied()
            .collect::<BTreeSet<_>>();
        for item_id in &changed {
            let item = &self.items[item_id];
            let SourceRef::Composition(instance) = &item.source else {
                continue;
            };
            let owner_timeline = self
                .tracks
                .get(&placements.state(item).track_id)
                .ok_or_else(|| format!("Item {} has a missing Track", item.id))?
                .timeline_id;
            if self.composition_timeline_reaches(
                index,
                replacements,
                placements,
                instance.timeline_id,
                owner_timeline,
                &mut BTreeSet::new(),
            )? {
                return Err(format!("Nested Timeline cycle reaches {owner_timeline}"));
            }
        }
        Ok(changed.len())
    }

    fn composition_timeline_reaches(
        &self,
        index: &TimelineEditPlanningIndex,
        replacements: &BTreeMap<TimelineItemId, TimelineItemPlacementState>,
        placements: &ItemPlacementOverlay<'_>,
        current: TimelineId,
        target: TimelineId,
        visited: &mut BTreeSet<TimelineId>,
    ) -> Result<bool, String> {
        if current == target {
            return Ok(true);
        }
        if !visited.insert(current) {
            return Ok(false);
        }
        let mut outgoing_items = index
            .composition_items_by_timeline
            .get(&current)
            .into_iter()
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>();
        for item_id in replacements.keys().copied() {
            let Some(item) = self.items.get(&item_id) else {
                continue;
            };
            if matches!(item.source, SourceRef::Composition(_)) {
                let owner = self
                    .tracks
                    .get(&placements.state(item).track_id)
                    .ok_or_else(|| format!("Item {} has a missing Track", item.id))?
                    .timeline_id;
                if owner == current {
                    outgoing_items.insert(item_id);
                }
            }
        }
        for item_id in outgoing_items {
            let item = &self.items[&item_id];
            let owner = self
                .tracks
                .get(&placements.state(item).track_id)
                .ok_or_else(|| format!("Item {} has a missing Track", item.id))?
                .timeline_id;
            if owner != current {
                continue;
            }
            let SourceRef::Composition(instance) = &item.source else {
                continue;
            };
            if self.composition_timeline_reaches(
                index,
                replacements,
                placements,
                instance.timeline_id,
                target,
                visited,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
