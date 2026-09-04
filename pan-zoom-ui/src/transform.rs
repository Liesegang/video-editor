use egui::{Pos2, Rect, Vec2};

use crate::CanvasState;

/// The canonical mapping between canvas world coordinates and screen space.
///
/// All canvas consumers (content, grid, hit testing, and partial renders) must
/// share one value so pan or zoom cannot be applied to only part of a surface.
/// The mapping is:
///
/// `screen = screen_origin + pan + world * zoom`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTransform {
    pub screen_origin: Pos2,
    pub state: CanvasState,
}

impl CanvasTransform {
    pub const fn new(screen_origin: Pos2, state: CanvasState) -> Self {
        Self {
            screen_origin,
            state,
        }
    }

    pub fn world_to_screen(self, world: Pos2) -> Pos2 {
        self.screen_origin + self.state.pan + world.to_vec2() * self.state.zoom
    }

    pub fn screen_to_world(self, screen: Pos2) -> Option<Pos2> {
        if !self.is_valid() || !screen.is_finite() {
            return None;
        }
        let world = (screen - self.screen_origin - self.state.pan) / self.state.zoom;
        world.is_finite().then_some(Pos2::new(world.x, world.y))
    }

    pub fn world_rect_to_screen(self, world: Rect) -> Option<Rect> {
        if !self.is_valid() || !world.is_finite() {
            return None;
        }
        let min = self.world_to_screen(world.min);
        let max = self.world_to_screen(world.max);
        (min.is_finite() && max.is_finite()).then_some(Rect::from_two_pos(min, max))
    }

    pub fn screen_rect_to_world(self, screen: Rect) -> Option<Rect> {
        if !screen.is_finite() {
            return None;
        }
        let min = self.screen_to_world(screen.min)?;
        let max = self.screen_to_world(screen.max)?;
        Some(Rect::from_two_pos(min, max))
    }

    fn is_valid(self) -> bool {
        self.screen_origin.is_finite()
            && self.state.pan.is_finite()
            && self.state.zoom.is_finite()
            && self.state.zoom.x != 0.0
            && self.state.zoom.y != 0.0
    }
}

/// Fit a world-space canvas into a viewport, returning the same transform
/// representation used for navigation and painting.
pub fn fit_canvas(
    viewport: Rect,
    canvas_size: Vec2,
    padding: Vec2,
    min_zoom: f32,
    max_zoom: f32,
) -> Option<CanvasTransform> {
    let viewport_size = viewport.size();
    if !viewport.is_finite()
        || !viewport_size.is_finite()
        || !canvas_size.is_finite()
        || !padding.is_finite()
        || viewport_size.x <= 0.0
        || viewport_size.y <= 0.0
        || canvas_size.x <= 0.0
        || canvas_size.y <= 0.0
        || min_zoom <= 0.0
        || !min_zoom.is_finite()
        || max_zoom < min_zoom
        || !max_zoom.is_finite()
    {
        return None;
    }
    let padding = egui::vec2(
        padding.x.max(0.0).min(viewport_size.x * 0.5),
        padding.y.max(0.0).min(viewport_size.y * 0.5),
    );
    let available = egui::vec2(
        (viewport_size.x - padding.x * 2.0).max(f32::EPSILON),
        (viewport_size.y - padding.y * 2.0).max(f32::EPSILON),
    );
    let zoom = (available.x / canvas_size.x)
        .min(available.y / canvas_size.y)
        .clamp(min_zoom, max_zoom);
    let pan = (viewport_size - canvas_size * zoom) * 0.5;
    Some(CanvasTransform::new(
        viewport.min,
        CanvasState::uniform(pan, zoom),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1.0e-4,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn world_and_screen_rects_round_trip_with_pan_and_zoom() {
        let transform = CanvasTransform::new(
            egui::pos2(100.0, 50.0),
            CanvasState::new(egui::vec2(-20.0, 30.0), egui::vec2(2.0, 4.0)),
        );
        let world = Rect::from_min_size(egui::pos2(10.0, 5.0), egui::vec2(40.0, 20.0));
        let screen = transform.world_rect_to_screen(world).expect("screen rect");
        let restored = transform
            .screen_rect_to_world(screen)
            .expect("round-trip world rect");

        assert_near(screen.min.x, 100.0);
        assert_near(screen.min.y, 100.0);
        assert_near(restored.min.x, world.min.x);
        assert_near(restored.min.y, world.min.y);
        assert_near(restored.max.x, world.max.x);
        assert_near(restored.max.y, world.max.y);
    }

    #[test]
    fn fit_centers_canvas_and_preserves_aspect_ratio() {
        let viewport = Rect::from_min_size(egui::pos2(20.0, 40.0), egui::vec2(1000.0, 600.0));
        let transform = fit_canvas(
            viewport,
            egui::vec2(1920.0, 1080.0),
            Vec2::splat(24.0),
            0.0001,
            1000.0,
        )
        .expect("fit transform");
        let content = transform
            .world_rect_to_screen(Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0)))
            .expect("content rect");

        assert!((content.center() - viewport.center()).length() <= 0.001);
        assert!(content.width() <= viewport.width());
        assert!(content.height() <= viewport.height());
        assert_near(content.width() / content.height(), 16.0 / 9.0);
    }
}
