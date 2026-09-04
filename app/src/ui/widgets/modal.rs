use eframe::egui::{self, Align2, Color32, Context, Id, InnerResponse, Order, Vec2};

pub struct Modal<'a> {
    title: String,
    open: Option<&'a mut bool>,
    id: Id,
    resizable: bool,
    movable: bool,
    fixed_size: Option<Vec2>,
    min_width: Option<f32>,
    max_width: Option<f32>,
    exact_content_width: Option<f32>,
    anchor: Option<(Align2, Vec2)>,
}

impl<'a> Modal<'a> {
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            id: Id::new(&title),
            title,
            open: None,
            resizable: true,
            movable: true,
            fixed_size: None,
            min_width: None,
            max_width: None,
            exact_content_width: None,
            anchor: None,
        }
    }

    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Standard centered application dialog with a fixed content width.
    pub fn dialog(title: impl Into<String>, width: f32) -> Self {
        Self::new(title)
            .resizable(false)
            .movable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .min_width(width)
            .max_width(width)
            .exact_content_width(width)
    }

    pub fn movable(mut self, movable: bool) -> Self {
        self.movable = movable;
        self
    }

    pub fn anchor(mut self, align: Align2, offset: impl Into<Vec2>) -> Self {
        self.anchor = Some((align, offset.into()));
        self
    }

    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = Some(width);
        self
    }

    /// Gives both axes one stable content extent. Use this for structured
    /// dialogs whose children contain remainder-sized strips or tables: those
    /// widgets need a finite parent instead of participating in Window's
    /// content-driven auto-size feedback loop.
    pub fn fixed_size(mut self, size: impl Into<Vec2>) -> Self {
        self.fixed_size = Some(size.into());
        self
    }

    pub fn max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    fn exact_content_width(mut self, width: f32) -> Self {
        self.exact_content_width = Some(width);
        self
    }

    pub fn show<R>(
        self,
        ctx: &Context,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<InnerResponse<Option<R>>> {
        let is_open = if let Some(open) = &self.open {
            **open
        } else {
            true
        };

        if !is_open {
            return None;
        }

        // 1. Draw blocking backdrop
        egui::Area::new(self.id.with("backdrop"))
            .interactable(true)
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(Order::Middle)
            .show(ctx, |ui| {
                let screen_rect = ctx.input(|i| i.content_rect());
                ui.allocate_rect(screen_rect, egui::Sense::click());
                ui.painter()
                    .rect_filled(screen_rect, 0.0, Color32::from_black_alpha(100));
            });

        // 2. Draw Window on top
        let mut window = egui::Window::new(&self.title)
            .id(self.id)
            .resizable(self.resizable)
            .collapsible(false)
            .title_bar(true)
            .movable(self.movable)
            .order(Order::Foreground);

        if let Some(open) = self.open {
            window = window.open(open);
        }

        if let Some(size) = self.fixed_size {
            window = window.fixed_size(size);
        }
        if let Some(width) = self.min_width {
            window = window.min_width(width);
        }
        if let Some(width) = self.max_width {
            window = window.max_width(width);
        }

        // Use anchor if set (disables movement), otherwise default to center
        if let Some((align, offset)) = self.anchor {
            window = window.anchor(align, offset);
        } else {
            window = window.default_pos(ctx.input(|i| i.content_rect()).center());
        }

        // `Window::resizable(false)` still auto-resizes in egui. Giving a
        // child `available_width()` back to that auto-size loop can therefore
        // add frame padding again on every frame. Constrain the content Ui at
        // the shared dialog boundary so its measured width is idempotent.
        window.show(ctx, |ui| {
            if let Some(width) = self.exact_content_width {
                ui.set_width(width);
            }
            add_contents(ui)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_width_dialog_does_not_grow_across_frames() {
        let context = Context::default();
        let mut sizes = Vec::new();
        for _ in 0..12 {
            let mut measured = None;
            let _frame_output = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 720.0),
                    )),
                    ..Default::default()
                },
                |context| {
                    measured = Some(
                        Modal::dialog("Stable dialog", 440.0)
                            .show(context, |ui| {
                                ui.label("Stable content");
                                ui.allocate_ui_with_layout(
                                    egui::vec2(ui.available_width(), 28.0),
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let _cancel = ui.button("Cancel");
                                        let _save = ui.button("Save");
                                    },
                                );
                            })
                            .expect("dialog")
                            .response
                            .rect
                            .size(),
                    );
                },
            );
            sizes.push(measured.expect("measured dialog"));
        }
        let stable = sizes[2];
        for size in &sizes[3..] {
            assert!((size.x - stable.x).abs() < 0.01, "width grew: {sizes:?}");
            assert!((size.y - stable.y).abs() < 0.01, "height grew: {sizes:?}");
        }
    }

    #[test]
    fn fixed_size_modal_contains_remainder_layout_without_growing() {
        use egui_extras::{Size, StripBuilder};

        let context = Context::default();
        let mut sizes = Vec::new();
        for _ in 0..12 {
            let mut measured = None;
            let _frame_output = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 720.0),
                    )),
                    ..Default::default()
                },
                |context| {
                    measured = Some(
                        Modal::new("Stable structured dialog")
                            .fixed_size(egui::vec2(760.0, 540.0))
                            .resizable(false)
                            .show(context, |ui| {
                                StripBuilder::new(ui)
                                    .size(Size::exact(150.0))
                                    .size(Size::remainder())
                                    .horizontal(|mut strip| {
                                        strip.cell(|ui| {
                                            ui.label("Sidebar");
                                        });
                                        strip.cell(|ui| {
                                            StripBuilder::new(ui)
                                                .size(Size::remainder())
                                                .size(Size::exact(56.0))
                                                .vertical(|mut strip| {
                                                    strip.cell(|ui| {
                                                        ui.label("Content");
                                                    });
                                                    strip.cell(|ui| {
                                                        ui.label("Footer");
                                                    });
                                                });
                                        });
                                    });
                            })
                            .expect("dialog")
                            .response
                            .rect
                            .size(),
                    );
                },
            );
            sizes.push(measured.expect("measured dialog"));
        }
        let stable = sizes[2];
        for size in &sizes[3..] {
            assert!((size.x - stable.x).abs() < 0.01, "width grew: {sizes:?}");
            assert!((size.y - stable.y).abs() < 0.01, "height grew: {sizes:?}");
        }
    }
}
