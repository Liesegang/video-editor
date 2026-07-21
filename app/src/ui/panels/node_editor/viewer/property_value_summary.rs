use eframe::egui::{self, Color32};
use library::model::path::{FillRule, PathValue};
use library::model::property::ColorValue;

use crate::ui::panels::node_editor::{bounded_non_selectable_label, non_selectable_label};

pub(super) fn render_color(ui: &mut egui::Ui, color: &ColorValue) -> egui::Response {
    let [r, g, b, a] = color.rgba();
    bounded_non_selectable_label(
        ui,
        format!("{r:.2},{g:.2},{b:.2},{a:.2} @ {}", color.color_space()),
        96.0,
        egui::Align::LEFT,
    )
    .on_hover_text(format!(
        "r={r}, g={g}, b={b}, a={a} @ {} (straight alpha)",
        color.color_space()
    ))
}

pub(super) fn render_path(ui: &mut egui::Ui, path: &PathValue) -> egui::Response {
    let contour_count = path.contours().len();
    let segment_count = path
        .contours()
        .iter()
        .map(|contour| contour.segments().len())
        .sum::<usize>();
    let closed_count = path
        .contours()
        .iter()
        .filter(|contour| contour.is_closed())
        .count();
    let fill_rule = match path.fill_rule() {
        FillRule::NonZero => "non-zero",
        FillRule::EvenOdd => "even-odd",
    };
    non_selectable_label(
        ui,
        egui::RichText::new(format!("Path · {contour_count} contours"))
            .small()
            .color(Color32::from_gray(125)),
    )
    .on_hover_text(format!(
        "{segment_count} segments · {closed_count}/{contour_count} closed · {fill_rule} fill"
    ))
}
