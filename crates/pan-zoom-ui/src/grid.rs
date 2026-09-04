use egui::{Painter, Pos2, Rect, Vec2};

use crate::{CanvasTheme, CanvasTransform, GridStroke};

/// World axis represented by a grid line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridAxis {
    /// Constant world X, painted as a vertical line.
    X,
    /// Constant world Y, painted as a horizontal line.
    Y,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridLineKind {
    Minor,
    Major,
    Origin,
}

/// One deterministic grid primitive in both world and screen coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLine {
    pub axis: GridAxis,
    pub kind: GridLineKind,
    pub world_position: f32,
    pub screen_position: f32,
}

/// Grid spacing and density policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridConfig {
    /// World coordinate where the grid index is zero.
    pub origin: Pos2,
    /// Base world spacing before adaptive density scaling.
    pub minor_spacing: Vec2,
    /// Base major spacing. Each component should be an integer multiple of
    /// `minor_spacing` so major lines remain stable while zooming.
    pub major_spacing: Vec2,
    /// Adapt both spacings by a 1/2/5-decade multiplier to avoid denser lines.
    pub adaptive: bool,
    pub min_screen_spacing: f32,
    pub max_lines_per_axis: usize,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            origin: Pos2::ZERO,
            minor_spacing: Vec2::splat(20.0),
            major_spacing: Vec2::splat(100.0),
            adaptive: true,
            min_screen_spacing: 10.0,
            max_lines_per_axis: 4_096,
        }
    }
}

/// Generate vertical lines first, then horizontal lines, each in ascending
/// screen order. Identical inputs always produce identical vector order.
pub fn grid_lines(viewport: Rect, transform: CanvasTransform, config: GridConfig) -> Vec<GridLine> {
    if !viewport.is_finite() || transform.screen_rect_to_world(viewport).is_none() {
        return Vec::new();
    }

    let origin_screen = transform.world_to_screen(config.origin);

    let mut lines = Vec::new();
    append_axis_lines(
        &mut lines,
        GridAxis::X,
        viewport.x_range(),
        origin_screen.x,
        transform.state.zoom.x,
        config.origin.x,
        config.minor_spacing.x,
        config.major_spacing.x,
        config,
    );
    append_axis_lines(
        &mut lines,
        GridAxis::Y,
        viewport.y_range(),
        origin_screen.y,
        transform.state.zoom.y,
        config.origin.y,
        config.minor_spacing.y,
        config.major_spacing.y,
        config,
    );
    lines
}

#[allow(
    clippy::too_many_arguments,
    reason = "axis generation receives the already-separated scalar components of the public grid contract"
)]
fn append_axis_lines(
    output: &mut Vec<GridLine>,
    axis: GridAxis,
    visible: egui::Rangef,
    screen_translation: f32,
    zoom: f32,
    world_origin: f32,
    base_minor: f32,
    base_major: f32,
    config: GridConfig,
) {
    if !zoom.is_finite()
        || zoom == 0.0
        || !world_origin.is_finite()
        || !base_minor.is_finite()
        || base_minor <= 0.0
        || !base_major.is_finite()
        || base_major < base_minor
        || config.max_lines_per_axis == 0
    {
        return;
    }

    let major_every = (base_major / base_minor).round().max(1.0) as i64;
    let mut multiplier = if config.adaptive {
        nice_ceil_multiplier(
            finite_positive(config.min_screen_spacing, 10.0) / (base_minor * zoom.abs()),
        )
    } else {
        1.0
    };
    if !multiplier.is_finite() || multiplier <= 0.0 {
        multiplier = 1.0;
    }

    let mut minor = base_minor * multiplier;
    let mut screen_step = minor * zoom;
    if !minor.is_finite() || !screen_step.is_finite() || screen_step == 0.0 {
        return;
    }

    let origin_screen = screen_translation;
    let mut bounds = visible_indices(visible, origin_screen, screen_step);
    for _ in 0..16 {
        let Some((first, last)) = bounds else {
            return;
        };
        if line_count(first, last) <= config.max_lines_per_axis {
            break;
        }
        minor *= 2.0;
        screen_step *= 2.0;
        bounds = visible_indices(visible, origin_screen, screen_step);
    }
    let Some((first, last)) = bounds else {
        return;
    };
    if line_count(first, last) > config.max_lines_per_axis {
        return;
    }

    let mut append_line = |index: i64| {
        let world_position = world_origin + index as f32 * minor;
        let screen_position = origin_screen + index as f32 * screen_step;
        if !world_position.is_finite() || !screen_position.is_finite() {
            return;
        }
        let kind = if index == 0 {
            GridLineKind::Origin
        } else if index.rem_euclid(major_every) == 0 {
            GridLineKind::Major
        } else {
            GridLineKind::Minor
        };
        output.push(GridLine {
            axis,
            kind,
            world_position,
            screen_position,
        });
    };
    if screen_step > 0.0 {
        for index in first..=last {
            append_line(index);
        }
    } else {
        for index in (first..=last).rev() {
            append_line(index);
        }
    }
}

fn visible_indices(visible: egui::Rangef, origin: f32, step: f32) -> Option<(i64, i64)> {
    if !visible.min.is_finite()
        || !visible.max.is_finite()
        || !origin.is_finite()
        || !step.is_finite()
        || step == 0.0
    {
        return None;
    }
    let (first, last) = if step > 0.0 {
        (
            ((visible.min - origin) / step).ceil() as i64,
            ((visible.max - origin) / step).floor() as i64,
        )
    } else {
        (
            ((visible.max - origin) / step).ceil() as i64,
            ((visible.min - origin) / step).floor() as i64,
        )
    };
    (first <= last).then_some((first, last))
}

fn line_count(first: i64, last: i64) -> usize {
    last.saturating_sub(first)
        .saturating_add(1)
        .try_into()
        .unwrap_or(usize::MAX)
}

fn nice_ceil_multiplier(target: f32) -> f32 {
    if !target.is_finite() || target <= 0.0 {
        return 1.0;
    }
    let decade = 10.0_f32.powf(target.log10().floor());
    let normalized = target / decade;
    let step = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    decade * step
}

fn finite_positive(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

/// Fill a canvas and paint its generated grid in screen space.
pub fn paint_canvas(
    painter: &Painter,
    viewport: Rect,
    transform: CanvasTransform,
    grid: GridConfig,
    theme: CanvasTheme,
) {
    painter.rect_filled(viewport, 0.0, theme.background);
    for line in grid_lines(viewport, transform, grid) {
        let stroke = stroke_for_kind(line.kind, theme);
        match line.axis {
            GridAxis::X => painter.line_segment(
                [
                    egui::pos2(line.screen_position, viewport.min.y),
                    egui::pos2(line.screen_position, viewport.max.y),
                ],
                egui::Stroke::new(stroke.width, stroke.color),
            ),
            GridAxis::Y => painter.line_segment(
                [
                    egui::pos2(viewport.min.x, line.screen_position),
                    egui::pos2(viewport.max.x, line.screen_position),
                ],
                egui::Stroke::new(stroke.width, stroke.color),
            ),
        };
    }
}

fn stroke_for_kind(kind: GridLineKind, theme: CanvasTheme) -> GridStroke {
    match kind {
        GridLineKind::Minor => theme.minor_grid,
        GridLineKind::Major => theme.major_grid,
        GridLineKind::Origin => theme.origin_grid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_grid_generates_major_minor_and_origin_lines() {
        let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 80.0));
        let config = GridConfig {
            minor_spacing: Vec2::splat(10.0),
            major_spacing: Vec2::splat(50.0),
            adaptive: false,
            ..GridConfig::default()
        };
        let lines = grid_lines(
            viewport,
            CanvasTransform::new(
                Pos2::ZERO,
                crate::CanvasState::uniform(egui::vec2(20.0, 10.0), 1.0),
            ),
            config,
        );

        assert!(lines.iter().any(|line| {
            line.axis == GridAxis::X
                && line.kind == GridLineKind::Origin
                && line.screen_position == 20.0
        }));
        assert!(lines.iter().any(|line| {
            line.axis == GridAxis::X
                && line.kind == GridLineKind::Major
                && line.world_position == 50.0
        }));
        assert!(lines
            .iter()
            .any(|line| line.axis == GridAxis::Y && line.kind == GridLineKind::Minor));
    }

    #[test]
    fn grid_generation_is_deterministic_under_negative_pan_and_xy_zoom() {
        let viewport = Rect::from_min_max(egui::pos2(5.0, 7.0), egui::pos2(321.0, 198.0));
        let state = crate::CanvasState::new(egui::vec2(-113.25, 47.5), egui::vec2(3.5, -0.75));
        let config = GridConfig {
            origin: egui::pos2(2.0, -4.0),
            minor_spacing: egui::vec2(0.5, 2.0),
            major_spacing: egui::vec2(2.5, 10.0),
            ..GridConfig::default()
        };

        let transform = CanvasTransform::new(egui::pos2(11.0, 23.0), state);
        let first = grid_lines(viewport, transform, config);
        let second = grid_lines(viewport, transform, config);

        assert_eq!(first, second);
        assert!(!first.is_empty());
        assert!(first.len() <= config.max_lines_per_axis * 2);
        assert!(first.windows(2).all(|pair| pair[0].axis != pair[1].axis
            || pair[0].screen_position <= pair[1].screen_position));
    }

    #[test]
    fn adaptive_grid_remains_bounded_at_extreme_zoom() {
        let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(4_000.0, 2_000.0));
        let config = GridConfig {
            max_lines_per_axis: 512,
            ..GridConfig::default()
        };
        let lines = grid_lines(
            viewport,
            CanvasTransform::new(
                Pos2::ZERO,
                crate::CanvasState::new(Vec2::ZERO, egui::vec2(1.0e-8, 1.0e8)),
            ),
            config,
        );

        assert!(lines.len() <= config.max_lines_per_axis * 2);
        assert!(lines.iter().all(|line| line.screen_position.is_finite()));
    }
}
