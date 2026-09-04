pub mod settings_dialog;
pub mod unsaved_changes;

/// Renders a standard dialog footer with buttons aligned to the bottom-right.
///
/// # Arguments
/// - `ui` - The egui Ui.
/// - `add_contents` - A closure that adds buttons from right to left. This
///   keeps the footer compact while its complete button group is right-aligned.
///
/// # Example
/// ```rust
/// dialog_footer(ui, |ui| {
///     if ui.button("OK").clicked() { /* ... */ }
///     if ui.button("Cancel").clicked() { /* ... */ }
/// });
/// ```
pub fn dialog_footer<R>(
    ui: &mut eframe::egui::Ui,
    add_contents: impl FnOnce(&mut eframe::egui::Ui) -> R,
) -> eframe::egui::InnerResponse<R> {
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);
    ui.allocate_ui_with_layout(
        eframe::egui::vec2(ui.available_width(), 28.0),
        eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
        add_contents,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogButtonRole {
    Primary,
    Destructive,
    Secondary,
}

/// Applies one shared size and role treatment to dialog action buttons.
pub fn dialog_button(
    ui: &mut eframe::egui::Ui,
    label: impl Into<eframe::egui::WidgetText>,
    role: DialogButtonRole,
) -> eframe::egui::Response {
    let mut button = eframe::egui::Button::new(label).min_size(eframe::egui::vec2(104.0, 28.0));
    match role {
        DialogButtonRole::Primary => {
            button = button
                .fill(ui.visuals().selection.bg_fill)
                .stroke(ui.visuals().selection.stroke);
        }
        DialogButtonRole::Destructive => {
            button = button
                .fill(eframe::egui::Color32::from_rgb(132, 48, 52))
                .stroke(eframe::egui::Stroke::new(
                    1.0,
                    eframe::egui::Color32::from_rgb(205, 92, 92),
                ));
        }
        DialogButtonRole::Secondary => {}
    }
    ui.add(button)
}
