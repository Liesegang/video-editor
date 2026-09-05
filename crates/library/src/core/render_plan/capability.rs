//! Capability dependency queries over a hierarchical RenderPlan.
//!
//! Selected Module Output metadata is authoritative. The Timeline walk keeps
//! nesting and public media bindings structural; callers that need an exact
//! answer for a partial nested TimeMap may refine a positive result at the
//! concrete frame/range boundary.

use std::collections::{HashSet, VecDeque};

use super::{CompiledModuleInvocation, ModuleHost, RenderCapability, RenderPlan};
use crate::error::LibraryError;
use crate::model::authoring::{
    AttachmentOwner, AttachmentProcessor, AuthoringProject, InstanceLocator, InstancePath,
    MediaInputBinding, MediaTime, SourceRef, TimelineId, TimelineItemId, TimelineTrackKind,
    TransitionMediaType,
};

impl RenderPlan {
    /// Returns whether a capability can contribute anywhere in the requested
    /// Timeline hierarchy. A positive result is conservative across a nested
    /// placement's partial TimeMap range; a negative result is authoritative.
    pub fn timeline_may_require_capability(
        &self,
        project: &AuthoringProject,
        timeline_id: TimelineId,
        instance_path: Option<&InstancePath>,
        capability: RenderCapability,
    ) -> Result<bool, LibraryError> {
        let path = instance_path
            .cloned()
            .unwrap_or_else(|| InstancePath::root(timeline_id));
        CapabilityReachability::new(project, self, capability)
            .timeline_may_require(timeline_id, path)
    }
}

struct CapabilityReachability<'a> {
    project: &'a AuthoringProject,
    plan: &'a RenderPlan,
    capability: RenderCapability,
    pending_timelines: VecDeque<(TimelineId, InstancePath)>,
    pending_items: VecDeque<(TimelineId, TimelineItemId, InstancePath)>,
    visited_timelines: HashSet<(TimelineId, InstancePath)>,
    visited_items: HashSet<(TimelineItemId, InstancePath)>,
}

impl<'a> CapabilityReachability<'a> {
    fn new(
        project: &'a AuthoringProject,
        plan: &'a RenderPlan,
        capability: RenderCapability,
    ) -> Self {
        Self {
            project,
            plan,
            capability,
            pending_timelines: VecDeque::new(),
            pending_items: VecDeque::new(),
            visited_timelines: HashSet::new(),
            visited_items: HashSet::new(),
        }
    }

    fn timeline_may_require(
        mut self,
        timeline_id: TimelineId,
        instance_path: InstancePath,
    ) -> Result<bool, LibraryError> {
        self.pending_timelines
            .push_back((timeline_id, instance_path));
        while !self.pending_timelines.is_empty() || !self.pending_items.is_empty() {
            if let Some((timeline_id, path)) = self.pending_timelines.pop_front() {
                if self.visit_timeline(timeline_id, path)? {
                    return Ok(true);
                }
            } else if let Some((timeline_id, item_id, path)) = self.pending_items.pop_front()
                && self.visit_item(timeline_id, item_id, path)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn visit_timeline(
        &mut self,
        timeline_id: TimelineId,
        path: InstancePath,
    ) -> Result<bool, LibraryError> {
        if !self.visited_timelines.insert((timeline_id, path.clone())) {
            return Ok(false);
        }
        let timeline = self.project.timelines.get(&timeline_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline {timeline_id} does not exist"))
        })?;
        let compiled = self.plan.timelines.get(&timeline_id).ok_or_else(|| {
            LibraryError::Validation(format!("RenderPlan has no Timeline {timeline_id}"))
        })?;

        for attachment in self.project.attachments.values().filter(|attachment| {
            attachment.enabled
                && !attachment.bypassed
                && matches!(&attachment.processor, AttachmentProcessor::Module(_))
                && attachment.stage.effect_media_type()
                    == Some(crate::model::project::PortDataType::Image)
                && match &attachment.owner {
                    AttachmentOwner::Timeline {
                        timeline_id: owner_id,
                    } => *owner_id == timeline_id,
                    AttachmentOwner::Track { track_id } => timeline.track_order.contains(track_id),
                    AttachmentOwner::Item { .. } => false,
                }
        }) {
            if self.host_may_require(ModuleHost::Attachment(attachment.id), timeline_id, &path)? {
                return Ok(true);
            }
        }

        for scheduled in &compiled.schedule {
            if scheduled.interval.duration == MediaTime::zero()
                || scheduled.interval.start >= timeline.duration
            {
                continue;
            }
            let track = self
                .project
                .tracks
                .get(&scheduled.track_id)
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Timeline item {} has no Track {}",
                        scheduled.item_id, scheduled.track_id
                    ))
                })?;
            if track.kind != TimelineTrackKind::Audio {
                self.pending_items
                    .push_back((timeline_id, scheduled.item_id, path.clone()));
            }
        }
        for transition in &compiled.transitions {
            let transition_interval = transition.progress.interval();
            if transition.processor.contract.media_type != TransitionMediaType::Image
                || transition_interval.duration == MediaTime::zero()
                || transition_interval.start >= timeline.duration
            {
                continue;
            }
            if let Some(host) = transition.module_host {
                let invocation = self
                    .plan
                    .effective_transition_invocation(host, &path)
                    .ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "RenderPlan has no Module invocation for Transition {}",
                            transition.id
                        ))
                    })?;
                if self.invocation_may_require(&invocation, timeline_id, &path)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn visit_item(
        &mut self,
        timeline_id: TimelineId,
        item_id: TimelineItemId,
        path: InstancePath,
    ) -> Result<bool, LibraryError> {
        if !self.visited_items.insert((item_id, path.clone())) {
            return Ok(false);
        }
        let item = self.project.items.get(&item_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline item {item_id} does not exist"))
        })?;
        let track = self.project.tracks.get(&item.track_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline item {item_id} has no Track"))
        })?;
        if track.timeline_id != timeline_id {
            return Err(LibraryError::Validation(format!(
                "Timeline item {item_id} belongs to Timeline {}, not {timeline_id}",
                track.timeline_id
            )));
        }

        for attachment in self.project.attachments.values().filter(|attachment| {
            attachment.enabled
                && !attachment.bypassed
                && matches!(&attachment.processor, AttachmentProcessor::Module(_))
                && attachment.stage.effect_media_type()
                    == Some(crate::model::project::PortDataType::Image)
                && attachment.owner == AttachmentOwner::Item { item_id }
        }) {
            if self.host_may_require(ModuleHost::Attachment(attachment.id), timeline_id, &path)? {
                return Ok(true);
            }
        }

        match &item.source {
            SourceRef::Composition(instance) => self
                .pending_timelines
                .push_back((instance.timeline_id, path.nested(item_id))),
            SourceRef::Module(_) => {
                if self.host_may_require(
                    ModuleHost::TimelineItem {
                        timeline_id,
                        item_id,
                    },
                    timeline_id,
                    &path,
                )? {
                    return Ok(true);
                }
            }
            SourceRef::Asset { .. }
            | SourceRef::Text { .. }
            | SourceRef::Shape { .. }
            | SourceRef::Solid { .. } => {}
        }
        Ok(false)
    }

    fn host_may_require(
        &mut self,
        host: ModuleHost,
        timeline_id: TimelineId,
        path: &InstancePath,
    ) -> Result<bool, LibraryError> {
        let invocation = self.plan.invocation(host).cloned().ok_or_else(|| {
            LibraryError::Validation(format!("RenderPlan has no invocation for {host:?}"))
        })?;
        self.invocation_may_require(&invocation, timeline_id, path)
    }

    fn invocation_may_require(
        &mut self,
        invocation: &CompiledModuleInvocation,
        timeline_id: TimelineId,
        path: &InstancePath,
    ) -> Result<bool, LibraryError> {
        let definition = self
            .plan
            .module_definitions
            .get(&invocation.definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan has no Module definition {}",
                    invocation.definition_id
                ))
            })?;
        let output = definition
            .outputs
            .get(&invocation.output_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Module invocation selects missing output {}",
                    invocation.output_id
                ))
            })?;
        if output.requires(self.capability) {
            return Ok(true);
        }
        for (input_id, binding) in &invocation.input_bindings {
            if !output.reachable_media_inputs.contains(input_id) {
                continue;
            }
            let MediaInputBinding::TimelineItemOutput {
                locator, item_id, ..
            } = binding;
            let (source_timeline_id, source_path) = match locator {
                InstanceLocator::SameTimeline => (timeline_id, path.clone()),
                InstanceLocator::Exact(exact_path) => {
                    let item = self.project.items.get(item_id).ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Module input references missing Timeline item {item_id}"
                        ))
                    })?;
                    let track = self.project.tracks.get(&item.track_id).ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Module input item {item_id} has no Track"
                        ))
                    })?;
                    (track.timeline_id, exact_path.clone())
                }
            };
            self.pending_items
                .push_back((source_timeline_id, *item_id, source_path));
        }
        Ok(false)
    }
}
