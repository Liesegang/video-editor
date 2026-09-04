use super::*;

pub(super) fn module_effect_controls(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    resources: &EffectStackResources<'_>,
    attachment: &Attachment,
    invocation: &library::model::authoring::ModuleInvocation,
) {
    let Some(instance) = resources
        .project
        .module_instances
        .get(&invocation.instance_id)
    else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing Module instance");
        return;
    };
    let Some(definition) = resources
        .project
        .module_definitions
        .get(&instance.definition_id)
    else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing Module definition");
        return;
    };

    for parameter in &definition.interface.parameters {
        let is_overridden = instance.parameter_overrides.contains_key(&parameter.id);
        let current = instance
            .parameter_overrides
            .get(&parameter.id)
            .unwrap_or(&parameter.default_value);
        let draft_key = format!("module:{}", parameter.id);
        let mut edited = None;
        let mut reset = false;
        ui.horizontal(|ui| {
            property_label(
                ui,
                &format!(
                    "attachment:{}:module_parameter:{}",
                    attachment.id, parameter.id
                ),
                &parameter.name,
            );
            let draft = state
                .inspector
                .effect_values
                .entry((attachment.id, draft_key))
                .or_insert_with(|| current.clone());
            if property_control(
                ui,
                &format!(
                    "attachment:{}:module_parameter:{}",
                    attachment.id, parameter.id
                ),
                draft,
                None,
                "",
                0.1,
            ) {
                edited = Some(draft.clone());
            }
            if is_overridden {
                reset = ui
                    .small_button(icons::ARROW_COUNTER_CLOCKWISE)
                    .on_hover_text("Reset to the Module default")
                    .clicked();
            }
        });
        if let Some(value) = edited {
            match resources
                .service
                .set_module_parameter(instance.id, parameter.id, value)
            {
                Ok(_) => state.status = format!("Updated {}", parameter.name),
                Err(error) => state.error = Some(error.to_string()),
            }
        } else if reset {
            match resources
                .service
                .clear_module_parameter_override(instance.id, parameter.id)
            {
                Ok(_) => state.status = format!("Reset {}", parameter.name),
                Err(error) => state.error = Some(error.to_string()),
            }
        }
        super::super::value_provenance(
            ui,
            invocation.automation_tracks.contains_key(&parameter.id),
            is_overridden,
        );
    }

    let additional_inputs = definition
        .interface
        .media_inputs
        .iter()
        .filter(|input| !input.primary)
        .collect::<Vec<_>>();
    if additional_inputs.is_empty() {
        if definition.interface.parameters.is_empty() {
            ui.weak("Open the Node Editor to add or publish controls.");
        }
        return;
    }

    let Some(timeline_id) = attachment_timeline_id(resources.project, &attachment.owner) else {
        ui.colored_label(ui.visuals().error_fg_color, "Effect owner has no Timeline");
        return;
    };
    ui.add_space(2.0);
    ui.weak("Clip inputs");
    for input in additional_inputs {
        let Some(output_kind) = media_output_kind(input.data_type) else {
            continue;
        };
        let mut candidates = resources
            .project
            .items
            .values()
            .filter(|candidate| {
                resources
                    .project
                    .tracks
                    .get(&candidate.track_id)
                    .is_some_and(|track| track.timeline_id == timeline_id)
            })
            .filter(|candidate| {
                !matches!(
                    &attachment.owner,
                    AttachmentOwner::Item { item_id } if *item_id == candidate.id
                )
            })
            .filter(|candidate| item_supports_output(resources.project, candidate, output_kind))
            .collect::<Vec<_>>();
        candidates
            .sort_by_key(|candidate| (candidate.layer, candidate.interval.start, candidate.id));

        let current = invocation.input_bindings.get(&input.id);
        let current_item_id = current.map(|binding| {
            let MediaInputBinding::TimelineItemOutput { item_id, .. } = binding;
            *item_id
        });
        let current_label = current_item_id
            .and_then(|item_id| resources.project.items.get(&item_id))
            .map_or_else(
                || {
                    if input.required {
                        "Choose clip...".to_string()
                    } else {
                        "Unbound".to_string()
                    }
                },
                |item| {
                    if matches!(
                        current,
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

        ui.horizontal(|ui| {
            property_label(
                ui,
                &format!("attachment:{}:module_input:{}", attachment.id, input.id),
                &input.name,
            );
            egui::ComboBox::from_id_salt(("attachment-module-input", attachment.id, input.id))
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(current.is_none(), "Unbound").clicked()
                        && current.is_some()
                    {
                        match resources
                            .service
                            .unbind_attachment_module_input(attachment.id, input.id)
                        {
                            Ok(_) => state.status = format!("Unbound {}", input.name),
                            Err(error) => state.error = Some(error.to_string()),
                        }
                        ui.close();
                    }
                    for source in &candidates {
                        let selected = matches!(
                            current,
                            Some(MediaInputBinding::TimelineItemOutput {
                                locator: InstanceLocator::SameTimeline,
                                item_id,
                                ..
                            }) if *item_id == source.id
                        );
                        if ui.selectable_label(selected, &source.name).clicked() {
                            let binding = MediaInputBinding::TimelineItemOutput {
                                locator: InstanceLocator::SameTimeline,
                                item_id: source.id,
                                output: output_kind,
                                stage: match output_kind {
                                    MediaOutputKind::Image => ItemOutputStage::PostTransform,
                                    MediaOutputKind::Audio => ItemOutputStage::PostEffects,
                                },
                            };
                            match resources.service.bind_attachment_module_input(
                                attachment.id,
                                input.id,
                                binding,
                            ) {
                                Ok(_) => {
                                    state.status =
                                        format!("Bound {} to {}", source.name, input.name)
                                }
                                Err(error) => state.error = Some(error.to_string()),
                            }
                            ui.close();
                        }
                    }
                });
        });
        if input.required && current.is_none() {
            ui.colored_label(ui.visuals().warn_fg_color, "A clip input is required");
        }
    }
    ui.weak("Bindings use published inputs; internal Node IDs remain private.");
}

fn attachment_timeline_id(
    project: &AuthoringProject,
    owner: &AttachmentOwner,
) -> Option<TimelineId> {
    match owner {
        AttachmentOwner::Timeline { timeline_id } => Some(*timeline_id),
        AttachmentOwner::Track { track_id } => {
            project.tracks.get(track_id).map(|track| track.timeline_id)
        }
        AttachmentOwner::Item { item_id } => project
            .items
            .get(item_id)
            .and_then(|item| project.tracks.get(&item.track_id))
            .map(|track| track.timeline_id),
    }
}

const fn media_output_kind(data_type: PortDataType) -> Option<MediaOutputKind> {
    match data_type {
        PortDataType::Image => Some(MediaOutputKind::Image),
        PortDataType::Audio => Some(MediaOutputKind::Audio),
        _ => None,
    }
}

fn item_supports_output(
    project: &AuthoringProject,
    item: &TimelineItem,
    output: MediaOutputKind,
) -> bool {
    match &item.source {
        SourceRef::Asset { asset_id } => project
            .assets
            .iter()
            .find(|asset| asset.id == *asset_id)
            .is_some_and(|asset| match output {
                MediaOutputKind::Image => {
                    matches!(asset.kind, AssetKind::Image | AssetKind::Video)
                }
                MediaOutputKind::Audio => {
                    matches!(asset.kind, AssetKind::Audio | AssetKind::Video)
                }
            }),
        SourceRef::Text { .. } | SourceRef::Shape { .. } | SourceRef::Solid { .. } => {
            output == MediaOutputKind::Image
        }
        SourceRef::Composition(instance) => match output {
            MediaOutputKind::Image => true,
            MediaOutputKind::Audio => project.tracks.values().any(|track| {
                track.timeline_id == instance.timeline_id
                    && track.kind != library::model::authoring::TimelineTrackKind::Visual
            }),
        },
        SourceRef::Module(invocation) => project
            .module_instances
            .get(&invocation.instance_id)
            .and_then(|instance| project.module_definitions.get(&instance.definition_id))
            .and_then(|definition| definition.output(invocation.output_id))
            .is_some_and(|candidate| {
                candidate.supports(match output {
                    MediaOutputKind::Image => PortDataType::Image,
                    MediaOutputKind::Audio => PortDataType::Audio,
                })
            }),
    }
}
