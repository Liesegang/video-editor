//! One resolved path body for plain and opacity-grouped Shape rasterization.

use skia_safe::{Canvas, Paint, Path, Rect};

use super::paint::{PaintFactory, StrokeRenderConfig};
use crate::error::LibraryError;
use crate::model::frame::appearance::{appearance_outsets, path_effect_outset};
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::entity::{FramePathPart, StyleConfig};
use crate::rendering::skia_working_surface::{self, SkiaSurfaceContract};

struct ResolvedPathPart {
    path: Path,
    opacity: f32,
}

/// Resolves aggregate and grouped path geometry through the single canonical
/// PathValue/SVG boundary, then paints one ordered vector body.
pub(super) struct PathBody {
    aggregate: Path,
    parts: Vec<ResolvedPathPart>,
}

impl PathBody {
    pub(super) fn resolve(
        canonical_path: Option<&crate::model::path::PathValue>,
        path_data: &str,
        parts: &[FramePathPart],
    ) -> Result<Self, LibraryError> {
        let aggregate =
            super::super::path_geometry::resolve_renderer_path(canonical_path, path_data)?;
        let parts = parts
            .iter()
            .map(|part| {
                let opacity = part.opacity.into_inner();
                if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                    return Err(LibraryError::Render(format!(
                        "Grouped path opacity must be finite and within 0..=1, got {opacity}"
                    )));
                }
                Ok(ResolvedPathPart {
                    path: super::super::path_geometry::resolve_renderer_path(
                        part.canonical_path.as_ref(),
                        &part.path,
                    )?,
                    opacity,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { aggregate, parts })
    }

    pub(super) fn bounds(&self) -> Rect {
        if self.parts.is_empty() {
            return self.aggregate.compute_tight_bounds();
        }
        self.parts
            .iter()
            .filter(|part| !part.path.is_empty())
            .map(|part| part.path.compute_tight_bounds())
            .reduce(|bounds, part| {
                Rect::new(
                    bounds.left.min(part.left),
                    bounds.top.min(part.top),
                    bounds.right.max(part.right),
                    bounds.bottom.max(part.bottom),
                )
            })
            .unwrap_or_else(Rect::new_empty)
    }

    pub(super) fn draw_body(
        &self,
        contract: &SkiaSurfaceContract,
        canvas: &Canvas,
        path_effects: &[PathEffect],
        styles: &[StyleConfig],
    ) -> Result<(), LibraryError> {
        self.visit_paths(
            canvas,
            appearance_outsets(styles).body + path_effect_outset(path_effects) + 1.0,
            |canvas, path| draw_path_body(contract, canvas, path, path_effects, styles),
        )
    }

    pub(super) fn draw_silhouette(
        &self,
        contract: &SkiaSurfaceContract,
        canvas: &Canvas,
        path_effects: &[PathEffect],
    ) -> Result<(), LibraryError> {
        self.visit_paths(
            canvas,
            path_effect_outset(path_effects) + 1.0,
            |canvas, path| draw_path_silhouette(contract, canvas, path, path_effects),
        )
    }

    fn visit_paths(
        &self,
        canvas: &Canvas,
        outset: f32,
        mut draw: impl FnMut(&Canvas, &Path) -> Result<(), LibraryError>,
    ) -> Result<(), LibraryError> {
        if self.parts.is_empty() {
            return draw(canvas, &self.aggregate);
        }
        for part in &self.parts {
            if part.opacity <= 0.0 || part.path.is_empty() {
                continue;
            }
            if part.opacity >= 1.0 {
                draw(canvas, &part.path)?;
                continue;
            }
            let bounds = part
                .path
                .compute_tight_bounds()
                .with_outset((outset, outset));
            canvas.save_layer_alpha_f(Some(bounds), part.opacity);
            draw(canvas, &part.path)?;
            canvas.restore();
        }
        Ok(())
    }
}

fn draw_path_body(
    contract: &SkiaSurfaceContract,
    canvas: &Canvas,
    path: &Path,
    path_effects: &[PathEffect],
    styles: &[StyleConfig],
) -> Result<(), LibraryError> {
    for config in styles {
        match &config.style {
            DrawStyle::Fill { color, offset } => PaintFactory::new(contract).draw_shape_fill(
                canvas,
                path,
                color,
                path_effects,
                *offset,
            )?,
            DrawStyle::Stroke {
                color,
                width,
                offset,
                cap,
                join,
                miter,
                dash_array,
                dash_offset,
            } => PaintFactory::new(contract).draw_shape_stroke(
                canvas,
                path,
                path_effects,
                StrokeRenderConfig {
                    color,
                    width: *width,
                    offset: *offset,
                    cap,
                    join,
                    miter: *miter,
                    dash_array,
                    dash_offset: *dash_offset,
                },
            )?,
            DrawStyle::ColorOverlay { .. }
            | DrawStyle::GradientOverlay { .. }
            | DrawStyle::PatternOverlay { .. }
            | DrawStyle::DropShadow { .. }
            | DrawStyle::InnerShadow { .. }
            | DrawStyle::OuterGlow { .. }
            | DrawStyle::InnerGlow { .. }
            | DrawStyle::Satin { .. }
            | DrawStyle::BevelEmboss { .. } => {}
        }
    }
    Ok(())
}

fn draw_path_silhouette(
    contract: &SkiaSurfaceContract,
    canvas: &Canvas,
    path: &Path,
    path_effects: &[PathEffect],
) -> Result<(), LibraryError> {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    skia_working_surface::set_paint_authored_color(&mut paint, contract, &Color::white(), 1.0)?;
    super::paint::apply_path_effects(path_effects, path, &mut paint)?;
    canvas.draw_path(path, &paint);
    Ok(())
}
