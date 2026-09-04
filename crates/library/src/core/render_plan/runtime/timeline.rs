//! Hierarchical Timeline scheduling, including first-class image transitions.

use std::collections::{HashMap, HashSet};

use super::frame_values::neutralize_root_blend;
use super::transition_module::TransitionModuleImageRequest;
use super::*;
use crate::core::render_plan::{CompiledTimeline, CompiledTransition};
use crate::error::TransitionSourceHandleError;
use crate::model::authoring::{TimelineTrackId, TransitionMediaType};
use crate::model::frame::entity::{
    FrameTransition, FrameTransitionKind, FrameTransitionSource, NormalizedProgress16,
};

impl AuthoringFrameEvaluator<'_> {
    pub(super) fn evaluate_timeline_group_inner(
        &mut self,
        timeline_id: TimelineId,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
    ) -> Result<FrameItem, LibraryError> {
        let timeline = self.project.timelines.get(&timeline_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline {timeline_id} does not exist"))
        })?;
        let compiled = self.plan.timelines.get(&timeline_id).ok_or_else(|| {
            LibraryError::Validation(format!("RenderPlan has no Timeline {timeline_id}"))
        })?;
        let mut tracks = Vec::new();
        for track_id in &timeline.track_order {
            let track = self.project.tracks.get(track_id).ok_or_else(|| {
                LibraryError::Validation(format!("Timeline {timeline_id} has a missing Track"))
            })?;
            if track.kind == TimelineTrackKind::Audio {
                continue;
            }
            let children = self.evaluate_track_children(
                timeline_id,
                *track_id,
                timeline_time,
                instance_path,
                compiled,
            )?;
            if children.is_empty() {
                continue;
            }
            let track_time = timeline_time.to_seconds_f64();
            let mut group = FrameItem::Group(FrameGroup {
                source_id: track.id.as_uuid(),
                kind: FrameGroupKind::Track,
                width: timeline.width,
                height: timeline.height,
                background_color: transparent(),
                transform: transform_at(&track.authored_properties, track_time)?,
                blend_mode: BlendMode::Normal,
                effect_time: OrderedFloat(track_time),
                effects: Vec::new(),
                items: children,
            });
            group = self.apply_attachments(
                group,
                &AttachmentOwner::Track { track_id: track.id },
                AttachmentStage::TrackPostComposite,
                timeline_id,
                timeline_time,
                timeline_time,
                instance_path,
            )?;
            tracks.push(group);
        }
        let seconds = timeline_time.to_seconds_f64();
        let mut group = FrameItem::Group(FrameGroup {
            source_id: timeline.id.as_uuid(),
            kind: FrameGroupKind::Composition,
            width: timeline.width,
            height: timeline.height,
            background_color: timeline.background_color.clone(),
            transform: transform_at(&timeline.authored_properties, seconds)?,
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(seconds),
            effects: Vec::new(),
            items: tracks,
        });
        group = self.apply_attachments(
            group,
            &AttachmentOwner::Timeline { timeline_id },
            AttachmentStage::TimelinePostComposite,
            timeline_id,
            timeline_time,
            timeline_time,
            instance_path,
        )?;
        Ok(group)
    }

    fn evaluate_track_children(
        &mut self,
        timeline_id: TimelineId,
        track_id: TimelineTrackId,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
        compiled: &CompiledTimeline,
    ) -> Result<Vec<FrameItem>, LibraryError> {
        let mut suppressed = HashSet::new();
        let mut transitions_at: HashMap<usize, Vec<&CompiledTransition>> = HashMap::new();
        for transition in &compiled.transitions {
            if transition.processor.contract.media_type != TransitionMediaType::Image
                || !transition
                    .progress
                    .interval()
                    .contains(timeline_time)
                    .map_err(LibraryError::Validation)?
            {
                continue;
            }
            if !transition.processor.is_builtin_cross_dissolve()
                && transition.processor.module_processor().is_none()
            {
                let identity = transition.processor.operation().map_or_else(
                    || "ModuleDefinition".to_string(),
                    |operation| format!("{}@{}", operation.component_id, operation.version),
                );
                return Err(LibraryError::Render(format!(
                    "Image transition {} has no registered runtime processor '{identity}'",
                    transition.id
                )));
            }
            let from = compiled.schedule.get(transition.from.schedule_index);
            let to = compiled.schedule.get(transition.to.schedule_index);
            if from.is_none_or(|source| source.track_id != track_id)
                || to.is_none_or(|source| source.track_id != track_id)
            {
                continue;
            }
            for source_index in [transition.from.schedule_index, transition.to.schedule_index] {
                if !suppressed.insert(source_index) {
                    return Err(LibraryError::Validation(format!(
                        "Timeline item at schedule index {source_index} participates in multiple active Image transitions"
                    )));
                }
            }
            transitions_at
                .entry(transition.output_schedule_index)
                .or_default()
                .push(transition);
        }

        let mut children = Vec::new();
        for schedule_index in compiled
            .track_schedules
            .get(&track_id)
            .into_iter()
            .flatten()
        {
            let scheduled = compiled.schedule.get(*schedule_index).ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Timeline {timeline_id} schedule index is invalid"
                ))
            })?;
            if !suppressed.contains(schedule_index)
                && scheduled
                    .is_active(timeline_time)
                    .map_err(LibraryError::Validation)?
                && let Some(item) = self.evaluate_item_stage(
                    timeline_id,
                    scheduled.item_id,
                    timeline_time,
                    instance_path,
                    ItemOutputStage::PostTransform,
                )?
            {
                children.push(item);
            }
            if let Some(transitions) = transitions_at.remove(schedule_index) {
                for transition in transitions {
                    children.push(self.evaluate_image_transition(
                        timeline_id,
                        timeline_time,
                        instance_path,
                        compiled,
                        transition,
                    )?);
                }
            }
        }
        Ok(children)
    }

    fn evaluate_image_transition(
        &mut self,
        timeline_id: TimelineId,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
        compiled: &CompiledTimeline,
        transition: &CompiledTransition,
    ) -> Result<FrameItem, LibraryError> {
        let evaluate = |runtime: &mut Self,
                        item_id|
         -> Result<FrameTransitionSource, LibraryError> {
            let source_time = compiled
                .schedule
                .iter()
                .find(|scheduled| scheduled.item_id == item_id)
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Transition {} source item {item_id} is absent from its compiled schedule",
                        transition.id
                    ))
                })?
                .local_time(timeline_time)
                .map_err(LibraryError::Validation)?;
            let mut item = runtime
                .evaluate_item_stage_for_transition(
                    timeline_id,
                    item_id,
                    timeline_time,
                    instance_path,
                    ItemOutputStage::PostTransform,
                    transition.id,
                )?
                .ok_or_else(|| {
                    LibraryError::TransitionSourceHandleUnavailable(TransitionSourceHandleError {
                        transition_id: transition.id.as_uuid(),
                        item_id: item_id.as_uuid(),
                        timeline_time: timeline_time.to_seconds_f64(),
                        source_time: source_time.to_seconds_f64(),
                        reason: "source evaluator produced no Image".to_string(),
                    })
                })?;
            // A/B are isolated processor inputs. Their Timeline blend modes
            // belong to placement compositing and must not be evaluated
            // against each input's transparent isolation surface.
            neutralize_root_blend(&mut item);
            Ok(FrameTransitionSource {
                item_id: item_id.as_uuid(),
                source_time: OrderedFloat(source_time.to_seconds_f64()),
                item,
            })
        };
        let from = evaluate(self, transition.from.item_id)?;
        let to = evaluate(self, transition.to.item_id)?;
        let timeline = &self.project.timelines[&timeline_id];
        let progress = transition
            .progress
            .sample_at(timeline_time)
            .map_err(LibraryError::Validation)?;
        if !progress.is_finite() || !(0.0..=1.0).contains(&progress) {
            return Err(LibraryError::Render(format!(
                "Cross Dissolve {} produced invalid normalized progress {progress}",
                transition.id
            )));
        }
        let progress = NormalizedProgress16::new(progress as f32).map_err(LibraryError::Render)?;
        if transition.processor.is_builtin_cross_dissolve() {
            return Ok(FrameItem::Transition(Box::new(FrameTransition {
                transition_id: transition.id.as_uuid(),
                timeline_time: OrderedFloat(timeline_time.to_seconds_f64()),
                kind: FrameTransitionKind::CrossDissolve,
                width: timeline.width,
                height: timeline.height,
                blend_mode: transition.output_blend_mode,
                progress,
                from,
                to,
            })));
        }
        if transition.processor.module_processor().is_some() {
            return self.evaluate_transition_module_image(TransitionModuleImageRequest {
                timeline_id,
                timeline_time,
                instance_path,
                transition,
                progress,
                from,
                to,
            });
        }
        Err(LibraryError::Render(format!(
            "Image transition {} has no executable processor",
            transition.id
        )))
    }
}
