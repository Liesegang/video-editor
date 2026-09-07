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
};
use crate::rendering::blend::with_restored_canvas;
use crate::rendering::skia_working_surface::SkiaSurfaceContract;
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

pub(super) struct TextBody {
    pub(super) layout: ShapedTextLayout,
    pub(super) transforms: Vec<TransformData>,
    batches: Vec<GlyphBatch>,
    geometry: Vec<crate::rendering::text_layout::ShapedGlyphGeometry>,
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
        let geometry = layout.glyph_geometry()?;
        for run in &layout.runs {
            let elements = layout.run_element_indices(run)?;
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
        let geometry = crate::rendering::text_layout::transformed_glyph_bounds(
            &self.layout.metadata,
            &self.transforms,
            &self.geometry,
            0.0,
        )?;
        let content = crate::rendering::text_layout::transformed_glyph_bounds(
            &self.layout.metadata,
            &self.transforms,
            &self.geometry,
            body_outset,
        )?;
        Some((geometry, content))
    }

    pub(super) fn draw_style(
        &self,
        contract: &SkiaSurfaceContract,
        canvas: &Canvas,
        geometry: skia_safe::Rect,
        config: &StyleConfig,
    ) -> Result<(), LibraryError> {
        if config.style.composite_phase() != super::layer_styles::CompositePhase::Body {
            return Err(LibraryError::Render(
                "Text Shape rasterization received an Image layer style".to_string(),
            ));
        }
        self.paint_batches(canvas, |material| {
            // Glyphs are drawn under their individual Ensemble affine, while
            // authored Paint geometry belongs to the complete Text object.
            // Cancel that batch CTM in the shader so Gradient and Pattern do
            // not restart or rotate independently for each shaped element.
            let shader_local_matrix = material
                .affine
                .inverse()
                .map(|inverse| build_transform_matrix(&inverse));
            PaintFactory::new(contract).text_paint(
                &config.style,
                material.opacity,
                material.color.as_ref(),
                geometry,
                shader_local_matrix.as_ref(),
            )
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
