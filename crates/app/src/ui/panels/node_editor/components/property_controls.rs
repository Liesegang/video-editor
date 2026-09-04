use eframe::egui;

use crate::ui::panels::node_editor::{PORT_ROW_HEIGHT, PROPERTY_LABEL_WIDTH};

/// The bounded, non-selectable label used by the production Node input rows.
pub(in crate::ui::panels::node_editor) fn property_label(
    ui: &mut egui::Ui,
    text: impl Into<String>,
) -> egui::Response {
    let text = text.into();
    let width = measured_label_width(ui, &text, PROPERTY_LABEL_WIDTH);
    ui.add_sized(
        [width, PORT_ROW_HEIGHT],
        egui::Label::new(text)
            .selectable(false)
            .halign(egui::Align::LEFT),
    )
}

/// Width required to show a production Node label without truncation.
/// Header, port and property rows share the same font measurement and padding.
pub(in crate::ui::panels::node_editor) fn measured_label_width(
    ui: &egui::Ui,
    text: &str,
    minimum: f32,
) -> f32 {
    let text_width = ui
        .painter()
        .layout_no_wrap(
            text.to_string(),
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().text_color(),
        )
        .size()
        .x;
    minimum.max(text_width + 8.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_node_labels_expand_beyond_the_old_fixed_width() {
        let context = egui::Context::default();
        let mut measured = 0.0;
        drop(context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                measured = measured_label_width(
                    ui,
                    "A very long production property label that must remain readable",
                    PROPERTY_LABEL_WIDTH,
                );
            });
        }));
        assert!(measured > PROPERTY_LABEL_WIDTH * 2.0);
    }
}
