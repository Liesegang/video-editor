//! Managed Solid, Gradient, and Pattern material construction.

use skia_safe::{
    FilterMode, Matrix, Paint as SkPaint, PictureRecorder, Rect, Shader, TileMode, gradient_shader,
};

use crate::error::LibraryError;
use crate::model::property::{
    GradientGeometry, GradientSpread, GradientValue, Paint, PatternKind, PatternValue,
};
use crate::rendering::skia_working_surface::{self, SkiaSurfaceContract};

pub(super) struct PaintShaderFactory<'a> {
    surface_contract: &'a SkiaSurfaceContract,
    render_scale: f64,
}

impl<'a> PaintShaderFactory<'a> {
    pub(super) const fn new(surface_contract: &'a SkiaSurfaceContract, render_scale: f64) -> Self {
        Self {
            surface_contract,
            render_scale,
        }
    }

    pub(super) fn configure(
        &self,
        paint: &mut SkPaint,
        material: &Paint,
        geometry: Rect,
        opacity: f32,
        shader_local_matrix: Option<&Matrix>,
    ) -> Result<(), LibraryError> {
        match material {
            Paint::Solid(color) => {
                let (color, color_space) = skia_working_surface::authored_paint_color4f(
                    self.surface_contract,
                    color,
                    opacity,
                )?;
                paint.set_color4f(color, color_space.as_ref());
            }
            Paint::Gradient(gradient) => {
                let shader = self.gradient(gradient, geometry, opacity)?;
                paint.set_shader(match shader_local_matrix {
                    Some(matrix) => shader.with_local_matrix(matrix),
                    None => shader,
                });
            }
            Paint::Pattern(pattern) => {
                let shader = self.pattern(pattern, opacity)?;
                paint.set_shader(match shader_local_matrix {
                    Some(matrix) => shader.with_local_matrix(matrix),
                    None => shader,
                });
            }
        }
        Ok(())
    }

    pub(super) fn gradient(
        &self,
        gradient: &GradientValue,
        geometry: Rect,
        opacity: f32,
    ) -> Result<Shader, LibraryError> {
        let mut colors = Vec::with_capacity(gradient.stops().len());
        let mut color_space = None;
        for stop in gradient.stops() {
            let (color, stop_space) = skia_working_surface::authored_paint_color4f(
                self.surface_contract,
                stop.color(),
                opacity,
            )?;
            colors.push(color);
            color_space = color_space.or(stop_space);
        }
        let positions = gradient
            .stops()
            .iter()
            .map(|stop| stop.offset() as f32)
            .collect::<Vec<_>>();
        let point = |value: crate::model::property::Vec2| {
            (
                geometry.left + value.x.into_inner() as f32 * geometry.width(),
                geometry.top + value.y.into_inner() as f32 * geometry.height(),
            )
        };
        let tile = match gradient.spread() {
            GradientSpread::Pad => TileMode::Clamp,
            GradientSpread::Repeat => TileMode::Repeat,
            GradientSpread::Reflect => TileMode::Mirror,
        };
        let interpolation = gradient_shader::Interpolation::from(gradient_shader::Flags::default());
        match gradient.geometry() {
            GradientGeometry::Linear { start, end } => gradient_shader::linear_with_interpolation(
                (point(start), point(end)),
                (&colors, color_space),
                Some(positions.as_slice()),
                tile,
                interpolation,
                None,
            ),
            GradientGeometry::Radial { center, radius } => {
                gradient_shader::radial_with_interpolation(
                    (
                        point(center),
                        radius.into_inner() as f32 * geometry.width().min(geometry.height()),
                    ),
                    (&colors, color_space),
                    Some(positions.as_slice()),
                    tile,
                    interpolation,
                    None,
                )
            }
        }
        .ok_or_else(|| LibraryError::Render("Cannot create Gradient paint shader".to_string()))
    }

    pub(super) fn pattern(
        &self,
        pattern: &PatternValue,
        opacity: f32,
    ) -> Result<Shader, LibraryError> {
        let width = (pattern.scale().x.into_inner() * self.render_scale) as f32;
        let height = (pattern.scale().y.into_inner() * self.render_scale) as f32;
        let bounds = Rect::from_wh(width, height);
        let mut recorder = PictureRecorder::new();
        let tile = recorder.begin_recording(bounds, false);
        tile.save_layer_alpha_f(Some(bounds), opacity.clamp(0.0, 1.0));
        let material = |color: &crate::model::property::ColorValue| Paint::Solid(color.clone());
        let mut background = SkPaint::default();
        self.configure(
            &mut background,
            &material(pattern.background()),
            bounds,
            1.0,
            None,
        )?;
        tile.draw_rect(bounds, &background);
        let mut foreground = SkPaint::default();
        self.configure(
            &mut foreground,
            &material(pattern.foreground()),
            bounds,
            1.0,
            None,
        )?;
        let duty = pattern.duty() as f32;
        match pattern.kind() {
            PatternKind::Checker => {
                let x = width * duty;
                let y = height * duty;
                tile.draw_rect(Rect::from_xywh(0.0, 0.0, x, y), &foreground);
                tile.draw_rect(Rect::from_xywh(x, y, width - x, height - y), &foreground);
            }
            PatternKind::Stripes => {
                tile.draw_rect(Rect::from_xywh(0.0, 0.0, width * duty, height), &foreground);
            }
            PatternKind::Dots => {
                tile.draw_circle(
                    (width / 2.0, height / 2.0),
                    width.min(height) * duty / 2.0,
                    &foreground,
                );
            }
            PatternKind::Grid => {
                tile.draw_rect(Rect::from_xywh(0.0, 0.0, width * duty, height), &foreground);
                tile.draw_rect(Rect::from_xywh(0.0, 0.0, width, height * duty), &foreground);
            }
        }
        tile.restore();
        let picture = recorder
            .finish_recording_as_picture(Some(&bounds))
            .ok_or_else(|| LibraryError::Render("Cannot record Pattern paint".to_string()))?;
        let mut matrix = Matrix::translate((
            (pattern.phase().x.into_inner() * self.render_scale) as f32,
            (pattern.phase().y.into_inner() * self.render_scale) as f32,
        ));
        matrix.pre_rotate(pattern.angle() as f32, None);
        Ok(picture.to_shader(
            Some((TileMode::Repeat, TileMode::Repeat)),
            FilterMode::Nearest,
            Some(&matrix),
            Some(&bounds),
        ))
    }
}
