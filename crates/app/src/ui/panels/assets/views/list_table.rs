use super::*;

const LIST_ROW_MIN_HEIGHT: f32 = 44.0;
const LIST_ROW_GAP: f32 = 2.0;
const TABLE_ROW_HEIGHT: f32 = 30.0;
pub(super) const TABLE_WIDTH: f32 = 652.0;
pub(super) const TABLE_COLUMNS: [(&str, f32); 5] = [
    ("Name", 270.0),
    ("Kind", 104.0),
    ("Size", 104.0),
    ("FPS", 78.0),
    ("Duration", 96.0),
];

pub(super) fn list_entry(
    ui: &mut egui::Ui,
    entry: LibraryEntry<'_>,
    index: usize,
    state: &AuthoringUiState,
) -> Response {
    let width = ui.available_width().max(0.0);
    let metadata = entry.list_metadata();
    let text_width = (width - 43.0).max(1.0);
    let name_galley = ui.painter().layout(
        entry.name().to_string(),
        egui::FontId::proportional(13.0),
        ui.visuals().text_color(),
        text_width,
    );
    let metadata_galley = ui.painter().layout(
        metadata.clone(),
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
        text_width,
    );
    let row_height =
        (8.0 + name_galley.size().y + 1.0 + metadata_galley.size().y).max(LIST_ROW_MIN_HEIGHT);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, row_height), Sense::click_and_drag());
    paint_entry_background(ui, rect, &response, entry.selected(state), index, 3.0);
    let (icon, icon_color) = entry.icon();
    let icon_rect = Rect::from_min_size(rect.min, Vec2::new(29.0, LIST_ROW_MIN_HEIGHT));
    ui.painter().text(
        icon_rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(17.0),
        icon_color,
    );
    let name_position = egui::pos2(icon_rect.right() + 6.0, rect.top() + 4.0);
    let metadata_position = egui::pos2(
        name_position.x,
        name_position.y + name_galley.size().y + 1.0,
    );
    let metadata_rect = Rect::from_min_size(metadata_position, metadata_galley.size());
    ui.painter().galley(
        name_position,
        name_galley,
        ui.style()
            .interact_selectable(&response, entry.selected(state))
            .text_color(),
    );
    ui.painter().galley(
        metadata_position,
        metadata_galley,
        ui.visuals().weak_text_color(),
    );
    register_metadata(entry, metadata_rect, &metadata, rect, "list");
    ui.add_space(LIST_ROW_GAP);
    response
}

pub(super) fn table_header(ui: &mut egui::Ui, section_id: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(TABLE_WIDTH, 24.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 2.0, ui.visuals().faint_bg_color);
    let mut left = rect.left();
    for (name, width) in TABLE_COLUMNS {
        let cell = Rect::from_min_size(
            egui::pos2(left, rect.top()),
            Vec2::new(width, rect.height()),
        );
        paint_single_line(
            ui,
            cell,
            name,
            egui::FontId::proportional(11.0),
            ui.visuals().strong_text_color(),
            7.0,
        );
        let visible_cell = cell.intersect(ui.clip_rect());
        crate::qa::register_component_with_metadata(
            format!(
                "assets.table.column:{section_id}:{}",
                name.to_ascii_lowercase()
            ),
            "asset_table_column",
            visible_cell,
            true,
            Some(serde_json::json!({
                "label": name,
                "width": width,
                "elided_by_viewport": visible_cell.width() + 0.1 < cell.width(),
            })),
        );
        left += width;
    }
    crate::qa::register_component_with_metadata(
        format!("assets.table.columns:{section_id}"),
        "asset_table_header",
        rect,
        true,
        Some(serde_json::json!({
            "columns": TABLE_COLUMNS.map(|(name, _)| name),
            "column_widths": TABLE_COLUMNS.map(|(_, width)| width),
            "minimum_width": TABLE_WIDTH,
            "horizontal_scroll": true,
            "overflow": "horizontal_scroll",
        })),
    );
}

pub(super) fn table_entry(
    ui: &mut egui::Ui,
    entry: LibraryEntry<'_>,
    index: usize,
    state: &AuthoringUiState,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(TABLE_WIDTH, TABLE_ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    paint_entry_background(ui, rect, &response, entry.selected(state), index, 0.0);
    let values = [
        entry.name().to_string(),
        entry.kind().to_string(),
        entry.size(),
        entry.fps(),
        entry.duration(),
    ];
    let mut left = rect.left();
    for (column, ((_, width), value)) in TABLE_COLUMNS.into_iter().zip(values).enumerate() {
        let cell = Rect::from_min_size(
            egui::pos2(left, rect.top()),
            Vec2::new(width, rect.height()),
        );
        if column == 0 {
            let (icon, icon_color) = entry.icon();
            ui.painter().text(
                egui::pos2(cell.left() + 15.0, cell.center().y),
                egui::Align2::CENTER_CENTER,
                icon,
                egui::FontId::proportional(15.0),
                icon_color,
            );
            paint_single_line(
                ui,
                Rect::from_min_max(
                    egui::pos2(cell.left() + 30.0, cell.top()),
                    cell.right_bottom(),
                ),
                &value,
                egui::FontId::proportional(12.0),
                ui.visuals().text_color(),
                3.0,
            );
        } else {
            paint_single_line(
                ui,
                cell,
                &value,
                egui::FontId::proportional(12.0),
                if value == "--" {
                    ui.visuals().weak_text_color()
                } else {
                    ui.visuals().text_color()
                },
                7.0,
            );
        }
        left += width;
    }
    register_metadata(entry, rect, &entry.list_metadata(), rect, "table");
    response
}
