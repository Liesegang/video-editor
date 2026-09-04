pub mod settings_dialog;
pub mod unsaved_changes;

/// Renders a standard dialog footer with buttons aligned to the bottom-right.
///
/// # Arguments
/// - `ui` - The egui Ui.
/// - `add_contents` - A closure that adds buttons. Buttons should be added in
///   reverse order (right to left) because this helper uses
///   `Layout::right_to_left`.
///
/// # Example
/// ```rust
/// dialog_footer(ui, |ui| {
///     if ui.button("OK").clicked() { /* ... */ }
///     if ui.button("Cancel").clicked() { /* ... */ }
/// });
/// ```
pub fn dialog_footer(ui: &mut eframe::egui::Ui, add_contents: impl FnOnce(&mut eframe::egui::Ui)) {
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(5.0);
    ui.with_layout(
        eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
        add_contents,
    );
}
