use crate::model::frame::color::Color;
use skia_safe::{Canvas, Paint, Rect};

pub trait Decorator: Send + Sync {
    fn draw(&self, canvas: &Canvas, bounds: Rect, paint: &Paint);
}

/// Smart Backplate（自動背景）の対象レベル
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash)]
pub enum BackplateTarget {
    Char,  // 文字ごと
    Line,  // 行ごと
    Block, // 全体
    Parts, // パス/パーツごと（文字をグリフパスに分解）
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash)]
pub enum BackplateShape {
    Rect,
    RoundedRect,
    Circle,
}

pub struct BackplateDecorator {
    pub target: BackplateTarget,
    pub shape: BackplateShape,
    pub color: Color,
    pub padding: (f32, f32, f32, f32), // Top, Right, Bottom, Left
    pub corner_radius: f32,            // RoundedRect用
    pub follow_animation: bool,
}

impl BackplateDecorator {
    pub fn new(
        target: BackplateTarget,
        shape: BackplateShape,
        color: Color,
        padding: (f32, f32, f32, f32),
    ) -> Self {
        Self {
            target,
            shape,
            color,
            padding,
            corner_radius: 10.0,
            follow_animation: true,
        }
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    pub fn with_follow_animation(mut self, follow: bool) -> Self {
        self.follow_animation = follow;
        self
    }
}

impl Decorator for BackplateDecorator {
    fn draw(&self, canvas: &Canvas, bounds: Rect, paint: &Paint) {
        let padded = Rect::new(
            bounds.left - self.padding.3,   // Left
            bounds.top - self.padding.0,    // Top
            bounds.right + self.padding.1,  // Right
            bounds.bottom + self.padding.2, // Bottom
        );

        let mut fill_paint = paint.clone();
        fill_paint.set_color(skia_safe::Color::from_argb(
            self.color.a,
            self.color.r,
            self.color.g,
            self.color.b,
        ));
        fill_paint.set_anti_alias(true);

        match self.shape {
            BackplateShape::Rect => {
                canvas.draw_rect(padded, &fill_paint);
            }
            BackplateShape::RoundedRect => {
                let rrect =
                    skia_safe::RRect::new_rect_xy(padded, self.corner_radius, self.corner_radius);
                canvas.draw_rrect(rrect, &fill_paint);
            }
            BackplateShape::Circle => {
                canvas.draw_oval(padded, &fill_paint);
            }
        }
    }
}

pub struct RectDecorator {
    pub color: Color,
    pub padding: f32,
}

impl RectDecorator {
    pub fn new(color: Color, padding: f32) -> Self {
        Self { color, padding }
    }
}

impl Decorator for RectDecorator {
    fn draw(&self, canvas: &Canvas, bounds: Rect, paint: &Paint) {
        let padded = Rect::new(
            bounds.left - self.padding,
            bounds.top - self.padding,
            bounds.right + self.padding,
            bounds.bottom + self.padding,
        );

        let mut fill_paint = paint.clone();
        fill_paint.set_color(skia_safe::Color::from_argb(
            self.color.a,
            self.color.r,
            self.color.g,
            self.color.b,
        ));
        fill_paint.set_anti_alias(true);

        canvas.draw_rect(padded, &fill_paint);
    }
}

pub struct CircleDecorator {
    pub color: Color,
    pub padding: f32,
}

impl CircleDecorator {
    pub fn new(color: Color, padding: f32) -> Self {
        Self { color, padding }
    }
}

impl Decorator for CircleDecorator {
    fn draw(&self, canvas: &Canvas, bounds: Rect, paint: &Paint) {
        let padded = Rect::new(
            bounds.left - self.padding,
            bounds.top - self.padding,
            bounds.right + self.padding,
            bounds.bottom + self.padding,
        );

        let mut fill_paint = paint.clone();
        fill_paint.set_color(skia_safe::Color::from_argb(
            self.color.a,
            self.color.r,
            self.color.g,
            self.color.b,
        ));
        fill_paint.set_anti_alias(true);

        canvas.draw_oval(padded, &fill_paint);
    }
}
