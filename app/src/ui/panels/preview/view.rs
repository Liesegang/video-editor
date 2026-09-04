//! Preview camera, controls, and canvas painting.
//!
//! The image, ROI, grid, and pointer navigation all consume one
//! [`CanvasTransform`]. This keeps the shared `pan-zoom-ui` crate as the sole
//! authority for world/screen mapping.

use egui_phosphor::regular as icons;
use library::model::authoring::Timeline;
use library::model::frame::frame::Region;
use pan_zoom_ui::{AxisMask, CanvasState, CanvasTransform, NavigationConfig};

use crate::state::authoring::{AuthoringPreviewView, AuthoringUiState, PreviewTool};
use crate::ui::viewport::{ViewportController, ViewportInputPolicy, ViewportState, ZoomPolicy};

use super::register_button_qa;

const FIT_PADDING: f32 = 24.0;
const MIN_ZOOM: f32 = 0.0001;
const MAX_ZOOM: f32 = 1000.0;
const CHECKER_SIZE: f32 = 12.0;

pub(super) fn toolbar(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    state: &mut AuthoringUiState,
    text_tool_enabled: bool,
    path_tool_enabled: bool,
) {
    if !text_tool_enabled && state.preview.active_tool == PreviewTool::Text {
        state.preview.active_tool = PreviewTool::Select;
    }
    if !path_tool_enabled && state.preview.active_tool == PreviewTool::Path {
        state.preview.active_tool = PreviewTool::Select;
        state.preview.path_editor.cancel_drag();
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal_centered(|ui| {
            ui.style_mut().spacing.item_spacing = egui::vec2(4.0, 0.0);
            tool_button(
                ui,
                state,
                PreviewTool::Select,
                icons::CURSOR,
                "select",
                true,
            );
            tool_button(
                ui,
                state,
                PreviewTool::Text,
                icons::TEXT_T,
                "text",
                text_tool_enabled,
            );
            tool_button(
                ui,
                state,
                PreviewTool::Path,
                icons::BEZIER_CURVE,
                "path",
                path_tool_enabled,
            );
            tool_button(ui, state, PreviewTool::Pan, icons::HAND, "pan", true);
            tool_button(
                ui,
                state,
                PreviewTool::Zoom,
                icons::MAGNIFYING_GLASS,
                "zoom",
                true,
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(format!("{:.0}%", state.preview.canvas.zoom.x * 100.0));

                let grid = ui
                    .add(
                        egui::Button::new(egui::RichText::new(icons::GRID_FOUR).size(18.0))
                            .selected(state.preview.show_grid),
                    )
                    .on_hover_text("Toggle canvas grid");
                register_button_qa("preview.grid", "grid", &grid, state.preview.show_grid);
                if grid.clicked() {
                    state.preview.show_grid = !state.preview.show_grid;
                }

                let fit = ui
                    .add(egui::Button::new(
                        egui::RichText::new(icons::FRAME_CORNERS).size(18.0),
                    ))
                    .on_hover_text("Fit Timeline to Preview");
                register_button_qa("preview.fit", "fit", &fit, false);
                if fit.clicked() {
                    state.preview.auto_fit = true;
                    state.preview.fitted_timeline = None;
                }

                ui.separator();
            });
        });
    });
}

fn tool_button(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    tool: PreviewTool,
    icon: &str,
    name: &str,
    enabled: bool,
) {
    let selected = state.preview.active_tool == tool;
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new(egui::RichText::new(icon).size(18.0)).selected(selected),
        )
        .on_hover_text(format!("{} Tool", title_case(name)));
    register_button_qa(&format!("preview.tool.{name}"), name, &response, selected);
    if response.clicked() {
        state.preview.active_tool = tool;
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub(super) fn navigate(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    view: &mut AuthoringPreviewView,
) -> egui::Response {
    let mut handled_pan = false;
    let pan_tool_active = view.active_tool == PreviewTool::Pan;
    let zoom_tool_active = view.active_tool == PreviewTool::Zoom;
    let config = NavigationConfig {
        input_policy: ViewportInputPolicy::Trackpad,
        zoom_policy: ZoomPolicy::Uniform,
        pan_axes: AxisMask::BOTH,
        zoom_axes: AxisMask::BOTH,
        min_zoom: egui::Vec2::splat(MIN_ZOOM),
        max_zoom: egui::Vec2::splat(MAX_ZOOM),
        ..NavigationConfig::default()
    };
    let (changed, response) = ViewportController::new(
        ui,
        ui.make_persistent_id("preview.viewport"),
        Some(egui::Key::Space),
    )
    .with_config(config)
    .with_pan_tool_active(pan_tool_active)
    .with_zoom_tool_active(zoom_tool_active)
    .interact_with_rect(viewport, view, &mut handled_pan);
    if changed {
        view.auto_fit = false;
    }
    response
}

impl ViewportState for AuthoringPreviewView {
    fn canvas_state(&self) -> CanvasState {
        self.canvas
    }

    fn set_canvas_state(&mut self, state: CanvasState) {
        self.canvas = state;
    }
}

pub(super) fn update_fit(
    view: &mut AuthoringPreviewView,
    timeline: &Timeline,
    viewport: egui::Rect,
) {
    let timeline_changed = view.fitted_timeline != Some(timeline.id);
    if timeline_changed {
        view.fitted_timeline = Some(timeline.id);
        view.auto_fit = true;
    }
    let resized = (view.last_viewport_size - viewport.size()).length_sq() > 0.25;
    view.last_viewport_size = viewport.size();
    if !view.auto_fit || (!timeline_changed && !resized) {
        return;
    }
    let size = viewport.size();
    let padding = egui::vec2(FIT_PADDING.min(size.x * 0.1), FIT_PADDING.min(size.y * 0.1));
    if let Some(fitted) = pan_zoom_ui::fit_canvas(
        viewport,
        egui::vec2(timeline.width as f32, timeline.height as f32),
        padding,
        MIN_ZOOM,
        MAX_ZOOM,
    ) {
        view.canvas = fitted.state;
    }
}

pub(super) fn preview_content_rect(transform: CanvasTransform, canvas: egui::Vec2) -> egui::Rect {
    transform
        .world_rect_to_screen(egui::Rect::from_min_size(egui::Pos2::ZERO, canvas))
        .unwrap_or(egui::Rect::NOTHING)
}

pub(super) fn preview_canvas_transform(
    viewport: egui::Rect,
    view: &AuthoringPreviewView,
) -> CanvasTransform {
    CanvasTransform::new(viewport.min, view.canvas)
}

pub(super) fn visible_region(
    viewport: egui::Rect,
    transform: CanvasTransform,
    canvas_size: egui::Vec2,
) -> Option<Region> {
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, canvas_size);
    let content = transform.world_rect_to_screen(canvas)?;
    let visible = viewport.intersect(content);
    if !visible.is_positive() {
        return None;
    }
    let world = transform.screen_rect_to_world(visible)?.intersect(canvas);
    if !world.is_positive() {
        return None;
    }
    Some(Region {
        x: f64::from(world.min.x),
        y: f64::from(world.min.y),
        width: f64::from(world.width()),
        height: f64::from(world.height()),
    })
}

pub(super) fn paint_empty_preview(ui: &egui::Ui, viewport: egui::Rect, state: &AuthoringUiState) {
    paint_preview_background(
        ui,
        viewport,
        viewport,
        preview_canvas_transform(viewport, &state.preview),
        state.preview.show_grid,
    );
}

pub(super) fn paint_preview_background(
    ui: &egui::Ui,
    viewport: egui::Rect,
    content: egui::Rect,
    transform: CanvasTransform,
    show_grid: bool,
) {
    let painter = ui.painter().with_clip_rect(viewport);
    if show_grid {
        pan_zoom_ui::paint_canvas(
            &painter,
            viewport,
            transform,
            pan_zoom_ui::GridConfig::default(),
            pan_zoom_ui::CanvasTheme::default(),
        );
    } else {
        painter.rect_filled(
            viewport,
            0.0,
            pan_zoom_ui::CanvasTheme::default().background,
        );
    }
    paint_checkerboard(&painter, viewport.intersect(content), content.min);
}

fn paint_checkerboard(painter: &egui::Painter, clipped: egui::Rect, origin: egui::Pos2) {
    if !clipped.is_positive() {
        return;
    }
    let first_column = ((clipped.min.x - origin.x) / CHECKER_SIZE).floor() as i64;
    let first_row = ((clipped.min.y - origin.y) / CHECKER_SIZE).floor() as i64;
    let columns = (clipped.width() / CHECKER_SIZE).ceil() as usize + 2;
    let rows = (clipped.height() / CHECKER_SIZE).ceil() as usize + 2;
    for row_offset in 0..rows {
        let row = first_row.saturating_add(row_offset as i64);
        let y = origin.y + row as f32 * CHECKER_SIZE;
        for column_offset in 0..columns {
            let column = first_column.saturating_add(column_offset as i64);
            let x = origin.x + column as f32 * CHECKER_SIZE;
            let color = if (row + column).rem_euclid(2) == 0 {
                egui::Color32::from_gray(50)
            } else {
                egui::Color32::from_gray(67)
            };
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, y), egui::Vec2::splat(CHECKER_SIZE))
                    .intersect(clipped),
                0.0,
                color,
            );
        }
    }
}

#[cfg(test)]
pub(super) fn preview_fit_transform(
    viewport: egui::Rect,
    canvas: egui::Vec2,
) -> Option<CanvasTransform> {
    pan_zoom_ui::fit_canvas(
        viewport,
        canvas,
        egui::Vec2::splat(FIT_PADDING),
        MIN_ZOOM,
        MAX_ZOOM,
    )
}
