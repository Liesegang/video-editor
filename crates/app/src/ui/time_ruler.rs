//! Shared time-ruler surface for Timeline and Curve Editor.
//!
//! Domain panels provide positions and labels from their authoritative canvas
//! transform. This component owns one interaction and visual contract so
//! scrubbing does not drift from the grid underneath it.

use egui::{Pos2, Rect, Response, Sense, Stroke};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TimeRulerTick {
    pub x: f32,
    pub label: String,
}

pub(crate) struct TimeRuler<'a> {
    interaction_id: &'a str,
    qa_id: &'a str,
    ticks: &'a [TimeRulerTick],
    duration_x: Option<f32>,
}

impl<'a> TimeRuler<'a> {
    pub(crate) const fn new(
        interaction_id: &'a str,
        qa_id: &'a str,
        ticks: &'a [TimeRulerTick],
    ) -> Self {
        Self {
            interaction_id,
            qa_id,
            ticks,
            duration_x: None,
        }
    }

    pub(crate) const fn with_duration_x(mut self, duration_x: Option<f32>) -> Self {
        self.duration_x = duration_x;
        self
    }

    pub(crate) fn show(self, ui: &mut egui::Ui, rect: Rect) -> Response {
        let response = ui.interact(
            rect,
            ui.id().with(self.interaction_id),
            Sense::click_and_drag(),
        );
        crate::qa::register_component(self.qa_id, "time_ruler", rect);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, ui.visuals().panel_fill);
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        for tick in self.ticks {
            if tick.x < rect.left() || tick.x > rect.right() {
                continue;
            }
            painter.line_segment(
                [
                    Pos2::new(tick.x, rect.bottom() - 9.0),
                    Pos2::new(tick.x, rect.bottom()),
                ],
                Stroke::new(1.0, ui.visuals().weak_text_color()),
            );
            painter.text(
                Pos2::new(tick.x + 3.0, rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                &tick.label,
                egui::FontId::monospace(10.0),
                ui.visuals().weak_text_color(),
            );
        }
        if let Some(duration_x) = self
            .duration_x
            .filter(|x| rect.contains(Pos2::new(*x, rect.center().y)))
        {
            painter.line_segment(
                [
                    Pos2::new(duration_x, rect.top()),
                    Pos2::new(duration_x, rect.bottom()),
                ],
                Stroke::new(1.0, egui::Color32::from_rgb(150, 90, 90)),
            );
        }
        response
    }
}
