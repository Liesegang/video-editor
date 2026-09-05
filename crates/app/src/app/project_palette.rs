//! Application boundary for Project Palette UI intents.

use library::editor::TimelineEditorService;

use crate::state::authoring::AuthoringUiState;
use crate::ui::widgets::color_value_picker::PaletteUiIntent;

pub(super) fn apply_pending(
    context: &egui::Context,
    service: &TimelineEditorService,
    state: &mut AuthoringUiState,
) {
    for intent in crate::ui::widgets::palette_intent::drain(context) {
        let (result, status) = match intent {
            PaletteUiIntent::AddSolid {
                suggested_name,
                color,
            } => (
                service
                    .add_solid_paint_definition(suggested_name.clone(), color)
                    .map(|_| ()),
                format!("Added {suggested_name} to Project Palette"),
            ),
            PaletteUiIntent::Rename { id, name } => (
                service
                    .rename_paint_definition(id, name.clone())
                    .map(|_| ()),
                format!("Renamed Palette color to {name}"),
            ),
            PaletteUiIntent::Reorder { id, new_index } => (
                service.reorder_paint_definition(id, new_index).map(|_| ()),
                "Reordered Project Palette".to_string(),
            ),
            PaletteUiIntent::Delete { id } => (
                service.delete_paint_definition(id).map(|_| ()),
                "Deleted Project Palette color".to_string(),
            ),
        };
        match result {
            Ok(()) => state.status = status,
            Err(error) => state.error = Some(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::property::{ColorSpaceRef, ColorValue};

    #[test]
    fn queued_intent_crosses_the_ui_boundary_once() {
        let context = egui::Context::default();
        let service = TimelineEditorService::create_default("Palette UI").unwrap();
        let root = service.snapshot().unwrap().root_timeline_id;
        let mut state = AuthoringUiState::new(root);
        crate::ui::widgets::palette_intent::queue(
            &context,
            PaletteUiIntent::AddSolid {
                suggested_name: "Accent".to_string(),
                color: ColorValue::new(ColorSpaceRef::srgb(), [0.25, 0.5, 1.0, 1.0]).unwrap(),
            },
        );

        apply_pending(&context, &service, &mut state);
        assert_eq!(service.revision().unwrap().get(), 1);
        assert_eq!(service.snapshot().unwrap().palette.definitions.len(), 1);
        apply_pending(&context, &service, &mut state);
        assert_eq!(service.revision().unwrap().get(), 1);
    }
}
