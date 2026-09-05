//! One shaped glyph painter for ordinary and animated Text.
//!
//! SkParagraph owns shaping, fallback, visual order, and baseline placement.
//! Ensemble changes the transform/material of those glyphs, never reshapes
//! individual strings or sends neutral text through a second painter.

use skia_safe::{Canvas, Paint, Point, TextBlob, TextBlobBuilder};

use super::paint::PaintFactory;
use super::{Affine2D, build_transform_matrix};
use crate::core::ensemble::types::{EnsembleData, TransformData};
use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::model::frame::entity::StyleConfig;
use crate::model::frame::runtime_shape::{
    RuntimeBounds, evaluate_text_element_transforms, text_element_affine, text_element_center,
    transform_bounds,
};
use crate::rendering::blend::with_restored_canvas;
use crate::rendering::skia_working_surface::{self, SkiaSurfaceContract};
use crate::rendering::text_layout::ShapedTextLayout;

#[derive(Clone, PartialEq)]
struct ElementPaint {
    affine: Affine2D,
    opacity: f32,
    color: Option<Color>,
}

struct GlyphBatch {
    blob: TextBlob,
    origin: Point,
    paint: ElementPaint,
}

struct GlyphGeometry {
    bounds: RuntimeBounds,
    element: usize,
}

pub(super) struct TextBody {
    pub(super) layout: ShapedTextLayout,
    pub(super) transforms: Vec<TransformData>,
    batches: Vec<GlyphBatch>,
    geometry: Vec<GlyphGeometry>,
}

impl TextBody {
    pub(super) fn resolve(
        text: &str,
        font_name: &str,
        size: f32,
        ensemble: Option<&EnsembleData>,
        current_time: f32,
    ) -> Result<Self, LibraryError> {
        let layout = ShapedTextLayout::new(text, font_name, size);
        let transforms = match ensemble.filter(|ensemble| ensemble.enabled) {
            Some(ensemble) => {
                evaluate_text_element_transforms(&layout.metadata, ensemble, current_time)?
            }
            None => vec![TransformData::identity(); layout.metadata.elements.len()],
        };
        let paints = layout
            .metadata
            .elements
            .iter()
            .zip(&transforms)
            .map(|(element, transform)| ElementPaint {
                affine: text_element_affine(text_element_center(element), transform),
                opacity: transform.opacity,
                color: transform.color_override.clone(),
            })
            .collect::<Vec<_>>();
        let mut batches = Vec::new();
        let mut geometry = Vec::new();
        for run in &layout.runs {
            let elements = run
                .source_starts
                .iter()
                .take(run.glyphs.len())
                .map(|start| {
                    let start = *start as usize;
                    let index = layout
                        .metadata
                        .elements
                        .partition_point(|element| element.utf8_range.end <= start);
                    layout
                        .metadata
                        .elements
                        .get(index)
                        .filter(|element| element.utf8_range.contains(&start))
                        .map(|_| index)
                        .ok_or_else(|| {
                            LibraryError::Render(format!(
                                "Shaped glyph at UTF-8 byte {start} has no Text element"
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            for ((bounds, position), element) in
                run.bounds.iter().zip(&run.positions).zip(&elements)
            {
                if bounds.is_empty() {
                    continue;
                }
                let x = run.origin.x + position.x;
                let y = run.origin.y + position.y;
                geometry.push(GlyphGeometry {
                    bounds: RuntimeBounds::new(
                        bounds.left + x,
                        bounds.top + y,
                        bounds.right + x,
                        bounds.bottom + y,
                    ),
                    element: *element,
                });
            }
            let mut start = 0;
            while start < run.glyphs.len() {
                let paint = &paints[elements[start]];
                let mut end = start + 1;
                while end < run.glyphs.len() && paints[elements[end]] == *paint {
                    end += 1;
                }
                let mut builder = TextBlobBuilder::new();
                let (glyphs, positions) = builder.alloc_run_pos(&run.font, end - start, None);
                glyphs.copy_from_slice(&run.glyphs[start..end]);
                positions.copy_from_slice(&run.positions[start..end]);
                let blob = builder.make().ok_or_else(|| {
                    LibraryError::Render("Failed to retain shaped Text glyph batch".to_string())
                })?;
                batches.push(GlyphBatch {
                    blob,
                    origin: run.origin,
                    paint: paint.clone(),
                });
                start = end;
            }
        }
        Ok(Self {
            layout,
            transforms,
            batches,
            geometry,
        })
    }

    pub(super) fn local_bounds(&self, body_outset: f32) -> Option<(RuntimeBounds, RuntimeBounds)> {
        self.geometry
            .iter()
            .filter(|glyph| self.transforms[glyph.element].opacity > 0.0)
            .map(|glyph| {
                let element = &self.layout.metadata.elements[glyph.element];
                let transform = &self.transforms[glyph.element];
                let center = text_element_center(element);
                (
                    transform_bounds(glyph.bounds, center, transform),
                    transform_bounds(glyph.bounds.expand(body_outset), center, transform),
                )
            })
            .reduce(|(geometry, content), (next_geometry, next_content)| {
                (geometry.union(next_geometry), content.union(next_content))
            })
    }

    pub(super) fn draw_body(
        &self,
        contract: &SkiaSurfaceContract,
        canvas: &Canvas,
        styles: &[StyleConfig],
    ) -> Result<(), LibraryError> {
        for config in styles {
            if config.style.composite_phase() != super::layer_styles::CompositePhase::Body {
                continue;
            }
            self.paint_batches(canvas, |material| {
                PaintFactory::new(contract).text_paint(
                    &config.style,
                    material.opacity,
                    material.color.as_ref(),
                )
            })?;
        }
        Ok(())
    }

    pub(super) fn draw_silhouette(
        &self,
        contract: &SkiaSurfaceContract,
        canvas: &Canvas,
    ) -> Result<(), LibraryError> {
        self.paint_batches(canvas, |material| {
            let mut paint = Paint::default();
            skia_working_surface::set_paint_authored_color(
                &mut paint,
                contract,
                &Color::white(),
                material.opacity,
            )?;
            paint.set_anti_alias(true);
            Ok(paint)
        })
    }

    fn paint_batches(
        &self,
        canvas: &Canvas,
        paint: impl Fn(&ElementPaint) -> Result<Paint, LibraryError>,
    ) -> Result<(), LibraryError> {
        for batch in &self.batches {
            if batch.paint.opacity <= 0.0 {
                continue;
            }
            let paint = paint(&batch.paint)?;
            with_restored_canvas(canvas, |canvas| -> Result<(), LibraryError> {
                canvas.concat(&build_transform_matrix(&batch.paint.affine));
                canvas.draw_text_blob(&batch.blob, batch.origin, &paint);
                Ok(())
            })?;
        }
        Ok(())
    }
}
