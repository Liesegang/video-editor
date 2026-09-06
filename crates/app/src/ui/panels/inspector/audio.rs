use egui_phosphor::regular as icons;
use library::editor::TimelineEditorService;
use library::model::asset::AssetKind;
use library::model::authoring::{AuthoringProject, SourceRef, TimelineItem};
use library::model::property::PropertyValue;
use ordered_float::OrderedFloat;

use crate::state::authoring::AuthoringUiState;

pub(super) fn audio_section(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
) {
    egui::CollapsingHeader::new(format!("{} Audio", icons::SPEAKER_HIGH))
        .default_open(true)
        .show(ui, |ui| {
            let key = "gain";
            let draft_key = "authored:gain".to_string();
            let local_time = super::item_local_time(project, state, item);
            let local_seconds = local_time
                .as_ref()
                .map_or(0.0, |time| time.to_seconds_f64());
            let authored = item.authored_properties.get(key);
            let initial = super::property_value_at(
                item,
                key,
                PropertyValue::Number(OrderedFloat(1.0)),
                local_seconds,
            );
            let model_value = initial.clone();
            let owner = library::editor::AuthoringPropertyOwner::Item(item.id);
            let pending_keyframe = super::property_authoring::pending_authored_keyframe(
                state.inspector.transient_property_edit.as_ref(),
                owner,
                key,
            );
            let (changed, finished, mode_action, edited_value) = {
                let value = state
                    .inspector
                    .property_values
                    .entry(draft_key)
                    .or_insert(initial);
                let result = super::property_row(
                    ui,
                    value,
                    &project.palette,
                    super::PropertyRowSpec {
                        control_id: &format!("item:{}:gain", item.id),
                        label: "Gain",
                        definition: None,
                        suffix: " ×",
                        speed: 0.01,
                        mode_state: super::PropertyModeState::from_property(
                            authored,
                            local_seconds,
                            true,
                        ),
                        allow_keyframe: true,
                        keyframe_disabled_reason: None,
                        allow_expression: true,
                        pending_keyframe,
                    },
                );
                (
                    result.changed,
                    result.finished,
                    result.mode_action,
                    value.clone(),
                )
            };
            if changed {
                if let Ok(local_time) = &local_time {
                    if let Some(edit) = super::property_authoring::authored_transient_edit(
                        state.inspector.synced_revision,
                        owner,
                        key,
                        authored,
                        *local_time,
                        edited_value.clone(),
                    ) {
                        super::property_authoring::update_transient_edit(
                            &mut state.inspector.transient_property_edit,
                            edit,
                        );
                    }
                }
            }
            if finished && edited_value != model_value {
                let active_edit = super::property_authoring::take_matching_authored_edit(
                    &mut state.inspector.transient_property_edit,
                    owner,
                    key,
                );
                let result = local_time.clone().and_then(|time| {
                    if authored.is_some_and(|property| property.evaluator == "expression") {
                        super::property_authoring::commit_authored_value(
                            service,
                            owner,
                            key,
                            authored,
                            edited_value.clone(),
                            time,
                        )
                    } else {
                        active_edit
                            .or_else(|| {
                                super::property_authoring::authored_transient_edit(
                                    state.inspector.synced_revision,
                                    owner,
                                    key,
                                    authored,
                                    time,
                                    edited_value.clone(),
                                )
                            })
                            .ok_or_else(|| {
                                "Inspector has no synchronized Project revision".to_string()
                            })?
                            .commit(service)
                            .map_err(|error| error.to_string())
                    }
                });
                if let Err(error) = result {
                    state.error = Some(error);
                }
            } else if finished {
                let _ = super::property_authoring::take_matching_authored_edit(
                    &mut state.inspector.transient_property_edit,
                    owner,
                    key,
                );
            }
            if let Some(action) = mode_action {
                state.inspector.transient_property_edit = None;
                let result = local_time.and_then(|time| {
                    super::property_authoring::apply_authored_mode_action(
                        service,
                        owner,
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
                    state.status = format!("Gain: {}", super::mode_action_label(action));
                }
            }
            if authored.is_some_and(|property| property.evaluator == "expression") {
                super::expression_source(
                    ui,
                    state,
                    service,
                    item,
                    key,
                    authored,
                    &format!("item:{}:{key}", item.id),
                );
            }
            super::value_provenance(
                ui,
                item.authored_properties
                    .get(key)
                    .is_some_and(|property| property.evaluator == "keyframe"),
                false,
            );
            ui.weak("1.0 × is the imported level; mix changes remain on this clip.");
        });
}

pub(super) fn item_is_audio_asset(project: &AuthoringProject, item: &TimelineItem) -> bool {
    let SourceRef::Asset { asset_id } = &item.source else {
        return false;
    };
    project
        .assets
        .iter()
        .find(|asset| asset.id == *asset_id)
        .is_some_and(|asset| asset.kind == AssetKind::Audio)
}
