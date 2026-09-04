use std::collections::HashMap;

use super::AuthoringProject;
use super::item_placement::{ItemPlacementOverlay, TimelineItemOrderIndex};
use super::validation::{validate_transition, validate_transition_participant_conflict};
use crate::model::authoring::{
    MediaOutputKind, MediaTime, RationalRate, TimelineId, TimelineItem, TimelineItemId, Transition,
    TransitionAlignment, TransitionId, TransitionProcessor,
};

/// A fully preflighted default Transition creation offered by Timeline UI.
///
/// The query owns participant order, media capability, timing, alignment, and
/// conflict policy. Presentation code must submit these values unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransitionCreationCandidate {
    pub timeline_id: TimelineId,
    pub from_item_id: TimelineItemId,
    pub to_item_id: TimelineItemId,
    pub output: MediaOutputKind,
    pub edit_point: MediaTime,
    pub duration: MediaTime,
    pub alignment: TransitionAlignment,
}

impl TransitionCreationCandidate {
    /// Returns the built-in processor paired with this preflighted media kind.
    pub fn processor(self) -> TransitionProcessor {
        match self.output {
            MediaOutputKind::Image => TransitionProcessor::cross_dissolve(),
            MediaOutputKind::Audio => TransitionProcessor::audio_crossfade(),
        }
    }
}

impl AuthoringProject {
    /// Returns the valid outgoing Transition choices for one Timeline item.
    ///
    /// Later-starting items with a butt cut or visible overlap are considered.
    /// Layer entries outside the Transition interval do not block a valid
    /// temporal edit; an active item between the participant layers does. A
    /// butt cut receives a conservative centered default using available
    /// visible spans, while an existing visible overlap is represented
    /// exactly. Every proposed choice is checked by the same validators used
    /// by persistence and RenderPlan compilation.
    pub fn transition_creation_candidates(
        &self,
        from_item_id: TimelineItemId,
    ) -> Result<Vec<TransitionCreationCandidate>, String> {
        let from = self
            .items
            .get(&from_item_id)
            .ok_or_else(|| format!("Timeline item {from_item_id} does not exist"))?;
        let timeline_id = self
            .tracks
            .get(&from.track_id)
            .ok_or_else(|| format!("Timeline item {from_item_id} has no Track"))?
            .timeline_id;
        let placements = ItemPlacementOverlay::empty();
        let order = TimelineItemOrderIndex::build(self, &placements);
        let mut targets = self
            .items
            .values()
            .filter(|to| {
                to.id != from.id
                    && to.track_id == from.track_id
                    && to.interval.start > from.interval.start
            })
            .collect::<Vec<_>>();
        targets.sort_by_key(|to| (to.interval.start, to.layer, to.id));
        let mut candidates = Vec::new();
        for to in targets {
            let Some((edit_point, duration, alignment)) = transition_default_timing(from, to)?
            else {
                continue;
            };
            for output in [MediaOutputKind::Image, MediaOutputKind::Audio] {
                let candidate = TransitionCreationCandidate {
                    timeline_id,
                    from_item_id,
                    to_item_id: to.id,
                    output,
                    edit_point,
                    duration,
                    alignment,
                };
                let proposed = Transition {
                    id: TransitionId::new(),
                    timeline_id,
                    from_item_id,
                    to_item_id: to.id,
                    edit_point,
                    duration,
                    alignment,
                    processor: candidate.processor(),
                    parameters: HashMap::new(),
                };
                if validate_transition(
                    self,
                    &proposed,
                    &placements,
                    order.participants_have_clear_layer_span(self, &placements, &proposed),
                )
                .is_ok()
                    && self.transitions.values().all(|existing| {
                        validate_transition_participant_conflict(&proposed, existing).is_ok()
                    })
                {
                    candidates.push(candidate);
                }
            }
        }
        Ok(candidates)
    }
}

fn transition_default_timing(
    from: &TimelineItem,
    to: &TimelineItem,
) -> Result<Option<(MediaTime, MediaTime, TransitionAlignment)>, String> {
    let from_end = from.interval.end()?;
    let to_end = to.interval.end()?;
    let overlap_start = from.interval.start.max(to.interval.start);
    let overlap_end = from_end.min(to_end);
    let (edit_point, duration) = if overlap_start < overlap_end {
        let duration = overlap_end.checked_sub(overlap_start)?;
        let half = duration.checked_div_rate(RationalRate::new(2, 1)?)?;
        (overlap_start.checked_add(half)?, duration)
    } else if from_end == to.interval.start {
        let duration = MediaTime::from_whole_seconds(1)
            .min(from.interval.duration)
            .min(to.interval.duration);
        if duration <= MediaTime::zero() {
            return Ok(None);
        }
        (from_end, duration)
    } else {
        return Ok(None);
    };
    Ok(Some((
        edit_point,
        duration,
        TransitionAlignment::CenteredOnEdit,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authoring::{
        RationalRate, SourceRef, TimelineInterval, TimelineTrackId, TimelineTrackKind,
    };
    use crate::model::frame::color::Color;
    use crate::model::project::asset::{Asset, AssetKind};

    fn seconds(value: i64) -> MediaTime {
        MediaTime::from_whole_seconds(value)
    }

    fn project() -> AuthoringProject {
        AuthoringProject::new(
            "transition candidates",
            320,
            180,
            RationalRate::new(30, 1).expect("valid fixture rate"),
            seconds(30),
        )
        .expect("valid fixture project")
    }

    fn add_solid(
        project: &mut AuthoringProject,
        track_id: TimelineTrackId,
        name: &str,
        start: i64,
        duration: i64,
        layer: i64,
    ) -> TimelineItemId {
        let id = TimelineItemId::new();
        project.items.insert(
            id,
            TimelineItem {
                id,
                track_id,
                name: name.to_string(),
                source: SourceRef::Solid {
                    color: Color::white(),
                },
                interval: TimelineInterval::new(seconds(start), seconds(duration))
                    .expect("valid fixture interval"),
                time_map: Default::default(),
                layer,
                parent: None,
                blend_mode: Default::default(),
                authored_properties: Default::default(),
            },
        );
        id
    }

    fn add_audio(
        project: &mut AuthoringProject,
        track_id: TimelineTrackId,
        name: &str,
        start: i64,
        duration: i64,
        layer: i64,
    ) -> TimelineItemId {
        let path = format!("{name}.wav");
        let asset = Asset::new(&path, &path, AssetKind::Audio);
        let asset_id = asset.id;
        project.assets.push(asset);
        let id = TimelineItemId::new();
        project.items.insert(
            id,
            TimelineItem {
                id,
                track_id,
                name: name.to_string(),
                source: SourceRef::Asset { asset_id },
                interval: TimelineInterval::new(seconds(start), seconds(duration))
                    .expect("valid fixture interval"),
                time_map: Default::default(),
                layer,
                parent: None,
                blend_mode: Default::default(),
                authored_properties: Default::default(),
            },
        );
        id
    }

    fn root_track(project: &AuthoringProject) -> TimelineTrackId {
        project.timelines[&project.root_timeline_id].track_order[0]
    }

    #[test]
    fn butt_cut_produces_a_centered_bounded_default() {
        let mut project = project();
        let track = root_track(&project);
        let from = add_solid(&mut project, track, "From", 0, 5, 0);
        let to = add_solid(&mut project, track, "To", 5, 5, 1);

        let candidates = project
            .transition_creation_candidates(from)
            .expect("candidate query");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].to_item_id, to);
        assert_eq!(candidates[0].output, MediaOutputKind::Image);
        assert_eq!(candidates[0].edit_point, seconds(5));
        assert_eq!(candidates[0].duration, seconds(1));
        assert_eq!(candidates[0].alignment, TransitionAlignment::CenteredOnEdit);
    }

    #[test]
    fn visible_overlap_is_preserved_as_the_exact_transition_interval() {
        let mut project = project();
        let track = root_track(&project);
        let from = add_solid(&mut project, track, "From", 0, 7, 0);
        let to = add_solid(&mut project, track, "To", 3, 7, 1);

        let candidates = project
            .transition_creation_candidates(from)
            .expect("candidate query");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].to_item_id, to);
        assert_eq!(candidates[0].edit_point, seconds(5));
        assert_eq!(candidates[0].duration, seconds(4));
    }

    #[test]
    fn inactive_layer_between_participants_does_not_hide_a_temporal_candidate() {
        let mut project = project();
        let track = root_track(&project);
        let from = add_solid(&mut project, track, "From", 0, 5, 0);
        let _middle = add_solid(&mut project, track, "Middle", 10, 2, 1);
        let target = add_solid(&mut project, track, "Target", 5, 5, 2);

        let candidates = project
            .transition_creation_candidates(from)
            .expect("candidate query");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].to_item_id, target);
    }

    #[test]
    fn active_layer_between_participants_blocks_the_candidate() {
        let mut project = project();
        let track = root_track(&project);
        let from = add_solid(&mut project, track, "From", 0, 5, 0);
        let _middle = add_solid(&mut project, track, "Middle", 4, 2, 1);
        let target = add_solid(&mut project, track, "Target", 5, 5, 2);

        let candidates = project
            .transition_creation_candidates(from)
            .expect("candidate query");

        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.to_item_id != target)
        );
    }

    #[test]
    fn overlapping_earlier_item_is_not_offered_as_a_reverse_transition() {
        let mut project = project();
        let track = root_track(&project);
        let earlier = add_solid(&mut project, track, "Earlier", 0, 7, 0);
        let later = add_solid(&mut project, track, "Later", 3, 7, 1);

        let candidates = project
            .transition_creation_candidates(later)
            .expect("candidate query");

        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.to_item_id != earlier)
        );
    }

    #[test]
    fn unsupported_output_is_not_offered() {
        let mut project = project();
        let track = root_track(&project);
        let from = add_solid(&mut project, track, "From", 0, 5, 0);
        add_solid(&mut project, track, "To", 5, 5, 1);

        let candidates = project
            .transition_creation_candidates(from)
            .expect("candidate query");

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.output)
                .collect::<Vec<_>>(),
            vec![MediaOutputKind::Image]
        );
    }

    #[test]
    fn track_kind_filters_candidates_to_the_pipeline_that_will_render_them() {
        let mut audio_track_project = project();
        let audio_track = root_track(&audio_track_project);
        audio_track_project
            .tracks
            .get_mut(&audio_track)
            .unwrap()
            .kind = TimelineTrackKind::Audio;
        let image_from = add_solid(&mut audio_track_project, audio_track, "Image A", 0, 5, 0);
        add_solid(&mut audio_track_project, audio_track, "Image B", 5, 5, 1);
        assert!(
            audio_track_project
                .transition_creation_candidates(image_from)
                .unwrap()
                .is_empty(),
            "an Audio Track must not offer an Image transition"
        );

        let mut visual_track_project = project();
        let visual_track = root_track(&visual_track_project);
        visual_track_project
            .tracks
            .get_mut(&visual_track)
            .unwrap()
            .kind = TimelineTrackKind::Visual;
        let audio_from = add_audio(&mut visual_track_project, visual_track, "Audio A", 0, 5, 0);
        add_audio(&mut visual_track_project, visual_track, "Audio B", 5, 5, 1);
        assert!(
            visual_track_project
                .transition_creation_candidates(audio_from)
                .unwrap()
                .is_empty(),
            "a Visual Track must not offer an Audio transition"
        );

        let mut visual_image_project = project();
        let visual_image_track = root_track(&visual_image_project);
        visual_image_project
            .tracks
            .get_mut(&visual_image_track)
            .unwrap()
            .kind = TimelineTrackKind::Visual;
        let visual_from = add_solid(
            &mut visual_image_project,
            visual_image_track,
            "Visual A",
            0,
            5,
            0,
        );
        add_solid(
            &mut visual_image_project,
            visual_image_track,
            "Visual B",
            5,
            5,
            1,
        );
        assert_eq!(
            visual_image_project
                .transition_creation_candidates(visual_from)
                .unwrap()
                .iter()
                .map(|candidate| candidate.output)
                .collect::<Vec<_>>(),
            vec![MediaOutputKind::Image]
        );

        let mut audio_audio_project = project();
        let audio_audio_track = root_track(&audio_audio_project);
        audio_audio_project
            .tracks
            .get_mut(&audio_audio_track)
            .unwrap()
            .kind = TimelineTrackKind::Audio;
        let audio_from = add_audio(
            &mut audio_audio_project,
            audio_audio_track,
            "Dedicated Audio A",
            0,
            5,
            0,
        );
        add_audio(
            &mut audio_audio_project,
            audio_audio_track,
            "Dedicated Audio B",
            5,
            5,
            1,
        );
        assert_eq!(
            audio_audio_project
                .transition_creation_candidates(audio_from)
                .unwrap()
                .iter()
                .map(|candidate| candidate.output)
                .collect::<Vec<_>>(),
            vec![MediaOutputKind::Audio]
        );

        let mut combined_project = project();
        let combined_track = root_track(&combined_project);
        let combined_from = add_audio(&mut combined_project, combined_track, "Combined A", 0, 5, 0);
        add_audio(&mut combined_project, combined_track, "Combined B", 5, 5, 1);
        assert_eq!(
            combined_project
                .transition_creation_candidates(combined_from)
                .unwrap()
                .iter()
                .map(|candidate| candidate.output)
                .collect::<Vec<_>>(),
            vec![MediaOutputKind::Audio],
            "an AudioVisual Track must retain compatible Audio candidates"
        );
    }

    #[test]
    fn existing_same_media_transition_is_excluded() {
        let mut project = project();
        let track = root_track(&project);
        let from = add_solid(&mut project, track, "From", 0, 5, 0);
        add_solid(&mut project, track, "To", 5, 5, 1);
        let candidate = project
            .transition_creation_candidates(from)
            .expect("candidate query")[0];
        let id = TransitionId::new();
        project.transitions.insert(
            id,
            Transition {
                id,
                timeline_id: candidate.timeline_id,
                from_item_id: candidate.from_item_id,
                to_item_id: candidate.to_item_id,
                edit_point: candidate.edit_point,
                duration: candidate.duration,
                alignment: candidate.alignment,
                processor: TransitionProcessor::cross_dissolve(),
                parameters: HashMap::new(),
            },
        );

        assert!(
            project
                .transition_creation_candidates(from)
                .expect("candidate query")
                .is_empty()
        );
    }
}
