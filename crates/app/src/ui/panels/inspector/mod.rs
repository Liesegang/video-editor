mod asset_preview;
mod audio;
mod composition_parameters;
mod effect_stack;
mod module_media_input;
mod property_authoring;
mod text_ensemble;
mod timing;
mod transition;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use egui_phosphor::regular as icons;
use library::editor::{AuthoringPropertyOwner, AuthoringWaveformService, TimelineEditorService};
use library::model::authoring::{
    AttachmentOwner, AttachmentStage, AuthoringProject, InstanceLocator, ItemOutputStage,
    MediaInputBinding, MediaOutputKind, MediaTime, SourceRef, TimelineItem,
};
use library::model::property::{PropertyValue, Vec2};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;

use crate::state::authoring::{AuthoringSelection, AuthoringUiState};
use crate::state::node_editor::{ModuleEditorHost, NodeEditorDocument};
use crate::ui::media_preview::AuthoringMediaPreviewService;
use crate::ui::widgets::blend_mode_picker::blend_mode_picker;
use crate::ui::widgets::property_mode::{
    PropertyAuthoringMode, PropertyModeAction, PropertyModeState,
};

use property_authoring::{property_control, property_label, property_row, PropertyRowSpec};

pub fn inspector_panel(
    ui: &mut egui::Ui,
    project: &Arc<AuthoringProject>,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) {
    let selection = state.selection.primary();
    let revision = match service.revision() {
        Ok(revision) => revision,
        Err(error) => {
            state.error = Some(error.to_string());
            return;
        }
    };
    sync_draft(project, state, selection, revision);
    let Some(selection) = selection else {
        empty_inspector(ui);
        return;
    };

    crate::qa::register_component(
        "inspector.scroll_area",
        "inspector_scroll_area",
        ui.available_rect_before_wrap(),
    );
    egui::ScrollArea::vertical()
        .id_salt("inspector.scroll")
        .show(ui, |ui| match selection {
            AuthoringSelection::Timeline(id) => {
                if let Some(timeline) = project.timelines.get(&id) {
                    section_title(ui, icons::FILM_STRIP, "Timeline", &timeline.name, None);
                    editable_name(ui, state, &timeline.name, |name| {
                        service.update_timeline_settings(
                            id,
                            library::editor::TimelineSettingsUpdate {
                                name: Some(name),
                                ..Default::default()
                            },
                        )
                    });
                    ui.separator();
                    egui::Grid::new("timeline_settings_grid")
                        .num_columns(2)
                        .spacing([10.0, 7.0])
                        .show(ui, |ui| {
                            ui.label("Size");
                            ui.label(format!("{} × {}", timeline.width, timeline.height));
                            ui.end_row();
                            ui.label("Frame rate");
                            ui.label(format!("{:.3} fps", timeline.fps.to_f64()));
                            ui.end_row();
                            ui.label("Duration");
                            ui.label(format!("{:.3} s", timeline.duration.to_seconds_f64()));
                            ui.end_row();
                        });
                    effect_stack::effect_stack(
                        ui,
                        project,
                        state,
                        service,
                        plugins,
                        AttachmentOwner::Timeline { timeline_id: id },
                        &[
                            AttachmentStage::TimelinePostComposite,
                            AttachmentStage::TimelinePostMix,
                        ],
                    );
                }
            }
            AuthoringSelection::Track(id) => {
                if let Some(track) = project.tracks.get(&id) {
                    section_title(ui, icons::STACK, "Track", &track.name, None);
                    editable_name(ui, state, &track.name, |name| {
                        service.rename_track(id, name)
                    });
                    ui.separator();
                    ui.label(format!("Type: {}", track_kind_name(track.kind)));
                    effect_stack::effect_stack(
                        ui,
                        project,
                        state,
                        service,
                        plugins,
                        AttachmentOwner::Track { track_id: id },
                        &[
                            AttachmentStage::TrackPostComposite,
                            AttachmentStage::TrackPostMix,
                        ],
                    );
                }
            }
            AuthoringSelection::Item(id) => {
                if let Some(item) = project.items.get(&id) {
                    item_inspector(ui, project, state, service, plugins, item);
                }
            }
            AuthoringSelection::Transition(id) => {
                transition::transition_inspector(ui, project, state, service, id);
            }
            AuthoringSelection::Asset(id) => {
                if let Some(asset) = project.assets.iter().find(|asset| asset.id == id) {
                    asset_preview::asset_inspector(ui, project, asset, waveform, media_previews);
                }
            }
            AuthoringSelection::ModuleDefinition(id) => {
                if let Some(definition) = project.module_definitions.get(&id) {
                    section_title(
                        ui,
                        icons::SHARE_NETWORK,
                        "Node Clip template",
                        &definition.name,
                        None,
                    );
                    ui.separator();
                    ui.label(format!("{} nodes", definition.graph.nodes.len()));
                    ui.label(format!(
                        "{} published parameters",
                        definition.interface.parameters.len()
                    ));
                    ui.weak("Drag this template from Assets to create an independent instance.");
                }
            }
        });
}

fn empty_inspector(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        ui.label(egui::RichText::new(icons::CURSOR).size(24.0).weak());
        ui.label("Select a clip, track, or composition");
    });
}

fn section_title(
    ui: &mut egui::Ui,
    icon: &str,
    kind: &str,
    name: &str,
    action: Option<(&str, &str)>,
) -> Option<egui::Response> {
    let mut action_response = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icon).size(20.0));
        ui.vertical(|ui| {
            ui.weak(kind);
            ui.label(egui::RichText::new(name).strong());
        });
        if let Some((action_icon, tooltip)) = action {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                action_response = Some(ui.small_button(action_icon).on_hover_text(tooltip));
            });
        }
    });
    ui.add_space(6.0);
    action_response
}

fn editable_name(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    model_name: &str,
    commit: impl FnOnce(String) -> Result<library::model::authoring::ChangeSet, library::LibraryError>,
) {
    ui.horizontal(|ui| {
        ui.label("Name");
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.inspector.name).desired_width(f32::INFINITY),
        );
        if (response.lost_focus() || ui.input(|input| input.key_pressed(egui::Key::Enter)))
            && !state.inspector.name.trim().is_empty()
            && state.inspector.name != model_name
        {
            if let Err(error) = commit(state.inspector.name.trim().to_string()) {
                state.error = Some(error.to_string());
            }
        }
    });
}

fn item_inspector(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    item: &TimelineItem,
) {
    let open_node_clip = section_title(
        ui,
        item_icon(item),
        item_kind(item),
        &item.name,
        matches!(&item.source, SourceRef::Module(_))
            .then_some((icons::SHARE_NETWORK, "Open in Node Editor")),
    );
    if let Some(response) = open_node_clip {
        crate::qa::register_component_with_metadata(
            "inspector.node_clip.open_editor",
            "inspector_action",
            response.rect,
            response.enabled(),
            Some(serde_json::json!({
                "item_id": item.id,
                "action": "open_node_editor",
            })),
        );
        if response.clicked() {
            request_node_clip_document(project, state, item);
        }
    }
    editable_name(ui, state, &item.name, |name| {
        service.rename_item(item.id, name)
    });
    ui.separator();

    timing::timing_section(ui, state, service, item);
    if audio::item_is_audio_asset(project, item) {
        audio::audio_section(ui, project, state, service, item);
    } else {
        transform_section(ui, project, state, service, item);
        blend_section(ui, state, service, item);
    }

    if let SourceRef::Text {
        text,
        ensemble_operations,
    } = &item.source
    {
        ui.separator();
        egui::CollapsingHeader::new("Text")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.weak("Content");
                    composition_parameters::publication_icon(
                        ui,
                        project,
                        state,
                        service,
                        item,
                        composition_parameters::PublicationSpec {
                            target:
                                library::model::authoring::CompositionParameterTarget::TextContent {
                                    item_id: item.id,
                                },
                            default_value: PropertyValue::String(text.clone()),
                            suggested_name: format!("{} Text", item.name),
                        },
                    );
                });
                let response = ui.add(
                    egui::TextEdit::multiline(&mut state.inspector.text)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY),
                );
                if response.lost_focus() && state.inspector.text != *text {
                    if let Err(error) = service.set_text(item.id, state.inspector.text.clone()) {
                        state.error = Some(error.to_string());
                    }
                }
            });
        text_ensemble::text_ensemble_section(
            ui,
            project,
            state,
            service,
            plugins,
            item,
            ensemble_operations,
        );
    }

    if let SourceRef::Composition(instance) = &item.source {
        ui.separator();
        egui::CollapsingHeader::new("Nested composition")
            .default_open(true)
            .show(ui, |ui| {
                if let Some(timeline) = project.timelines.get(&instance.timeline_id) {
                    ui.label(&timeline.name);
                }
                ui.label(format!("Duration policy: {}", duration_policy_name(&instance.duration_policy)));
                ui.weak("Animation uses local composition time; moving this clip does not move its inner keys.");
            });
        composition_parameters::instance_parameters(ui, project, state, service, item, instance);
    }

    if let SourceRef::Module(invocation) = &item.source {
        module_parameters(ui, project, state, service, item, invocation);
    }

    effect_stack::effect_stack(
        ui,
        project,
        state,
        service,
        plugins,
        AttachmentOwner::Item { item_id: item.id },
        &[
            AttachmentStage::ItemPreTransform,
            AttachmentStage::ItemPostTransform,
            AttachmentStage::AudioPreFader,
            AttachmentStage::AudioPostFader,
        ],
    );
}

fn blend_section(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
) {
    ui.horizontal(|ui| {
        ui.weak("Blend");
        if let Some(blend_mode) = blend_mode_picker(ui, item.id, item.blend_mode) {
            match service.set_item_blend_mode(item.id, blend_mode) {
                Ok(_) => state.status = format!("Blend: {}", blend_mode.label()),
                Err(error) => state.error = Some(error.to_string()),
            }
        }
    });
}

fn request_node_clip_document(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    item: &TimelineItem,
) {
    let SourceRef::Module(invocation) = &item.source else {
        return;
    };
    let Some(instance) = project.module_instances.get(&invocation.instance_id) else {
        state.error = Some("The Node Clip instance is missing".to_string());
        return;
    };
    state
        .node_editor
        .request_document(NodeEditorDocument::ModuleDefinition {
            definition_id: instance.definition_id,
            host: ModuleEditorHost::NodeClip {
                timeline_item_id: item.id,
                instance_path: state.active_instance_path.clone(),
                module_instance_id: instance.id,
            },
        });
    state.status = format!("Opened {} in Node Editor", item.name);
}

fn transform_section(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
) {
    egui::CollapsingHeader::new("Transform")
        .default_open(true)
        .show(ui, |ui| {
            for (key, label, default, suffix, speed) in [
                (
                    "position",
                    "Position",
                    PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(0.0),
                        y: OrderedFloat(0.0),
                    }),
                    " px",
                    1.0,
                ),
                (
                    "anchor",
                    "Anchor",
                    PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(0.0),
                        y: OrderedFloat(0.0),
                    }),
                    " px",
                    1.0,
                ),
                (
                    "scale",
                    "Scale",
                    PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(1.0),
                        y: OrderedFloat(1.0),
                    }),
                    "×",
                    0.01,
                ),
                (
                    "rotation",
                    "Rotation",
                    PropertyValue::Number(OrderedFloat(0.0)),
                    "°",
                    0.1,
                ),
                (
                    "opacity",
                    "Opacity",
                    PropertyValue::Number(OrderedFloat(1.0)),
                    "",
                    0.01,
                ),
            ] {
                let draft_key = format!("authored:{key}");
                let local_time = item_local_time(project, state, item);
                let local_seconds = local_time
                    .as_ref()
                    .map_or(0.0, |time| time.to_seconds_f64());
                let authored = item.authored_properties.get(key);
                let mode_state =
                    PropertyModeState::from_property(authored, local_seconds, true);
                let allow_expression = default.supports_expression();
                let initial = property_value_at(item, key, default, local_seconds);
                let model_value = initial.clone();
                let (finished, mode_action, edited_value) = ui
                    .horizontal(|ui| {
                        let (finished, mode_action, edited_value, publish_default) = {
                            let value = state
                                .inspector
                                .property_values
                                .entry(draft_key)
                                .or_insert(initial);
                            let result = property_row(
                                ui,
                                value,
                                PropertyRowSpec {
                                    control_id: &format!("item:{}:{key}", item.id),
                                    label,
                                    definition: None,
                                    suffix,
                                    speed,
                                    mode_state,
                                    allow_expression,
                                },
                            );
                            (
                                result.finished,
                                result.mode_action,
                                value.clone(),
                                value.clone(),
                            )
                        };
                        composition_parameters::publication_icon(
                            ui,
                            project,
                            state,
                            service,
                            item,
                            composition_parameters::PublicationSpec {
                                target: library::model::authoring::CompositionParameterTarget::ItemProperty {
                                    item_id: item.id,
                                    property_key: key.to_string(),
                                },
                                default_value: publish_default,
                                suggested_name: format!("{} {label}", item.name),
                            },
                        );
                        (finished, mode_action, edited_value)
                    })
                    .inner;
                if finished && edited_value != model_value {
                    let result = local_time.clone().and_then(|time| {
                        property_authoring::commit_authored_value(
                            service,
                            AuthoringPropertyOwner::Item(item.id),
                            key,
                            authored,
                            edited_value.clone(),
                            time,
                        )
                    });
                    if let Err(error) = result {
                        state.error = Some(error);
                    }
                }
                if let Some(action) = mode_action {
                    let result = local_time.clone().and_then(|time| {
                        property_authoring::apply_authored_mode_action(
                            service,
                            AuthoringPropertyOwner::Item(item.id),
                            key,
                            authored,
                            edited_value,
                            time,
                            action,
                        )
                    });
                    if let Err(error) = result {
                        state.error = Some(error);
                    } else {
                        state.status = format!("{label}: {}", mode_action_label(action));
                    }
                }
                if authored.is_some_and(|property| property.evaluator == "expression") {
                    expression_source(
                        ui,
                        state,
                        service,
                        item,
                        key,
                        authored,
                        &format!("item:{}:{key}", item.id),
                    );
                }
                value_provenance(
                    ui,
                    item.authored_properties
                        .get(key)
                        .is_some_and(|property| property.evaluator == "keyframe"),
                    false,
                );
            }
        });
}

fn module_parameters(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    invocation: &library::model::authoring::ModuleInvocation,
) {
    let Some(instance) = project.module_instances.get(&invocation.instance_id) else {
        return;
    };
    let Some(definition) = project.module_definitions.get(&instance.definition_id) else {
        return;
    };
    ui.separator();
    egui::CollapsingHeader::new("Node Clip parameters")
        .default_open(true)
        .show(ui, |ui| {
            if definition.interface.parameters.is_empty() {
                ui.weak("Publish a Node input to expose a reusable control here.");
            }
            for parameter in &definition.interface.parameters {
                let key = format!("module:{}", parameter.id);
                let base_value = instance
                    .parameter_overrides
                    .get(&parameter.id)
                    .cloned()
                    .unwrap_or_else(|| parameter.default_value.clone());
                let local_time = item_local_time(project, state, item);
                let local_seconds = local_time
                    .as_ref()
                    .map_or(0.0, |time| time.to_seconds_f64());
                let automation = invocation.automation_tracks.get(&parameter.id);
                let initial = automation
                    .and_then(|track| {
                        local_time
                            .as_ref()
                            .ok()
                            .and_then(|time| track.evaluate_at(*time).ok())
                    })
                    .unwrap_or(base_value);
                let model_value = initial.clone();
                let mode_state = automation.map_or_else(
                    || PropertyModeState::constant(local_seconds),
                    |track| {
                        PropertyModeState::from_keyframe_times(
                            local_seconds,
                            track
                                .keyframes
                                .iter()
                                .map(|keyframe| keyframe.time.to_seconds_f64()),
                        )
                    },
                );
                let (finished, mode_action, edited_value) = {
                    let value = state
                        .inspector
                        .property_values
                        .entry(key)
                        .or_insert(initial);
                    let result = property_row(
                        ui,
                        value,
                        PropertyRowSpec {
                            control_id: &format!(
                                "module_instance:{}:{}",
                                instance.id, parameter.id
                            ),
                            label: &parameter.name,
                            definition: None,
                            suffix: "",
                            speed: 0.1,
                            mode_state,
                            allow_expression: false,
                        },
                    );
                    (result.finished, result.mode_action, value.clone())
                };
                if finished && edited_value != model_value {
                    let result = local_time.clone().and_then(|time| {
                        property_authoring::commit_module_parameter_value(
                            service,
                            item.id,
                            instance.id,
                            parameter.id,
                            automation,
                            edited_value.clone(),
                            time,
                        )
                    });
                    if let Err(error) = result {
                        state.error = Some(error);
                    }
                }
                if let Some(action) = mode_action {
                    let result = local_time.clone().and_then(|time| {
                        property_authoring::apply_module_parameter_mode_action(
                            service,
                            item.id,
                            parameter.id,
                            automation,
                            edited_value,
                            time,
                            action,
                        )
                    });
                    if let Err(error) = result {
                        state.error = Some(error);
                    } else {
                        state.status = format!("{}: {}", parameter.name, mode_action_label(action));
                    }
                }
                value_provenance(
                    ui,
                    automation.is_some(),
                    instance.parameter_overrides.contains_key(&parameter.id),
                );
            }
        });
    module_media_inputs(ui, project, state, service, item, invocation, definition);
}

fn module_media_inputs(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    invocation: &library::model::authoring::ModuleInvocation,
    definition: &library::model::authoring::ModuleDefinition,
) {
    if definition.interface.media_inputs.is_empty() {
        return;
    }
    let Some(host_track) = project.tracks.get(&item.track_id) else {
        return;
    };
    let mut candidates = project
        .items
        .values()
        .filter(|candidate| candidate.id != item.id)
        .filter(|candidate| {
            project
                .tracks
                .get(&candidate.track_id)
                .is_some_and(|track| track.timeline_id == host_track.timeline_id)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| (candidate.layer, candidate.interval.start, candidate.id));

    egui::CollapsingHeader::new("Node Clip inputs")
        .default_open(true)
        .show(ui, |ui| {
            for input in &definition.interface.media_inputs {
                ui.horizontal(|ui| {
                    ui.label(&input.name);
                    if input.data_type != library::model::project::PortDataType::Image {
                        ui.weak("Audio input runtime is not available yet");
                        return;
                    }
                    let current = invocation.input_bindings.get(&input.id).map(|binding| {
                        let MediaInputBinding::TimelineItemOutput { item_id, .. } = binding;
                        *item_id
                    });
                    let current_label = current
                        .and_then(|item_id| project.items.get(&item_id))
                        .map_or("Unbound", |source| source.name.as_str());
                    egui::ComboBox::from_id_salt(("module-media-input", item.id, input.id))
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(current.is_none(), "Unbound").clicked()
                                && current.is_some()
                            {
                                if let Err(error) = service.unbind_module_input(item.id, input.id) {
                                    state.error = Some(error.to_string());
                                }
                            }
                            for source in &candidates {
                                if !item_has_image_output(project, source) {
                                    continue;
                                }
                                if ui
                                    .selectable_label(current == Some(source.id), &source.name)
                                    .clicked()
                                {
                                    let binding = MediaInputBinding::TimelineItemOutput {
                                        locator: InstanceLocator::SameTimeline,
                                        item_id: source.id,
                                        output: MediaOutputKind::Image,
                                        stage: ItemOutputStage::PostTransform,
                                    };
                                    match service.bind_module_input(item.id, input.id, binding) {
                                        Ok(_) => {
                                            state.status =
                                                format!("Bound {} to {}", source.name, input.name);
                                        }
                                        Err(error) => state.error = Some(error.to_string()),
                                    }
                                }
                            }
                        });
                });
            }
            ui.weak("Inputs reference clip outputs, not internal Node UUIDs.");
        });
}

fn item_has_image_output(project: &AuthoringProject, item: &TimelineItem) -> bool {
    match &item.source {
        SourceRef::Asset { asset_id } => project
            .assets
            .iter()
            .find(|asset| asset.id == *asset_id)
            .is_some_and(|asset| {
                matches!(
                    asset.kind,
                    library::model::asset::AssetKind::Image
                        | library::model::asset::AssetKind::Video
                )
            }),
        SourceRef::Text { .. }
        | SourceRef::Shape { .. }
        | SourceRef::Solid { .. }
        | SourceRef::Composition(_) => true,
        SourceRef::Module(invocation) => project
            .module_instances
            .get(&invocation.instance_id)
            .and_then(|instance| project.module_definitions.get(&instance.definition_id))
            .and_then(|definition| definition.output(invocation.output_id))
            .is_some_and(|output| output.supports(library::model::project::PortDataType::Image)),
    }
}

fn sync_draft(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    selection: Option<AuthoringSelection>,
    revision: library::model::authoring::ProjectRevision,
) {
    let current_frame = state.timeline.current_frame;
    if state.inspector.target == selection && state.inspector.synced_revision == Some(revision) {
        if state.inspector.synced_frame != Some(current_frame) {
            state.inspector.synced_frame = Some(current_frame);
            state.inspector.property_values.clear();
            state.inspector.effect_values.clear();
            state.inspector.transient_property_edit = None;
        }
        return;
    }
    state.inspector.target = selection;
    state.inspector.synced_revision = Some(revision);
    state.inspector.synced_frame = Some(current_frame);
    state.inspector.property_values.clear();
    state.inspector.expression_sources.clear();
    state.inspector.effect_values.clear();
    state.inspector.transient_property_edit = None;
    state.inspector.name.clear();
    state.inspector.text.clear();
    let Some(selection) = selection else {
        return;
    };
    match selection {
        AuthoringSelection::Timeline(id) => {
            if let Some(timeline) = project.timelines.get(&id) {
                state.inspector.name.clone_from(&timeline.name);
            }
        }
        AuthoringSelection::Track(id) => {
            if let Some(track) = project.tracks.get(&id) {
                state.inspector.name.clone_from(&track.name);
            }
        }
        AuthoringSelection::Item(id) => {
            if let Some(item) = project.items.get(&id) {
                state.inspector.name.clone_from(&item.name);
                state.inspector.start_seconds = item.interval.start.to_seconds_f64();
                state.inspector.duration_seconds = item.interval.duration.to_seconds_f64();
                if let SourceRef::Text { text, .. } = &item.source {
                    state.inspector.text.clone_from(text);
                }
                for (key, property) in item.authored_properties.iter() {
                    if let Some(value) = property.value() {
                        state
                            .inspector
                            .property_values
                            .insert(format!("authored:{key}"), value.clone());
                    }
                    if let Some(source) = property.expression_text() {
                        state
                            .inspector
                            .expression_sources
                            .insert(format!("item:{}:{key}", item.id), source.to_string());
                    }
                }
            }
        }
        AuthoringSelection::Transition(_)
        | AuthoringSelection::Asset(_)
        | AuthoringSelection::ModuleDefinition(_) => {}
    }
}

fn expression_source(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    key: &str,
    property: Option<&library::model::property::Property>,
    control_id: &str,
) {
    let model_source = property
        .and_then(library::model::property::Property::expression_text)
        .unwrap_or_default();
    let committed_source = {
        let source = state
            .inspector
            .expression_sources
            .entry(control_id.to_string())
            .or_insert_with(|| model_source.to_string());
        property_authoring::expression_source_editor(ui, control_id, source, model_source)
            .then(|| source.clone())
    };
    if let Some(source) = committed_source {
        if let Err(error) = property_authoring::commit_expression_source(
            service,
            AuthoringPropertyOwner::Item(item.id),
            key,
            property,
            source,
        ) {
            state.error = Some(error);
        }
    }
}

const fn mode_action_label(action: PropertyModeAction) -> &'static str {
    match action {
        PropertyModeAction::SetMode(PropertyAuthoringMode::Constant) => "Constant",
        PropertyModeAction::SetMode(PropertyAuthoringMode::Keyframe) => "Keyframe",
        PropertyModeAction::SetMode(PropertyAuthoringMode::Expression) => "Expression",
        PropertyModeAction::ToggleKeyframe => "Keyframe updated",
    }
}

fn property_value_at(
    item: &TimelineItem,
    key: &str,
    default: PropertyValue,
    local_seconds: f64,
) -> PropertyValue {
    item.authored_properties
        .get(key)
        .and_then(|property| property.evaluate_at(local_seconds).ok())
        .unwrap_or(default)
}

pub(super) fn item_local_time(
    project: &AuthoringProject,
    state: &AuthoringUiState,
    item: &TimelineItem,
) -> Result<MediaTime, String> {
    let timeline = project
        .timelines
        .get(&state.active_timeline_id)
        .ok_or_else(|| "Active Timeline is missing".to_string())?;
    let timeline_time = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)?;
    item.time_map.local_time(item.interval, timeline_time)
}

fn item_icon(item: &TimelineItem) -> &'static str {
    match &item.source {
        SourceRef::Asset { .. } => icons::FILE_VIDEO,
        SourceRef::Text { .. } => icons::TEXT_T,
        SourceRef::Shape { .. } | SourceRef::Solid { .. } => icons::SQUARE,
        SourceRef::Composition(_) => icons::FILM_STRIP,
        SourceRef::Module(_) => icons::SHARE_NETWORK,
    }
}

fn item_kind(item: &TimelineItem) -> &'static str {
    match &item.source {
        SourceRef::Asset { .. } => "Media clip",
        SourceRef::Text { .. } => "Text clip",
        SourceRef::Shape { .. } => "Shape clip",
        SourceRef::Solid { .. } => "Solid clip",
        SourceRef::Composition(_) => "Nested composition",
        SourceRef::Module(_) => "Node Clip",
    }
}

fn track_kind_name(kind: library::model::authoring::TimelineTrackKind) -> &'static str {
    match kind {
        library::model::authoring::TimelineTrackKind::Visual => "Visual",
        library::model::authoring::TimelineTrackKind::Audio => "Audio",
        library::model::authoring::TimelineTrackKind::AudioVisual => "Audio + Visual",
    }
}

fn duration_policy_name(policy: &library::model::authoring::DurationPolicy) -> &'static str {
    match policy {
        library::model::authoring::DurationPolicy::Fixed => "Fixed",
        library::model::authoring::DurationPolicy::Scale => "Scale",
        library::model::authoring::DurationPolicy::Loop => "Loop",
        library::model::authoring::DurationPolicy::Responsive { .. } => "Responsive",
    }
}

pub(super) fn value_provenance(ui: &mut egui::Ui, keyframed: bool, overridden: bool) {
    if !keyframed && !overridden {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.add_space(126.0);
        let mut sources = vec!["Base"];
        if keyframed {
            sources.push("Keyframe");
        }
        if overridden {
            sources.push("Override");
        }
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("{}  →  Effective", sources.join(" + ")))
                    .small()
                    .weak(),
            )
            .wrap(),
        )
        .on_hover_text(if overridden {
            "The override belongs only to this instance placement."
        } else {
            "The effective value is evaluated from its Timeline keyframes."
        });
    });
}
