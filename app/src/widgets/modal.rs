use eframe::egui::{self, Align2, Color32, Context, Id, InnerResponse, Order, Vec2};

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    #[test]
    fn test_modal_shows_content() {
        let harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(|ctx| {
                Modal::new("Test Dialog").show(ctx, |ui| {
                    ui.label("Hello from modal");
                });
            });
        // モーダルのコンテンツが表示されているか
        assert!(harness.query_by_label("Hello from modal").is_some());
    }

    #[test]
    fn test_modal_hidden_when_closed() {
        let harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(|ctx| {
                let mut open = false;
                Modal::new("Hidden Dialog").open(&mut open).show(ctx, |ui| {
                    ui.label("Should not appear");
                });
            });
        // open=false のためコンテンツは非表示
        assert!(harness.query_by_label("Should not appear").is_none());
    }

    #[test]
    fn test_modal_shows_title() {
        let harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(|ctx| {
                Modal::new("My Title").show(ctx, |ui| {
                    ui.label("content");
                });
            });
        // ウィンドウタイトルがアクセシビリティツリーに存在する
        assert!(harness.query_by_label("My Title").is_some());
    }

    // ── Domain: Complex Content ──

    #[test]
    fn test_modal_renders_multiple_labels() {
        let harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(|ctx| {
                Modal::new("Multi Content").show(ctx, |ui| {
                    ui.label("First line");
                    ui.label("Second line");
                    ui.label("Third line");
                });
            });
        assert!(harness.query_by_label("First line").is_some());
        assert!(harness.query_by_label("Second line").is_some());
        assert!(harness.query_by_label("Third line").is_some());
    }

    #[test]
    fn test_modal_renders_button() {
        let harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(|ctx| {
                Modal::new("Button Dialog").show(ctx, |ui| {
                    let _ = ui.button("OK");
                    let _ = ui.button("Cancel");
                });
            });
        assert!(harness.query_by_label("OK").is_some());
        assert!(harness.query_by_label("Cancel").is_some());
    }

    #[test]
    fn test_modal_fixed_size_renders_content() {
        let harness = Harness::builder()
            .with_size(egui::vec2(600.0, 500.0))
            .build(|ctx| {
                Modal::new("Fixed Size Dialog")
                    .fixed_size([300.0, 200.0])
                    .show(ctx, |ui| {
                        ui.label("Inside fixed modal");
                    });
            });
        assert!(harness.query_by_label("Inside fixed modal").is_some());
    }

    // ── Domain: Interaction (Dynamic / Selenium-style) ──

    #[test]
    fn button_click_inside_modal_detected() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let clicked = Rc::new(RefCell::new(false));
        let c = clicked.clone();

        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                let c2 = c.clone();
                Modal::new("Interactive Modal").show(ctx, |ui| {
                    if ui.button("Do Something").clicked() {
                        *c2.borrow_mut() = true;
                    }
                });
            });

        harness.get_by_label("Do Something").click();
        harness.run();

        assert!(*clicked.borrow());
    }

    #[test]
    fn multiple_buttons_track_individual_clicks() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let accept_count = Rc::new(RefCell::new(0u32));
        let reject_count = Rc::new(RefCell::new(0u32));
        let a = accept_count.clone();
        let r = reject_count.clone();

        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                let a2 = a.clone();
                let r2 = r.clone();
                Modal::new("Multi Button Modal").show(ctx, |ui| {
                    if ui.button("Accept").clicked() {
                        *a2.borrow_mut() += 1;
                    }
                    if ui.button("Reject").clicked() {
                        *r2.borrow_mut() += 1;
                    }
                });
            });

        // Click Accept
        harness.get_by_label("Accept").click();
        harness.run();

        assert_eq!(*accept_count.borrow(), 1);
        assert_eq!(*reject_count.borrow(), 0);

        // Click Reject
        harness.get_by_label("Reject").click();
        harness.run();

        assert_eq!(*accept_count.borrow(), 1);
        assert_eq!(*reject_count.borrow(), 1);
    }
}

pub struct Modal<'a> {
    title: String,
    open: Option<&'a mut bool>,
    id: Id,
    resizable: bool,
    collapsible: bool,
    movable: bool,
    fixed_size: Option<Vec2>,
    anchor: Option<(Align2, Vec2)>,
}

#[allow(dead_code)]
impl<'a> Modal<'a> {
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            id: Id::new(&title),
            title,
            open: None,
            resizable: true,
            collapsible: false,
            movable: true,
            fixed_size: None,
            anchor: None,
        }
    }

    // ... (builders same)

    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.collapsible = collapsible;
        self
    }

    pub fn fixed_size(mut self, size: impl Into<Vec2>) -> Self {
        self.fixed_size = Some(size.into());
        self
    }

    pub fn default_width(self, _width: f32) -> Self {
        self
    }

    pub fn min_width(self, _width: f32) -> Self {
        self
    }

    pub fn min_height(self, _height: f32) -> Self {
        self
    }

    #[allow(deprecated)]
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
                let screen_rect = ctx.input(|i| i.screen_rect());
                ui.allocate_rect(screen_rect, egui::Sense::click());
                ui.painter()
                    .rect_filled(screen_rect, 0.0, Color32::from_black_alpha(100));
            });

        // 2. Draw Window on top
        let mut window = egui::Window::new(&self.title)
            .id(self.id)
            .resizable(self.resizable)
            .collapsible(self.collapsible)
            .movable(self.movable)
            .order(Order::Foreground);

        if let Some(open) = self.open {
            window = window.open(open);
        }

        if let Some(size) = self.fixed_size {
            window = window.fixed_size(size);
        }

        // Use anchor if set (disables movement), otherwise default to center
        if let Some((align, offset)) = self.anchor {
            window = window.anchor(align, offset);
        } else {
            window = window.default_pos(ctx.input(|i| i.screen_rect()).center());
        }

        window.show(ctx, add_contents)
    }
}
