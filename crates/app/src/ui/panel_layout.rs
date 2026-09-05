//! Shared geometry for editor panels with a fixed footer.
//!
//! The parent UI allocates its remaining rectangle exactly once. Body and
//! footer children then render inside that allocation, so neither a scroll
//! area nor the separator can grow the enclosing dock panel.

use egui::{Rect, Sense, Ui};

const FOOTER_SEPARATOR_HEIGHT: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelRegions {
    pub body: Rect,
    pub footer: Rect,
}

/// Allocate the remaining panel area and split it into a flexible body and a
/// fixed-height footer. A standard horizontal separator is painted in the gap.
pub(crate) fn allocate_panel_with_footer(ui: &mut Ui, footer_height: f32) -> PanelRegions {
    let available = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(available, Sense::hover());
    let panel = response.rect;

    let footer_height = footer_height.max(0.0).min(panel.height());
    let separator_height = FOOTER_SEPARATOR_HEIGHT.min(panel.height() - footer_height);
    let footer_top = panel.bottom() - footer_height;
    let body_bottom = footer_top - separator_height;

    let body = Rect::from_min_max(panel.min, egui::pos2(panel.right(), body_bottom));
    let footer = Rect::from_min_max(
        egui::pos2(panel.left(), footer_top),
        egui::pos2(panel.right(), panel.bottom()),
    );

    if separator_height > 0.0 {
        ui.painter().hline(
            panel.x_range(),
            body_bottom + separator_height * 0.5,
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
    }

    PanelRegions { body, footer }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    struct ScrollLayout {
        available: Rect,
        regions: PanelRegions,
        content_height: f32,
        viewport_height: f32,
    }

    fn layout_at_height(height: f32) -> (Rect, PanelRegions) {
        let context = egui::Context::default();
        let mut available = Rect::NOTHING;
        let mut regions = PanelRegions {
            body: Rect::NOTHING,
            footer: Rect::NOTHING,
        };
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, height),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    available = ui.available_rect_before_wrap();
                    regions = allocate_panel_with_footer(ui, 31.0);
                });
            },
        ));
        (available, regions)
    }

    #[test]
    fn footer_and_body_stay_inside_one_available_allocation() {
        let (available, regions) = layout_at_height(240.0);
        assert_eq!(regions.body.min, available.min);
        assert_eq!(regions.footer.bottom(), available.bottom());
        assert_eq!(regions.footer.height(), 31.0);
        assert!(regions.body.bottom() <= regions.footer.top());
        assert!(available.contains_rect(regions.body));
        assert!(available.contains_rect(regions.footer));
    }

    #[test]
    fn short_panels_clamp_the_footer_without_leaking() {
        let (available, regions) = layout_at_height(20.0);
        assert!(regions.body.height() >= 0.0);
        assert!(regions.footer.height() <= available.height());
        assert_eq!(regions.footer.bottom(), available.bottom());
        assert!(available.contains_rect(regions.body));
        assert!(available.contains_rect(regions.footer));
    }

    fn scroll_layout_at_height(panel_height: f32, content_height: f32) -> ScrollLayout {
        let context = egui::Context::default();
        let mut result = ScrollLayout {
            available: Rect::NOTHING,
            regions: PanelRegions {
                body: Rect::NOTHING,
                footer: Rect::NOTHING,
            },
            content_height: 0.0,
            viewport_height: 0.0,
        };
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, panel_height),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    result.available = ui.available_rect_before_wrap();
                    result.regions = allocate_panel_with_footer(ui, 31.0);
                    let scroll = ui
                        .scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(result.regions.body)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |ui| {
                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.allocate_space(egui::vec2(40.0, content_height));
                                    })
                            },
                        )
                        .inner;
                    result.content_height = scroll.content_size.y;
                    result.viewport_height = scroll.inner_rect.height();
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(result.regions.footer)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| {
                            drop(ui.button("Import"));
                        },
                    );
                });
            },
        ));
        result
    }

    #[test]
    fn body_scrolls_only_when_content_exceeds_its_viewport() {
        let ample = scroll_layout_at_height(300.0, 24.0);
        assert!(ample.content_height <= ample.viewport_height);
        assert!(ample.available.contains_rect(ample.regions.footer));

        let short = scroll_layout_at_height(100.0, 400.0);
        assert!(short.content_height > short.viewport_height);
        assert!(short.available.contains_rect(short.regions.footer));
        assert!(short.regions.body.bottom() <= short.regions.footer.top());
    }
}
