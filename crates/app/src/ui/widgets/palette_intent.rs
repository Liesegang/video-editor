//! Frame-local handoff from shared Paint controls to the Project command layer.
//!
//! Widgets only emit typed intents. The application drains them after all
//! docked panels finish drawing, so a Palette command never mutates the
//! authoritative Project while a panel is borrowing its immutable snapshot.

use egui::{Context, Id};

use super::color_value_picker::PaletteUiIntent;

fn queue_id() -> Id {
    Id::new("project_palette.intent_queue")
}

pub(crate) fn queue(context: &Context, intent: PaletteUiIntent) {
    context.data_mut(|data| {
        data.get_temp_mut_or_default::<Vec<PaletteUiIntent>>(queue_id())
            .push(intent);
    });
}

pub(crate) fn drain(context: &Context) -> Vec<PaletteUiIntent> {
    context.data_mut(|data| {
        std::mem::take(data.get_temp_mut_or_default::<Vec<PaletteUiIntent>>(queue_id()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::property::{ColorSpaceRef, ColorValue};

    #[test]
    fn queue_is_fifo_and_drained_once() {
        let context = Context::default();
        queue(
            &context,
            PaletteUiIntent::AddSolid {
                suggested_name: "First".to_string(),
                color: ColorValue::new(ColorSpaceRef::srgb(), [0.1, 0.2, 0.3, 1.0]).unwrap(),
            },
        );
        let id = library::model::authoring::PaintDefinitionId::new();
        queue(&context, PaletteUiIntent::Delete { id });

        let intents = drain(&context);
        assert_eq!(intents.len(), 2);
        assert!(matches!(
            intents[0],
            PaletteUiIntent::AddSolid { ref suggested_name, .. } if suggested_name == "First"
        ));
        assert_eq!(intents[1], PaletteUiIntent::Delete { id });
        assert!(drain(&context).is_empty());
    }
}
