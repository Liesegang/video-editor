use library::model::frame::frame::{FrameInfo, Region};
use library::model::project::Project;
use library::RenderRequestId;
use ordered_float::OrderedFloat;
use uuid::Uuid;

/// Render-affecting view state that is not part of the authoritative Project.
///
/// Timeline time is intentionally absent. Continuous playback may publish the
/// newest completed frame even after the audio clock has advanced, while every
/// field in this key remains an exact invalidation boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewPresentationKey {
    composition_id: Uuid,
    width: u64,
    height: u64,
    background_color: library::model::frame::color::Color,
    color_profile: String,
    render_scale: OrderedFloat<f64>,
    region: Option<Region>,
}

impl PreviewPresentationKey {
    pub fn from_frame(composition_id: Uuid, frame: &FrameInfo) -> Self {
        Self {
            composition_id,
            width: frame.width,
            height: frame.height,
            background_color: frame.background_color.clone(),
            color_profile: frame.color_profile.clone(),
            render_scale: frame.render_scale,
            region: frame.region,
        }
    }
}

#[derive(Clone, Debug)]
struct DesiredRender {
    generation: u64,
    frame: FrameInfo,
}

#[derive(Clone, Debug)]
struct InFlightRender {
    request_id: RenderRequestId,
    generation: u64,
    frame: FrameInfo,
}

#[derive(Clone, Debug)]
struct CompletedRender {
    generation: u64,
    frame: FrameInfo,
}

#[derive(Clone, Debug)]
pub struct PreviewRenderSubmission {
    pub request_id: RenderRequestId,
    pub frame: FrameInfo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewCompletionDecision {
    Publish,
    Discard,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreviewRenderDiagnostics {
    pub generation: u64,
    pub in_flight_request: Option<u64>,
    pub desired_pending: bool,
    pub submitted: u64,
    pub published: u64,
    pub discarded: u64,
    pub coalesced: u64,
}

/// Single-flight Preview scheduler with one caller-side latest-only slot.
///
/// The render worker cannot cancel a frame already executing. Keeping only one
/// submitted frame plus one replaceable desired frame avoids an unbounded
/// request flood. A completed frame may lag the audio clock only within the
/// same uninterrupted playback generation. Project mutations (including live
/// edits), explicit seeks, playback transitions, and presentation changes
/// advance the generation and make old results unpublishable.
pub struct PreviewRenderScheduler {
    generation: u64,
    next_request_serial: u64,
    last_project: Option<Project>,
    last_presentation: Option<PreviewPresentationKey>,
    last_seek_revision: Option<u64>,
    last_playing: Option<bool>,
    last_requested_time: Option<OrderedFloat<f64>>,
    available: bool,
    desired: Option<DesiredRender>,
    in_flight: Option<InFlightRender>,
    last_completed: Option<CompletedRender>,
    submitted: u64,
    published: u64,
    discarded: u64,
    coalesced: u64,
}

impl Default for PreviewRenderScheduler {
    fn default() -> Self {
        Self {
            generation: 1,
            next_request_serial: 1,
            last_project: None,
            last_presentation: None,
            last_seek_revision: None,
            last_playing: None,
            last_requested_time: None,
            available: false,
            desired: None,
            in_flight: None,
            last_completed: None,
            submitted: 0,
            published: 0,
            discarded: 0,
            coalesced: 0,
        }
    }
}

impl PreviewRenderScheduler {
    pub fn update_desired(
        &mut self,
        project: &Project,
        presentation: PreviewPresentationKey,
        frame: FrameInfo,
        playing: bool,
        seek_revision: u64,
    ) {
        let project_changed = self
            .last_project
            .as_ref()
            .is_some_and(|previous| previous != project);
        let presentation_changed = self
            .last_presentation
            .as_ref()
            .is_some_and(|previous| previous != &presentation);
        let seeked = self
            .last_seek_revision
            .is_some_and(|previous| previous != seek_revision);
        let playback_changed = self
            .last_playing
            .is_some_and(|previous| previous != playing);
        let time_discontinuity = self.last_requested_time.is_some_and(|previous| {
            if playing {
                frame.now_time < previous
            } else {
                frame.now_time != previous
            }
        });

        if project_changed
            || presentation_changed
            || seeked
            || playback_changed
            || time_discontinuity
        {
            self.advance_generation();
        }

        if self.last_project.is_none() || project_changed {
            self.last_project = Some(project.clone());
        }
        self.last_presentation = Some(presentation);
        self.last_seek_revision = Some(seek_revision);
        self.last_playing = Some(playing);
        self.last_requested_time = Some(frame.now_time);
        self.available = true;

        let generation = self.generation;
        if self
            .in_flight
            .as_ref()
            .is_some_and(|request| request.generation == generation && request.frame == frame)
            || self.last_completed.as_ref().is_some_and(|completed| {
                completed.generation == generation && completed.frame == frame
            })
        {
            self.desired = None;
            return;
        }
        if self
            .desired
            .as_ref()
            .is_some_and(|desired| desired.generation == generation && desired.frame == frame)
        {
            return;
        }
        if self.desired.is_some() {
            self.coalesced = self.coalesced.wrapping_add(1);
        }
        self.desired = Some(DesiredRender { generation, frame });
    }

    /// Invalidate any result still executing when the Preview cannot produce a
    /// current frame (for example an evaluation error or empty ROI).
    pub fn suspend(&mut self) {
        if self.available {
            self.advance_generation();
        }
        self.available = false;
        self.desired = None;
        self.last_presentation = None;
        self.last_playing = None;
        self.last_requested_time = None;
    }

    pub fn take_submission(&mut self) -> Option<PreviewRenderSubmission> {
        if self.in_flight.is_some() {
            return None;
        }
        let desired = self.desired.take()?;
        let request_id = RenderRequestId::new(self.next_request_serial);
        self.next_request_serial = next_nonzero(self.next_request_serial);
        self.submitted = self.submitted.wrapping_add(1);
        let frame = desired.frame.clone();
        self.in_flight = Some(InFlightRender {
            request_id,
            generation: desired.generation,
            frame: desired.frame,
        });
        Some(PreviewRenderSubmission { request_id, frame })
    }

    pub fn submission_failed(&mut self, request_id: RenderRequestId) {
        if self
            .in_flight
            .as_ref()
            .is_some_and(|request| request.request_id == request_id)
        {
            self.in_flight = None;
            self.discarded = self.discarded.wrapping_add(1);
        }
    }

    pub fn complete(
        &mut self,
        request_id: RenderRequestId,
        completed_frame: &FrameInfo,
    ) -> PreviewCompletionDecision {
        let Some(in_flight) = self.in_flight.take() else {
            self.discarded = self.discarded.wrapping_add(1);
            return PreviewCompletionDecision::Discard;
        };
        if in_flight.request_id != request_id {
            self.in_flight = Some(in_flight);
            self.discarded = self.discarded.wrapping_add(1);
            return PreviewCompletionDecision::Discard;
        }

        let latest_frame = self
            .desired
            .as_ref()
            .filter(|desired| desired.generation == self.generation)
            .map_or(&in_flight.frame, |desired| &desired.frame);
        let exact_protocol_match = &in_flight.frame == completed_frame;
        let generation_is_current = self.available && in_flight.generation == self.generation;
        let exact_current_frame = completed_frame == latest_frame;
        let playback_can_skip = self.last_playing == Some(true)
            && completed_frame.now_time <= latest_frame.now_time
            && self.last_completed.as_ref().is_none_or(|last| {
                last.generation != self.generation
                    || completed_frame.now_time >= last.frame.now_time
            });
        let publish = exact_protocol_match
            && generation_is_current
            && (exact_current_frame || playback_can_skip);

        if publish {
            self.published = self.published.wrapping_add(1);
            self.last_completed = Some(CompletedRender {
                generation: self.generation,
                frame: completed_frame.clone(),
            });
            if self.desired.as_ref().is_some_and(|desired| {
                desired.generation == self.generation && desired.frame == *completed_frame
            }) {
                self.desired = None;
            }
            PreviewCompletionDecision::Publish
        } else {
            self.discarded = self.discarded.wrapping_add(1);
            PreviewCompletionDecision::Discard
        }
    }

    pub fn requires_repaint(&self) -> bool {
        self.in_flight.is_some() || self.desired.is_some()
    }

    pub fn diagnostics(&self) -> PreviewRenderDiagnostics {
        PreviewRenderDiagnostics {
            generation: self.generation,
            in_flight_request: self
                .in_flight
                .as_ref()
                .map(|request| request.request_id.get()),
            desired_pending: self.desired.is_some(),
            submitted: self.submitted,
            published: self.published,
            discarded: self.discarded,
            coalesced: self.coalesced,
        }
    }

    fn advance_generation(&mut self) {
        self.generation = next_nonzero(self.generation);
        self.desired = None;
        self.last_completed = None;
    }
}

const fn next_nonzero(value: u64) -> u64 {
    let next = value.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::frame::color::Color;

    fn frame(time: f64) -> FrameInfo {
        FrameInfo {
            width: 1920,
            height: 1080,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(time),
            region: Some(Region {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            }),
            items: Vec::new(),
        }
    }

    fn update(
        scheduler: &mut PreviewRenderScheduler,
        project: &Project,
        composition_id: Uuid,
        frame: FrameInfo,
        playing: bool,
        seek_revision: u64,
    ) {
        let presentation = PreviewPresentationKey::from_frame(composition_id, &frame);
        scheduler.update_desired(project, presentation, frame, playing, seek_revision);
    }

    #[test]
    fn slow_continuous_playback_publishes_completed_frame_and_skips_to_latest() {
        let project = Project::new("playback");
        let composition_id = Uuid::new_v4();
        let mut scheduler = PreviewRenderScheduler::default();

        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(0.0),
            true,
            0,
        );
        let first = scheduler.take_submission().expect("initial request");
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(1.0),
            true,
            0,
        );
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(2.0),
            true,
            0,
        );
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(3.0),
            true,
            0,
        );

        assert_eq!(
            scheduler.complete(first.request_id, &first.frame),
            PreviewCompletionDecision::Publish,
            "latest completed pixels must remain visible while playback advances"
        );
        let next = scheduler
            .take_submission()
            .expect("latest desired frame replaces intermediate requests");
        assert_eq!(next.frame.now_time, OrderedFloat(3.0));
        assert_eq!(scheduler.diagnostics().submitted, 2);
        assert_eq!(scheduler.diagnostics().coalesced, 2);
    }

    #[test]
    fn live_project_edit_discards_old_pixels_before_scheduling_new_generation() {
        let mut project = Project::new("before");
        let composition_id = Uuid::new_v4();
        let mut scheduler = PreviewRenderScheduler::default();
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(0.0),
            false,
            0,
        );
        let stale = scheduler.take_submission().expect("initial request");

        project.name = "live edit before history commit".to_string();
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(0.0),
            false,
            0,
        );
        assert_eq!(
            scheduler.complete(stale.request_id, &stale.frame),
            PreviewCompletionDecision::Discard
        );
        assert!(scheduler.take_submission().is_some());
    }

    #[test]
    fn explicit_seek_while_playing_discards_pre_seek_result() {
        let project = Project::new("seek");
        let composition_id = Uuid::new_v4();
        let mut scheduler = PreviewRenderScheduler::default();
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(1.0),
            true,
            0,
        );
        let stale = scheduler.take_submission().expect("pre-seek request");
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(20.0),
            true,
            1,
        );

        assert_eq!(
            scheduler.complete(stale.request_id, &stale.frame),
            PreviewCompletionDecision::Discard
        );
        assert_eq!(
            scheduler
                .take_submission()
                .expect("post-seek request")
                .frame
                .now_time,
            OrderedFloat(20.0)
        );
    }

    #[test]
    fn composition_roi_and_scale_changes_are_strict_invalidation_boundaries() {
        let project = Project::new("presentation");
        let first_composition = Uuid::new_v4();
        let second_composition = Uuid::new_v4();

        for changed in [
            (second_composition, frame(0.0)),
            {
                let mut changed = frame(0.0);
                changed.region.as_mut().expect("region").x = 100.0;
                (first_composition, changed)
            },
            {
                let mut changed = frame(0.0);
                changed.render_scale = OrderedFloat(0.5);
                (first_composition, changed)
            },
        ] {
            let mut scheduler = PreviewRenderScheduler::default();
            update(
                &mut scheduler,
                &project,
                first_composition,
                frame(0.0),
                false,
                0,
            );
            let stale = scheduler.take_submission().expect("initial request");
            update(&mut scheduler, &project, changed.0, changed.1, false, 0);
            assert_eq!(
                scheduler.complete(stale.request_id, &stale.frame),
                PreviewCompletionDecision::Discard
            );
        }
    }

    #[test]
    fn paused_frame_is_submitted_once_and_repaint_stops_after_completion() {
        let project = Project::new("paused");
        let composition_id = Uuid::new_v4();
        let mut scheduler = PreviewRenderScheduler::default();
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(4.0),
            false,
            0,
        );
        let request = scheduler.take_submission().expect("paused request");
        assert!(scheduler.requires_repaint());

        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(4.0),
            false,
            0,
        );
        assert!(scheduler.take_submission().is_none());
        assert_eq!(
            scheduler.complete(request.request_id, &request.frame),
            PreviewCompletionDecision::Publish
        );
        assert!(!scheduler.requires_repaint());

        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(4.0),
            false,
            0,
        );
        assert!(scheduler.take_submission().is_none());
        assert_eq!(scheduler.diagnostics().submitted, 1);
    }

    #[test]
    fn suspension_discards_in_flight_result_then_stops_repainting() {
        let project = Project::new("error");
        let composition_id = Uuid::new_v4();
        let mut scheduler = PreviewRenderScheduler::default();
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(0.0),
            false,
            0,
        );
        let request = scheduler.take_submission().expect("request");
        scheduler.suspend();
        assert!(
            scheduler.requires_repaint(),
            "drain the stale worker result"
        );
        assert_eq!(
            scheduler.complete(request.request_id, &request.frame),
            PreviewCompletionDecision::Discard
        );
        assert!(!scheduler.requires_repaint());
    }

    #[test]
    fn unknown_out_of_order_result_cannot_consume_the_real_in_flight_slot() {
        let project = Project::new("out of order");
        let composition_id = Uuid::new_v4();
        let mut scheduler = PreviewRenderScheduler::default();
        update(
            &mut scheduler,
            &project,
            composition_id,
            frame(0.0),
            false,
            0,
        );
        let request = scheduler.take_submission().expect("request");

        assert_eq!(
            scheduler.complete(RenderRequestId::new(999), &request.frame),
            PreviewCompletionDecision::Discard
        );
        assert_eq!(
            scheduler.diagnostics().in_flight_request,
            Some(request.request_id.get())
        );
        assert_eq!(
            scheduler.complete(request.request_id, &request.frame),
            PreviewCompletionDecision::Publish
        );
        assert!(!scheduler.requires_repaint());
    }
}
