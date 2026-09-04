use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as icons;
use library::editor::{TimelineEditorService, TransitionPlacement};
use library::model::authoring::{
    AuthoringProject, MediaOutputKind, ModuleDefinition, ModuleDefinitionSharing, TimelineItem,
    TimelineItemId, TimelineTrackId, Transition, TransitionCreationCandidate, TransitionMediaType,
};

use crate::state::authoring::AuthoringUiState;

use super::viewport::seconds_to_screen_x;
use super::DeferredItemAction;

pub(super) fn add_transition_menu(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    item: &TimelineItem,
    actions: &mut Vec<DeferredItemAction>,
) {
    let Ok(candidates) = project.transition_creation_candidates(item.id) else {
        return;
    };
    if candidates.is_empty() {
        return;
    }

    let menu = ui.menu_button(format!("{} Add Transition", icons::PLUS), |ui| {
        for candidate in &candidates {
            add_processor_action(ui, project, *candidate, actions);
        }
    });
    crate::qa::register_component_with_metadata(
        format!("timeline.item.add_transition_menu:{}", item.id),
        "timeline_context_submenu",
        menu.response.rect,
        menu.response.enabled(),
        Some(serde_json::json!({
            "item_id": item.id,
            "candidate_count": candidates.len(),
        })),
    );
}

fn add_processor_action(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    candidate: TransitionCreationCandidate,
    actions: &mut Vec<DeferredItemAction>,
) {
    let (icon, processor_name) = match candidate.output {
        MediaOutputKind::Image => (icons::IMAGE, "Cross Dissolve"),
        MediaOutputKind::Audio => (icons::WAVEFORM, "Audio Crossfade"),
    };
    let target_name = project
        .items
        .get(&candidate.to_item_id)
        .map_or("Missing item", |item| item.name.as_str());
    let response = ui.button(format!("{icon} {processor_name} → {target_name}"));
    crate::qa::register_component_with_metadata(
        format!(
            "timeline.item.add_transition:{}:{}:{}",
            candidate.from_item_id,
            candidate.to_item_id,
            output_name(candidate.output)
        ),
        "timeline_context_menu_action",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "from_item_id": candidate.from_item_id,
            "to_item_id": candidate.to_item_id,
            "output": output_name(candidate.output),
            "processor": processor_name,
            "edit_point_seconds": candidate.edit_point.to_seconds_f64(),
            "duration_seconds": candidate.duration.to_seconds_f64(),
        })),
    );
    if response.clicked() {
        actions.push(DeferredItemAction::AddTransition(candidate));
        ui.close();
    }
}

pub(super) fn add_creation_candidate(
    service: &TimelineEditorService,
    candidate: TransitionCreationCandidate,
) -> Result<(), library::LibraryError> {
    service
        .add_transition(TransitionPlacement {
            from_item_id: candidate.from_item_id,
            to_item_id: candidate.to_item_id,
            edit_point: candidate.edit_point,
            duration: candidate.duration,
            alignment: candidate.alignment,
            processor: candidate.processor(),
            parameters: Default::default(),
        })
        .map(|_| ())
}

#[allow(
    clippy::too_many_arguments,
    reason = "Transition overlays combine authored topology, current selection, track filtering, clipped timeline geometry, and deferred UI actions"
)]
pub(super) fn paint_track_transitions(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    track_id: TimelineTrackId,
    target_item_id: Option<TimelineItemId>,
    row_rect: Rect,
    content_rect: Rect,
    actions: &mut Vec<DeferredItemAction>,
) {
    let mut transitions = project
        .transitions
        .values()
        .filter(|transition| {
            project
                .items
                .get(&transition.from_item_id)
                .is_some_and(|item| item.track_id == track_id)
                && target_item_id.is_none_or(|item_id| transition.to_item_id == item_id)
        })
        .collect::<Vec<_>>();
    transitions.sort_by_key(|transition| (transition.edit_point, transition.id));

    for transition in transitions {
        let Ok(interval) = transition.interval() else {
            continue;
        };
        let left = seconds_to_screen_x(
            interval.start.to_seconds_f64() as f32,
            content_rect,
            &state.timeline,
        );
        let width = (interval.duration.to_seconds_f64() as f32 * state.timeline.pixels_per_second)
            .max(10.0);
        let output = transition.processor.contract.media_type.output_kind();
        let shares_pair_with_other_media = project.transitions.values().any(|candidate| {
            candidate.id != transition.id
                && candidate.from_item_id == transition.from_item_id
                && candidate.to_item_id == transition.to_item_id
                && candidate.processor.contract.media_type.output_kind() != output
        });
        let available_height = (row_rect.height() - 8.0).max(8.0);
        let lane_height = if shares_pair_with_other_media {
            (available_height * 0.5).max(4.0)
        } else {
            available_height
        };
        let lane_offset = if shares_pair_with_other_media && output == MediaOutputKind::Audio {
            lane_height
        } else {
            0.0
        };
        let rect = Rect::from_min_size(
            Pos2::new(left, row_rect.top() + 4.0 + lane_offset),
            Vec2::new(width, lane_height),
        );
        let visible = rect.intersect(content_rect);
        if !visible.is_positive() {
            continue;
        }
        let response = ui.interact(
            visible,
            ui.id()
                .with(("timeline-transition", transition.id, target_item_id)),
            Sense::click(),
        );
        if response.clicked() {
            state
                .selection
                .replace(crate::state::authoring::AuthoringSelection::Transition(
                    transition.id,
                ));
        }
        let selected =
            state
                .selection
                .contains(crate::state::authoring::AuthoringSelection::Transition(
                    transition.id,
                ));
        crate::qa::register_component_with_metadata(
            format!("timeline.transition:{}", transition.id),
            "timeline_transition",
            visible,
            true,
            Some(serde_json::json!({
                "transition_id": transition.id,
                "from_item_id": transition.from_item_id,
                "to_item_id": transition.to_item_id,
                "start_seconds": interval.start.to_seconds_f64(),
                "duration_seconds": interval.duration.to_seconds_f64(),
                "edit_point_seconds": transition.edit_point.to_seconds_f64(),
                "output": output_name(output),
                "module_backed": transition.processor.module_processor().is_some(),
                "selected": selected,
            })),
        );
        let hovered = response.hovered();
        response.clone().on_hover_text(format!(
            "{} · {:.3} s",
            processor_label(project, transition),
            interval.duration.to_seconds_f64()
        ));
        response.context_menu(|ui| {
            let edit_logic = ui.button(format!(
                "{} Edit Transition Logic",
                icons::SHARE_NETWORK
            ));
            crate::qa::register_component(
                format!("timeline.transition.edit_logic:{}", transition.id),
                "timeline_context_menu_action",
                edit_logic.rect,
            );
            if edit_logic
                .on_hover_text(
                    "Open a finite A / B / Progress processing Module; clips and Timeline remain outside the Node graph",
                )
                .clicked()
            {
                actions.push(DeferredItemAction::EditTransitionLogic(transition.id));
                ui.close();
            }
            transition_processor_menu(ui, project, transition, actions);
            ui.separator();
            let remove = ui.button(format!("{} Remove Transition", icons::TRASH));
            crate::qa::register_component(
                format!("timeline.transition.remove:{}", transition.id),
                "timeline_context_menu_action",
                remove.rect,
            );
            if remove.clicked() {
                actions.push(DeferredItemAction::RemoveTransition(transition.id));
                ui.close();
            }
        });

        let painter = ui.painter().with_clip_rect(content_rect);
        let fill = if selected {
            Color32::from_rgba_unmultiplied(91, 178, 255, 190)
        } else if hovered {
            Color32::from_rgba_unmultiplied(74, 154, 255, 150)
        } else {
            Color32::from_rgba_unmultiplied(57, 119, 210, 118)
        };
        painter.rect_filled(rect, 3.0, fill);
        painter.rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0, Color32::from_rgb(135, 195, 255)),
            StrokeKind::Inside,
        );
        painter.line_segment(
            [rect.left_bottom(), rect.right_top()],
            Stroke::new(1.0, Color32::from_white_alpha(170)),
        );
        painter.line_segment(
            [rect.left_top(), rect.right_bottom()],
            Stroke::new(1.0, Color32::from_white_alpha(90)),
        );
    }
}

fn transition_processor_menu(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    transition: &Transition,
    actions: &mut Vec<DeferredItemAction>,
) {
    let module_choices =
        transition_module_choices(project, transition.processor.contract.media_type);
    let compatible_count = module_choices.len();
    let assignable_module_count = module_choices
        .iter()
        .filter(|choice| choice.assignment_error.is_none())
        .count();
    let current_definition_id = transition
        .processor
        .module_processor()
        .and_then(|module| project.module_instances.get(&module.instance_id))
        .map(|instance| instance.definition_id);
    let label = format!("{} Processor", icons::SLIDERS_HORIZONTAL);
    let menu = ui.menu_button(label, |ui| {
        let builtin_label = match transition.processor.contract.media_type {
            TransitionMediaType::Image => "Cross Dissolve",
            TransitionMediaType::Audio => "Audio Crossfade",
        };
        let builtin_active = current_definition_id.is_none();
        let builtin = ui.selectable_label(
            builtin_active,
            format!(
                "{} {builtin_label}",
                if builtin_active {
                    icons::CHECK
                } else {
                    icons::ARROWS_MERGE
                }
            ),
        );
        crate::qa::register_component_with_metadata(
            format!("timeline.transition.assign_builtin:{}", transition.id),
            "timeline_transition_processor_choice",
            builtin.rect,
            !builtin_active,
            Some(serde_json::json!({
                "transition_id": transition.id,
                "media_type": transition_media_name(transition.processor.contract.media_type),
            })),
        );
        if !builtin_active && builtin.clicked() {
            actions.push(DeferredItemAction::AssignBuiltinTransition(transition.id));
            ui.close();
        }

        if !module_choices.is_empty() {
            ui.separator();
            ui.weak("Reusable Transition Modules");
        }
        for module_choice in module_choices {
            let definition = module_choice.definition;
            let active = current_definition_id == Some(definition.id);
            let assignable = module_choice.assignment_error.is_none();
            let choice = ui.add_enabled(
                active || assignable,
                egui::Button::selectable(
                    active,
                    format!(
                        "{} {}",
                        if active {
                            icons::CHECK
                        } else if assignable {
                            icons::SHARE_NETWORK
                        } else {
                            icons::LOCK
                        },
                        definition.name
                    ),
                ),
            );
            crate::qa::register_component_with_metadata(
                format!(
                    "timeline.transition.assign_module:{}:{}",
                    transition.id, definition.id
                ),
                "timeline_transition_processor_choice",
                choice.rect,
                choice.enabled(),
                Some(serde_json::json!({
                    "transition_id": transition.id,
                    "definition_id": definition.id,
                    "media_type": transition_media_name(transition.processor.contract.media_type),
                    "assignable": assignable,
                    "assignment_error": module_choice.assignment_error.as_deref(),
                })),
            );
            if let Some(reason) = &module_choice.assignment_error {
                choice.clone().on_hover_text(reason);
                ui.weak(reason);
            }
            if !active && assignable && choice.clicked() {
                actions.push(DeferredItemAction::AssignTransitionModule {
                    transition_id: transition.id,
                    definition_id: definition.id,
                });
                ui.close();
            }
        }
    });
    crate::qa::register_component_with_metadata(
        format!("timeline.transition.processor_menu:{}", transition.id),
        "timeline_context_submenu",
        menu.response.rect,
        menu.response.enabled(),
        Some(serde_json::json!({
            "transition_id": transition.id,
            "compatible_module_count": compatible_count,
            "assignable_module_count": assignable_module_count,
            "media_type": transition_media_name(transition.processor.contract.media_type),
        })),
    );
}

struct TransitionModuleChoice<'a> {
    definition: &'a ModuleDefinition,
    /// A reusable Transition can be structurally valid yet require another
    /// Timeline input. It stays visible until the assignment form can bind
    /// that input atomically, rather than disappearing from the chooser.
    assignment_error: Option<String>,
}

fn transition_module_choices(
    project: &AuthoringProject,
    media_type: TransitionMediaType,
) -> Vec<TransitionModuleChoice<'_>> {
    let mut definitions = project
        .module_definitions
        .values()
        .filter(|definition| {
            matches!(
                definition.sharing,
                ModuleDefinitionSharing::ReusableTemplate(_)
            ) && definition
                .host_contract
                .transition()
                .is_some_and(|contract| contract.media_type == media_type)
        })
        .map(|definition| TransitionModuleChoice {
            assignment_error: definition
                .host_contract
                .transition()
                .and_then(|contract| contract.validate_atomic_assignment(definition).err()),
            definition,
        })
        .collect::<Vec<_>>();
    definitions.sort_by(|left, right| {
        left.definition
            .name
            .cmp(&right.definition.name)
            .then(left.definition.id.cmp(&right.definition.id))
    });
    definitions
}

const fn transition_media_name(media_type: TransitionMediaType) -> &'static str {
    match media_type {
        TransitionMediaType::Image => "image",
        TransitionMediaType::Audio => "audio",
    }
}

const fn output_name(output: MediaOutputKind) -> &'static str {
    match output {
        MediaOutputKind::Image => "image",
        MediaOutputKind::Audio => "audio",
    }
}

fn processor_label(
    project: &AuthoringProject,
    transition: &library::model::authoring::Transition,
) -> String {
    if let Some(definition) = transition
        .processor
        .module_processor()
        .and_then(|module| project.module_instances.get(&module.instance_id))
        .and_then(|instance| project.module_definitions.get(&instance.definition_id))
    {
        return definition.name.clone();
    }
    match transition.processor.contract.media_type.output_kind() {
        MediaOutputKind::Image => "Cross Dissolve".to_string(),
        MediaOutputKind::Audio => "Audio Crossfade".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{
        MediaTime, ModulePortAddress, ModuleTemplateOrigin, PublishedMediaInput,
        PublishedMediaInputId, RationalRate,
    };
    use library::model::node::Node;
    use library::model::project::{PortDataType, MERGE_IMAGES_PORT};

    fn seconds(value: i64) -> MediaTime {
        MediaTime::from_whole_seconds(value)
    }

    #[test]
    fn reusable_transition_choices_keep_required_inputs_discoverable() {
        let mut project = AuthoringProject::new(
            "transition templates",
            320,
            180,
            RationalRate::new(30, 1).unwrap(),
            seconds(20),
        )
        .unwrap();
        let reusable = |name, media_type| {
            ModuleDefinition::new_transition(
                name,
                ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
                media_type,
            )
            .unwrap()
            .0
        };
        let image = reusable("B Image", TransitionMediaType::Image);
        let audio = reusable("Audio", TransitionMediaType::Audio);
        let image_first = reusable("A Image", TransitionMediaType::Image);
        let mut required_input = reusable("Required Matte", TransitionMediaType::Image);
        let matte = Node::new_merge("Required Matte Target");
        required_input
            .interface
            .media_inputs
            .push(PublishedMediaInput {
                id: PublishedMediaInputId::new(),
                name: "Matte".to_string(),
                data_type: PortDataType::Image,
                target: ModulePortAddress {
                    node_id: matte.id,
                    port: MERGE_IMAGES_PORT.to_string(),
                },
                required: true,
                primary: false,
            });
        required_input.graph.nodes.insert(matte.id, matte);
        required_input.validate().unwrap();
        let general = ModuleDefinition::new_project_image("Ordinary Node Clip").0;
        let private = ModuleDefinition::new_transition(
            "Private Transition",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .unwrap()
        .0;
        let expected = [image_first.id, image.id, required_input.id];
        for definition in [image, audio, image_first, required_input, general, private] {
            project.module_definitions.insert(definition.id, definition);
        }

        let choices = transition_module_choices(&project, TransitionMediaType::Image);
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.definition.id)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(choices[0].assignment_error.is_none());
        assert!(choices[1].assignment_error.is_none());
        assert!(choices[2]
            .assignment_error
            .as_deref()
            .is_some_and(|reason| reason.contains("cannot be assigned without controls")));
    }
}
