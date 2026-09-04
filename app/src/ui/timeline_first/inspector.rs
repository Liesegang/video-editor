mod audio;
mod composition_parameters;
mod effect_stack;

use egui_phosphor::regular as icons;
use library::editor::{AuthoringPropertyOwner, TimelineEditorService};
use library::model::asset::AssetKind;
use library::model::authoring::{
    AttachmentOwner, AttachmentStage, AuthoringProject, InstanceLocator, ItemOutputStage,
    MediaInputBinding, MediaOutputKind, MediaTime, SourceRef, TimelineItem,
};
use library::model::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec2};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;

use crate::state::authoring::{AuthoringSelection, AuthoringUiState};
use crate::ui::widgets::property_drag_value::{FloatDragValueConfig, IntegerDragValueConfig};

pub fn inspector_panel(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
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

    egui::ScrollArea::vertical()
        .id_salt("authoring_inspector_scroll")
        .show(ui, |ui| match selection {
            AuthoringSelection::Timeline(id) => {
                if let Some(timeline) = project.timelines.get(&id) {
                    section_title(ui, icons::FILM_STRIP, "Timeline", &timeline.name);
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
                    section_title(ui, icons::STACK, "Track", &track.name);
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
            AuthoringSelection::Asset(id) => {
                if let Some(asset) = project.assets.iter().find(|asset| asset.id == id) {
                    let icon = match asset.kind {
                        AssetKind::Video => icons::FILE_VIDEO,
                        AssetKind::Audio => icons::FILE_AUDIO,
                        AssetKind::Image => icons::FILE_IMAGE,
                        AssetKind::Model3D => icons::CUBE,
                        AssetKind::Other => icons::FILE,
                    };
                    section_title(ui, icon, "Media", &asset.name);
                    ui.separator();
                    ui.monospace(&asset.path);
                    if let Some(duration) = asset.duration {
                        ui.label(format!("Duration: {duration:.3} s"));
                    }
                    if let (Some(width), Some(height)) = (asset.width, asset.height) {
                        ui.label(format!("Frame: {width} × {height}"));
                    }
                }
            }
            AuthoringSelection::ModuleDefinition(id) => {
                if let Some(definition) = project.module_definitions.get(&id) {
                    section_title(
                        ui,
                        icons::SHARE_NETWORK,
                        "Node Clip template",
                        &definition.name,
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

fn section_title(ui: &mut egui::Ui, icon: &str, kind: &str, name: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icon).size(20.0));
        ui.vertical(|ui| {
            ui.weak(kind);
            ui.label(egui::RichText::new(name).strong());
        });
    });
    ui.add_space(6.0);
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
    section_title(ui, item_icon(item), item_kind(item), &item.name);
    editable_name(ui, state, &item.name, |name| {
        service.rename_item(item.id, name)
    });
    ui.separator();

    timing_section(ui, state, service, item);
    if audio::item_is_audio_asset(project, item) {
        audio::audio_section(ui, project, state, service, item);
    } else {
        transform_section(ui, project, state, service, item);
    }

    if let SourceRef::Text { text } = &item.source {
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
                        library::model::authoring::CompositionParameterTarget::TextContent {
                            item_id: item.id,
                        },
                        PropertyValue::String(text.clone()),
                        format!("{} Text", item.name),
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

fn timing_section(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
) {
    egui::CollapsingHeader::new("Timing")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new(("item_timing", item.id))
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Start");
                    let start = ui.add(
                        egui::DragValue::new(&mut state.inspector.start_seconds)
                            .speed(0.01)
                            .range(0.0..=f64::INFINITY)
                            .suffix(" s")
                            .clamp_existing_to_range(false),
                    );
                    ui.end_row();
                    ui.label("Duration");
                    let duration = ui.add(
                        egui::DragValue::new(&mut state.inspector.duration_seconds)
                            .speed(0.01)
                            .range(1.0 / 1_000.0..=f64::INFINITY)
                            .suffix(" s")
                            .clamp_existing_to_range(false),
                    );
                    ui.end_row();

                    if numeric_finished(&start)
                        && state.inspector.start_seconds != item.interval.start.to_seconds_f64()
                    {
                        if let Ok(new_start) = MediaTime::from_seconds_f64(
                            state.inspector.start_seconds.max(0.0),
                            1_000_000,
                        ) {
                            if let Err(error) =
                                service.move_item(item.id, item.track_id, new_start, item.layer)
                            {
                                state.error = Some(error.to_string());
                            }
                        }
                    }
                    if numeric_finished(&duration)
                        && state.inspector.duration_seconds
                            != item.interval.duration.to_seconds_f64()
                    {
                        if let (Ok(start), Ok(duration)) = (
                            MediaTime::from_seconds_f64(
                                state.inspector.start_seconds.max(0.0),
                                1_000_000,
                            ),
                            MediaTime::from_seconds_f64(
                                state.inspector.duration_seconds.max(1.0 / 1_000.0),
                                1_000_000,
                            ),
                        ) {
                            if let Ok(interval) =
                                library::model::authoring::TimelineInterval::new(start, duration)
                            {
                                if let Err(error) = service.trim_item(item.id, interval) {
                                    state.error = Some(error.to_string());
                                }
                            }
                        }
                    }
                });
        });
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
                let initial =
                    property_value_at(item, key, default, item_local_seconds(project, state, item));
                let model_value = initial.clone();
                let (finished, keyframe_clicked, edited_value) = ui
                    .horizontal(|ui| {
                        let (finished, keyframe_clicked, edited_value, publish_default) = {
                            let value = state
                                .inspector
                                .property_values
                                .entry(draft_key)
                                .or_insert(initial);
                            let result = property_row(ui, label, value, None, suffix, speed, true);
                            (result.0, result.1, value.clone(), value.clone())
                        };
                        composition_parameters::publication_icon(
                            ui,
                            project,
                            state,
                            service,
                            item,
                            library::model::authoring::CompositionParameterTarget::ItemProperty {
                                item_id: item.id,
                                property_key: key.to_string(),
                            },
                            publish_default,
                            format!("{} {label}", item.name),
                        );
                        (finished, keyframe_clicked, edited_value)
                    })
                    .inner;
                if finished && edited_value != model_value {
                    commit_authored_value(state, service, item, key, edited_value.clone(), project);
                }
                if keyframe_clicked {
                    upsert_authored_key(state, service, item, key, edited_value, project);
                }
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
                let initial = instance
                    .parameter_overrides
                    .get(&parameter.id)
                    .cloned()
                    .unwrap_or_else(|| parameter.default_value.clone());
                let model_value = initial.clone();
                let automated = invocation.automation_tracks.contains_key(&parameter.id);
                let (finished, keyframe_clicked, edited_value) = {
                    let value = state
                        .inspector
                        .property_values
                        .entry(key)
                        .or_insert(initial);
                    let (finished, keyframe_clicked) =
                        property_row(ui, &parameter.name, value, None, "", 0.1, true);
                    (finished, keyframe_clicked, value.clone())
                };
                if finished && edited_value != model_value {
                    if let Err(error) = service.set_module_parameter(
                        instance.id,
                        parameter.id,
                        edited_value.clone(),
                    ) {
                        state.error = Some(error.to_string());
                    }
                }
                if keyframe_clicked {
                    let local_time = item_local_time(project, state, item);
                    match local_time.and_then(|time| {
                        service
                            .upsert_module_parameter_keyframe(
                                item.id,
                                parameter.id,
                                time,
                                edited_value,
                                None,
                            )
                            .map(|_| ())
                            .map_err(|error| error.to_string())
                    }) {
                        Ok(()) => {}
                        Err(error) => state.error = Some(error),
                    }
                }
                ui.horizontal(|ui| {
                    ui.add_space(126.0);
                    ui.weak(format!(
                        "Base{}  →  Effective",
                        if automated { " + Keyframe" } else { "" }
                    ));
                });
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
            .is_some_and(|asset| matches!(asset.kind, AssetKind::Image | AssetKind::Video)),
        SourceRef::Text { .. }
        | SourceRef::Shape { .. }
        | SourceRef::Solid { .. }
        | SourceRef::Composition(_) => true,
        SourceRef::Module(invocation) => project
            .module_instances
            .get(&invocation.instance_id)
            .and_then(|instance| project.module_definitions.get(&instance.definition_id))
            .and_then(|definition| {
                definition
                    .interface
                    .media_outputs
                    .iter()
                    .find(|output| output.id == invocation.output_id)
            })
            .is_some_and(|output| output.data_type == library::model::project::PortDataType::Image),
    }
}

fn property_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut PropertyValue,
    definition: Option<&PropertyDefinition>,
    suffix: &str,
    speed: f64,
    keyframe: bool,
) -> (bool, bool) {
    let mut finished = false;
    let mut keyframe_clicked = false;
    ui.horizontal(|ui| {
        ui.add_sized([112.0, 20.0], egui::Label::new(label));
        finished = property_control(ui, value, definition, suffix, speed);
        if keyframe {
            keyframe_clicked = ui
                .small_button("◇")
                .on_hover_text("Set or update keyframe at the playhead")
                .clicked();
        }
    });
    (finished, keyframe_clicked)
}

pub(super) fn property_control(
    ui: &mut egui::Ui,
    value: &mut PropertyValue,
    definition: Option<&PropertyDefinition>,
    suffix: &str,
    speed: f64,
) -> bool {
    match value {
        PropertyValue::Number(number) => {
            let mut raw = number.into_inner();
            let response =
                if let Some(config) = definition.and_then(FloatDragValueConfig::from_definition) {
                    ui.add(config.widget(&mut raw))
                } else {
                    ui.add(egui::DragValue::new(&mut raw).speed(speed).suffix(suffix))
                };
            if response.changed() {
                *number = OrderedFloat(raw);
            }
            numeric_finished(&response)
        }
        PropertyValue::Integer(integer) => {
            let response = if let Some(config) = definition
                .and_then(|definition| IntegerDragValueConfig::from_ui_type(definition.ui_type()))
            {
                ui.add(config.widget(integer))
            } else {
                ui.add(egui::DragValue::new(integer).speed(1.0).suffix(suffix))
            };
            numeric_finished(&response)
        }
        PropertyValue::Boolean(boolean) => ui.checkbox(boolean, "").changed(),
        PropertyValue::String(text) => {
            if let Some(PropertyUiType::Dropdown { options }) =
                definition.map(PropertyDefinition::ui_type)
            {
                let previous = text.clone();
                egui::ComboBox::from_id_salt((label_id(text), options.len()))
                    .selected_text(text.as_str())
                    .show_ui(ui, |ui| {
                        for option in options {
                            ui.selectable_value(text, option.clone(), option);
                        }
                    });
                *text != previous
            } else {
                let response = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
                text_edit_finished(
                    response.lost_focus(),
                    response.has_focus(),
                    ui.input(|input| input.key_pressed(egui::Key::Enter)),
                )
            }
        }
        PropertyValue::Vec2(vector) => {
            let config = definition
                .and_then(FloatDragValueConfig::from_definition)
                .unwrap_or(FloatDragValueConfig {
                    speed,
                    suffix: suffix.to_string(),
                    hard_min: None,
                    hard_max: None,
                });
            let mut x = vector.x.into_inner();
            let mut y = vector.y.into_inner();
            let x_response = ui.add(config.widget(&mut x).prefix("X "));
            let y_response = ui.add(config.widget(&mut y).prefix("Y "));
            if x_response.changed() || y_response.changed() {
                vector.x = OrderedFloat(x);
                vector.y = OrderedFloat(y);
            }
            numeric_finished(&x_response) || numeric_finished(&y_response)
        }
        PropertyValue::Color(color) => {
            let mut rgba =
                egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
            let response = ui.color_edit_button_srgba(&mut rgba);
            if response.changed() {
                [color.r, color.g, color.b, color.a] = rgba.to_array();
            }
            response.drag_stopped() || response.lost_focus()
        }
        _ => {
            ui.weak("Edit in Node Editor");
            false
        }
    }
}

fn text_edit_finished(lost_focus: bool, has_focus: bool, enter_pressed: bool) -> bool {
    lost_focus || (has_focus && enter_pressed)
}

fn sync_draft(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    selection: Option<AuthoringSelection>,
    revision: library::model::authoring::ProjectRevision,
) {
    if state.inspector.target == selection && state.inspector.synced_revision == Some(revision) {
        return;
    }
    state.inspector.target = selection;
    state.inspector.synced_revision = Some(revision);
    state.inspector.property_values.clear();
    state.inspector.effect_values.clear();
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
                if let SourceRef::Text { text } = &item.source {
                    state.inspector.text.clone_from(text);
                }
                for (key, property) in item.authored_properties.iter() {
                    if let Some(value) = property.value() {
                        state
                            .inspector
                            .property_values
                            .insert(format!("authored:{key}"), value.clone());
                    }
                }
            }
        }
        AuthoringSelection::Asset(_) | AuthoringSelection::ModuleDefinition(_) => {}
    }
}

fn commit_authored_value(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    key: &str,
    value: PropertyValue,
    project: &AuthoringProject,
) {
    let result = if item
        .authored_properties
        .get(key)
        .is_some_and(|property| property.evaluator == "keyframe")
    {
        item_local_time(project, state, item).and_then(|time| {
            service
                .upsert_authored_property_keyframe(
                    AuthoringPropertyOwner::Item(item.id),
                    key.to_string(),
                    time,
                    value,
                    None,
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    } else {
        service
            .set_authored_property_constant(
                AuthoringPropertyOwner::Item(item.id),
                key.to_string(),
                value,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    };
    if let Err(error) = result {
        state.error = Some(error);
    }
}

fn upsert_authored_key(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    key: &str,
    value: PropertyValue,
    project: &AuthoringProject,
) {
    let result = item_local_time(project, state, item).and_then(|time| {
        service
            .upsert_authored_property_keyframe(
                AuthoringPropertyOwner::Item(item.id),
                key.to_string(),
                time,
                value,
                None,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    if let Err(error) = result {
        state.error = Some(error);
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

fn item_local_time(
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

fn item_local_seconds(
    project: &AuthoringProject,
    state: &AuthoringUiState,
    item: &TimelineItem,
) -> f64 {
    item_local_time(project, state, item)
        .map(MediaTime::to_seconds_f64)
        .unwrap_or(0.0)
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

fn numeric_finished(response: &egui::Response) -> bool {
    response.drag_stopped()
        || response.lost_focus()
        || (response.has_focus()
            && response
                .ctx
                .input(|input| input.key_pressed(egui::Key::Enter)))
}

fn label_id(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::text_edit_finished;

    #[test]
    fn text_draft_commits_when_focus_leaves_after_an_earlier_edit_frame() {
        assert!(!text_edit_finished(false, true, false));
        assert!(text_edit_finished(true, false, false));
    }

    #[test]
    fn enter_commits_the_focused_single_line_editor() {
        assert!(text_edit_finished(false, true, true));
        assert!(!text_edit_finished(false, false, true));
    }
}
