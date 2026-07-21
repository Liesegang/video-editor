use eframe::egui::{self, Color32};

use crate::ui::panels::node_editor::{
    GraphItem, PORT_LABEL_WIDTH, PORT_ROW_HEIGHT, PROPERTY_LABEL_WIDTH,
};

pub(in crate::ui::panels::node_editor) fn continuous_response_finished(
    ui: &egui::Ui,
    response: &egui::Response,
) -> bool {
    response.drag_stopped()
        || response.lost_focus()
        || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
}

pub(in crate::ui::panels::node_editor) fn continuous_color_edit_button(
    ui: &mut egui::Ui,
    color: &mut Color32,
) -> (egui::Response, bool) {
    // `color_edit_button_srgba` derives its popup id from the current auto id
    // with the same salt. Observe that public popup state so closing the
    // picker becomes the history commit boundary, even on a frame where the
    // color itself no longer changes.
    let popup_id = ui.auto_id_with("popup");
    let was_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let response = ui.color_edit_button_srgba(color);
    let closed = was_open && !egui::Popup::is_id_open(ui.ctx(), popup_id);
    (response, closed)
}

pub(in crate::ui::panels::node_editor) fn non_selectable_label(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.add(egui::Label::new(text).selectable(false))
}

pub(in crate::ui::panels::node_editor) fn property_label(
    ui: &mut egui::Ui,
    text: impl Into<String>,
) -> egui::Response {
    bounded_non_selectable_label(ui, text, PROPERTY_LABEL_WIDTH, egui::Align::LEFT)
}

pub(in crate::ui::panels::node_editor) fn bounded_non_selectable_label(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    width: f32,
    align: egui::Align,
) -> egui::Response {
    ui.add_sized(
        [width, PORT_ROW_HEIGHT],
        egui::Label::new(text.into())
            .selectable(false)
            .truncate()
            .halign(align),
    )
}

pub(in crate::ui::panels::node_editor) fn port_label_width(_item: Option<GraphItem>) -> f32 {
    PORT_LABEL_WIDTH
}

pub(in crate::ui::panels::node_editor) fn strong_non_selectable_label(
    ui: &mut egui::Ui,
    text: impl Into<String>,
) -> egui::Response {
    ui.add(egui::Label::new(egui::RichText::new(text.into()).strong()).selectable(false))
}
