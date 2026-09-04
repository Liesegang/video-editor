//! Shared editor control for binding a Published media input to a Timeline item.
//!
//! Module hosts persist only Published Interface IDs. This picker deliberately
//! has no knowledge of Module-internal Node UUIDs.

use egui_phosphor::regular as icons;
use library::model::authoring::{
    AuthoringProject, InstanceLocator, ItemOutputStage, MediaInputBinding, MediaOutputKind,
    PublishedMediaInput, TimelineId, TimelineInterval, TimelineItem, TimelineItemId,
};
use library::model::project::PortDataType;

pub(crate) enum MediaInputPickerAction {
    Bind(MediaInputBinding),
    Unbind,
    Inherit,
}

pub(crate) struct MediaInputPicker<'a> {
    pub control_id: &'a str,
    pub project: &'a AuthoringProject,
    pub timeline_id: TimelineId,
    pub input: &'a PublishedMediaInput,
    pub current: Option<&'a MediaInputBinding>,
    pub excluded_items: &'a [TimelineItemId],
    /// When present, required inputs expose only sources active for this full
    /// host interval. General Module and Effect hosts leave it unset.
    pub required_coverage: Option<TimelineInterval>,
    /// A placement-local decision exists and can be removed to reveal its
    /// definition-scope binding again.
    pub can_inherit: bool,
}

pub(crate) fn media_input_picker(
    ui: &mut egui::Ui,
    picker: MediaInputPicker<'_>,
) -> Option<MediaInputPickerAction> {
    let output_kind = media_output_kind(picker.input.data_type)?;
    let candidates = input_candidates(
        picker.project,
        picker.timeline_id,
        picker.input,
        picker.excluded_items,
        picker.required_coverage,
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
            crate::ui::panels::inspector::property_authoring::property_label(
                ui,
                picker.control_id,
                &picker.input.name,
            );
            let combo = egui::ComboBox::from_id_salt(("published-media-input", picker.control_id))
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    let unbound = ui.selectable_label(picker.current.is_none(), "Unbound");
                    crate::qa::register_component_with_metadata(
                        format!("{}.unbound", picker.control_id),
                        "published_media_input_choice",
                        unbound.rect,
                        unbound.enabled(),
                        Some(serde_json::json!({
                            "published_input_id": picker.input.id,
                            "item_id": null,
                            "selected": picker.current.is_none(),
                        })),
                    );
                    if unbound.clicked() && picker.current.is_some() {
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
                        let candidate = ui.selectable_label(selected, &source.name);
                        crate::qa::register_component_with_metadata(
                            format!("{}.candidate:{}", picker.control_id, source.id),
                            "published_media_input_choice",
                            candidate.rect,
                            candidate.enabled(),
                            Some(serde_json::json!({
                                "published_input_id": picker.input.id,
                                "item_id": source.id,
                                "selected": selected,
                            })),
                        );
                        if candidate.clicked() {
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
            crate::qa::register_component_with_metadata(
                format!("{}.selector", picker.control_id),
                "published_media_input_selector",
                combo.response.rect,
                combo.response.enabled(),
                Some(serde_json::json!({
                    "published_input_id": picker.input.id,
                    "required": picker.input.required,
                    "bound_item_id": current_item_id,
                    "candidate_count": candidates.len(),
                })),
            );
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
            "candidate_item_ids": candidates.iter().map(|item| item.id).collect::<Vec<_>>(),
            "required": picker.input.required,
            "can_inherit": picker.can_inherit,
        })),
    );
    if picker.input.required && picker.current.is_none() {
        ui.colored_label(ui.visuals().warn_fg_color, "A clip input is required");
    }
    action
}

pub(crate) fn input_candidates<'a>(
    project: &'a AuthoringProject,
    timeline_id: TimelineId,
    input: &PublishedMediaInput,
    excluded_items: &[TimelineItemId],
    required_coverage: Option<TimelineInterval>,
) -> Vec<&'a TimelineItem> {
    let Some(output_kind) = media_output_kind(input.data_type) else {
        return Vec::new();
    };
    let mut candidates = project
        .items
        .values()
        .filter(|candidate| !excluded_items.contains(&candidate.id))
        .filter(|candidate| {
            project
                .validate_published_media_binding(
                    None,
                    timeline_id,
                    input,
                    &MediaInputBinding::TimelineItemOutput {
                        locator: InstanceLocator::SameTimeline,
                        item_id: candidate.id,
                        output: output_kind,
                        stage: match output_kind {
                            MediaOutputKind::Image => ItemOutputStage::PostTransform,
                            MediaOutputKind::Audio => ItemOutputStage::PostEffects,
                        },
                    },
                    required_coverage,
                )
                .is_ok()
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| (candidate.layer, candidate.interval.start, candidate.id));
    candidates
}

pub(crate) fn has_compatible_input_candidate(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    input: &PublishedMediaInput,
    excluded_items: &[TimelineItemId],
    required_coverage: Option<TimelineInterval>,
) -> bool {
    !input_candidates(
        project,
        timeline_id,
        input,
        excluded_items,
        required_coverage,
    )
    .is_empty()
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
    use library::model::authoring::{
        ModulePortAddress, PublishedMediaInputId, RationalRate, SourceRef, TimelineInterval,
        TimelineTrackId,
    };
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
        let image_input = PublishedMediaInput {
            id: PublishedMediaInputId::new(),
            name: "Image".to_string(),
            data_type: PortDataType::Image,
            target: ModulePortAddress {
                node_id: uuid::Uuid::new_v4(),
                port: "image".to_string(),
            },
            required: false,
            primary: false,
        };
        let audio_input = PublishedMediaInput {
            data_type: PortDataType::Audio,
            ..image_input.clone()
        };

        let candidates =
            input_candidates(&project, timeline_id, &image_input, &[excluded_id], None);

        assert_eq!(
            candidates.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![included_id]
        );
        assert!(
            input_candidates(&project, timeline_id, &audio_input, &[], None).is_empty(),
            "solid items cannot satisfy an Audio Published input"
        );
    }

    #[test]
    fn required_coverage_hides_partial_sources_but_keeps_full_sources() {
        let mut project = AuthoringProject::new(
            "covered picker",
            320,
            180,
            RationalRate::new(30, 1).unwrap(),
            library::model::authoring::MediaTime::from_whole_seconds(10),
        )
        .unwrap();
        let timeline_id = project.root_timeline_id;
        let track_id = project.timelines[&timeline_id].track_order[0];
        let make = |name: &str, interval| TimelineItem {
            id: TimelineItemId::new(),
            track_id,
            name: name.to_string(),
            source: SourceRef::Solid {
                color: Color::white(),
            },
            interval,
            time_map: Default::default(),
            layer: 0,
            parent: None,
            blend_mode: Default::default(),
            authored_properties: Default::default(),
        };
        let partial = make(
            "partial",
            TimelineInterval::new(
                library::model::authoring::MediaTime::from_whole_seconds(3),
                library::model::authoring::MediaTime::from_whole_seconds(3),
            )
            .unwrap(),
        );
        let full = make(
            "full",
            TimelineInterval::new(
                library::model::authoring::MediaTime::from_whole_seconds(3),
                library::model::authoring::MediaTime::from_whole_seconds(4),
            )
            .unwrap(),
        );
        let full_id = full.id;
        project.items.insert(partial.id, partial);
        project.items.insert(full.id, full);
        let input = PublishedMediaInput {
            id: PublishedMediaInputId::new(),
            name: "Matte".to_string(),
            data_type: PortDataType::Image,
            target: ModulePortAddress {
                node_id: uuid::Uuid::new_v4(),
                port: "image".to_string(),
            },
            required: true,
            primary: false,
        };
        let transition_interval = TimelineInterval::new(
            library::model::authoring::MediaTime::from_whole_seconds(3),
            library::model::authoring::MediaTime::from_whole_seconds(4),
        )
        .unwrap();

        let candidates = input_candidates(
            &project,
            timeline_id,
            &input,
            &[],
            Some(transition_interval),
        );

        assert_eq!(
            candidates.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![full_id]
        );
    }
}
