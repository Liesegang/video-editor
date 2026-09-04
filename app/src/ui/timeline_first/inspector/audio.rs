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
            let initial = super::property_value_at(
                item,
                key,
                PropertyValue::Number(OrderedFloat(1.0)),
                super::item_local_seconds(project, state, item),
            );
            let model_value = initial.clone();
            let (finished, keyframe_clicked, edited_value) = {
                let value = state
                    .inspector
                    .property_values
                    .entry(draft_key)
                    .or_insert(initial);
                let result = super::property_row(ui, "Gain", value, None, " ×", 0.01, true);
                (result.0, result.1, value.clone())
            };
            if finished && edited_value != model_value {
                super::commit_authored_value(
                    state,
                    service,
                    item,
                    key,
                    edited_value.clone(),
                    project,
                );
            }
            if keyframe_clicked {
                super::upsert_authored_key(state, service, item, key, edited_value, project);
            }
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
