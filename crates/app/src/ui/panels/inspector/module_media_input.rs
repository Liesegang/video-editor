//! Shared Inspector control for binding a Published media input to a Timeline item.
//!
//! Module hosts persist only Published Interface IDs. This picker deliberately
//! has no knowledge of Module-internal Node UUIDs.

use egui_phosphor::regular as icons;
use library::model::authoring::{
    AuthoringProject, InstanceLocator, ItemOutputStage, MediaInputBinding, MediaOutputKind,
    PublishedMediaInput, TimelineId, TimelineItem, TimelineItemId,
};
use library::model::project::PortDataType;

pub(super) enum MediaInputPickerAction {
    Bind(MediaInputBinding),
    Unbind,
    Inherit,
}

pub(super) struct MediaInputPicker<'a> {
    pub control_id: &'a str,
    pub project: &'a AuthoringProject,
    pub timeline_id: TimelineId,
    pub input: &'a PublishedMediaInput,
    pub current: Option<&'a MediaInputBinding>,
    pub excluded_items: &'a [TimelineItemId],
    /// A placement-local decision exists and can be removed to reveal its
    /// definition-scope binding again.
    pub can_inherit: bool,
}

pub(super) fn media_input_picker(
    ui: &mut egui::Ui,
    picker: MediaInputPicker<'_>,
) -> Option<MediaInputPickerAction> {
    let output_kind = media_output_kind(picker.input.data_type)?;
    let candidates = input_candidates(
        picker.project,
        picker.timeline_id,
        output_kind,
        picker.excluded_items,
    );
    let current_item_id = picker.current.map(|binding| {
        let MediaInputBinding::TimelineItemOutput { item_id, .. } = binding;
        *item_id
    });
    let current_label = current_item_id
        .and_then(|item_id| picker.project.items.get(&item_id))
        .map_or_else(
            || {
                if picker.input.required {
                    "Choose clip...".to_string()
                } else {
                    "Unbound".to_string()
                }
            },
            |item| {
                if matches!(
                    picker.current,
                    Some(MediaInputBinding::TimelineItemOutput {
                        locator: InstanceLocator::Exact(_),
                        ..
                    })
                ) {
                    format!("{} (fixed instance)", item.name)
                } else {
                    item.name.clone()
                }
            },
        );
    let mut action = None;
    let response = ui
        .horizontal(|ui| {
            super::property_label(ui, picker.control_id, &picker.input.name);
            egui::ComboBox::from_id_salt(("published-media-input", picker.control_id))
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(picker.current.is_none(), "Unbound")
                        .clicked()
                        && picker.current.is_some()
                    {
                        action = Some(MediaInputPickerAction::Unbind);
                        ui.close();
                    }
                    for source in &candidates {
                        let selected = matches!(
                            picker.current,
                            Some(MediaInputBinding::TimelineItemOutput {
                                locator: InstanceLocator::SameTimeline,
                                item_id,
                                ..
                            }) if *item_id == source.id
                        );
                        if ui.selectable_label(selected, &source.name).clicked() {
                            action = Some(MediaInputPickerAction::Bind(
                                MediaInputBinding::TimelineItemOutput {
                                    locator: InstanceLocator::SameTimeline,
                                    item_id: source.id,
                                    output: output_kind,
                                    stage: match output_kind {
                                        MediaOutputKind::Image => ItemOutputStage::PostTransform,
                                        MediaOutputKind::Audio => ItemOutputStage::PostEffects,
                                    },
                                },
                            ));
                            ui.close();
                        }
                    }
                });
            if picker.can_inherit
                && ui
                    .small_button(icons::ARROW_COUNTER_CLOCKWISE)
                    .on_hover_text("Use the inherited clip binding")
                    .clicked()
            {
                action = Some(MediaInputPickerAction::Inherit);
            }
        })
        .response;
    crate::qa::register_component_with_metadata(
        picker.control_id,
        "published_media_input_picker",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "published_input_id": picker.input.id,
            "media_type": match output_kind {
                MediaOutputKind::Image => "image",
                MediaOutputKind::Audio => "audio",
            },
            "bound_item_id": current_item_id,
            "candidate_count": candidates.len(),
            "can_inherit": picker.can_inherit,
        })),
    );
    if picker.input.required && picker.current.is_none() {
        ui.colored_label(ui.visuals().warn_fg_color, "A clip input is required");
    }
    action
}

fn input_candidates<'a>(
    project: &'a AuthoringProject,
    timeline_id: TimelineId,
    output_kind: MediaOutputKind,
    excluded_items: &[TimelineItemId],
) -> Vec<&'a TimelineItem> {
    let mut candidates = project
        .items
        .values()
        .filter(|candidate| {
            project
                .tracks
                .get(&candidate.track_id)
                .is_some_and(|track| track.timeline_id == timeline_id)
        })
        .filter(|candidate| !excluded_items.contains(&candidate.id))
        .filter(|candidate| {
            project
                .item_supports_output(candidate.id, output_kind)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| (candidate.layer, candidate.interval.start, candidate.id));
    candidates
}

const fn media_output_kind(data_type: PortDataType) -> Option<MediaOutputKind> {
    match data_type {
        PortDataType::Image => Some(MediaOutputKind::Image),
        PortDataType::Audio => Some(MediaOutputKind::Audio),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{RationalRate, SourceRef, TimelineInterval, TimelineTrackId};
    use library::model::frame::color::Color;

    #[test]
    fn candidates_stay_in_the_timeline_and_match_the_published_media_type() {
        let mut project = AuthoringProject::new(
            "picker",
            320,
            180,
            RationalRate::new(30, 1).unwrap(),
            library::model::authoring::MediaTime::from_whole_seconds(10),
        )
        .unwrap();
        let timeline_id = project.root_timeline_id;
        let visual_track = project.timelines[&timeline_id].track_order[0];
        let make = |track_id, name: &str, layer| TimelineItem {
            id: TimelineItemId::new(),
            track_id,
            name: name.to_string(),
            source: SourceRef::Solid {
                color: Color::white(),
            },
            interval: TimelineInterval::new(
                library::model::authoring::MediaTime::zero(),
                library::model::authoring::MediaTime::from_whole_seconds(2),
            )
            .unwrap(),
            time_map: Default::default(),
            layer,
            parent: None,
            blend_mode: Default::default(),
            authored_properties: Default::default(),
        };
        let included = make(visual_track, "included", 1);
        let excluded = make(visual_track, "excluded", 0);
        let wrong_timeline = make(TimelineTrackId::new(), "wrong timeline", 2);
        let included_id = included.id;
        let excluded_id = excluded.id;
        project.items.insert(included.id, included);
        project.items.insert(excluded.id, excluded);
        project.items.insert(wrong_timeline.id, wrong_timeline);

        let candidates = input_candidates(
            &project,
            timeline_id,
            MediaOutputKind::Image,
            &[excluded_id],
        );

        assert_eq!(
            candidates.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![included_id]
        );
        assert!(
            input_candidates(&project, timeline_id, MediaOutputKind::Audio, &[]).is_empty(),
            "solid items cannot satisfy an Audio Published input"
        );
    }
}
