use eframe::egui::{self, Align2, Color32, Context, Id, InnerResponse, Order, Vec2};

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
