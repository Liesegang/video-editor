use egui::Ui;

use crate::{
    action::HistoryManager,
    state::context::EditorContext,
    model::assets::{AssetKind},
};


pub fn assets_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _history_manager: &mut HistoryManager, // HistoryManager not used directly here
) {
    ui.heading("Assets");
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (index, asset) in editor_context.assets.iter().enumerate() {
            ui.push_id(asset.id(), |ui_in_scope| {
                let label_text = format!("{} ({:.1}s)", asset.name, asset.duration);
                let icon = match asset.kind {
                    AssetKind::Video => "🎥",
                    AssetKind::Audio => "🎵",
                    AssetKind::Image => "🖼️",
                    AssetKind::Composition(_) => "🗂️",
                };

                let item_response = ui_in_scope
                    .add(
                        egui::Label::new(
                            egui::RichText::new(format!("{} {}", icon, label_text))
                                .background_color(asset.color)
                                .color(egui::Color32::BLACK),
                        )
                        .sense(egui::Sense::drag()),
                    )
                    .on_hover_text(format!("Asset ID: {:?}", asset.id()));

                if item_response.drag_started() {
                    editor_context.dragged_asset = Some(index);
                }
                ui_in_scope.add_space(5.0);
            });
        }
    });
}
