//! Tight local bounds for normalized vector-layer styles.

use skia_safe::Rect;

use crate::core::ensemble::types::DecoratorConfig;
use crate::error::LibraryError;
use crate::model::frame::appearance::{AppearanceOutsets, appearance_outsets, path_effect_outset};
use crate::model::frame::draw_type::PathEffect;
use crate::model::frame::entity::StyleConfig;
use crate::model::frame::runtime_shape::{
    RuntimeBounds, measure_path_decorator_bounds, measure_text_decorator_bounds,
};

const ANTIALIAS_OUTSET: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct VectorLayerBounds {
    /// Exact source ink/geometry. Normalized Gradient Overlay coordinates use
    /// this rectangle and are independent from the render target dimensions.
    pub(super) geometry: Rect,
    /// Fill/Stroke and antialiasing support used to record the composed body.
    pub(super) content: Rect,
    /// Complete body plus underlay/overlay decoration support.
    pub(super) visual: Rect,
}

impl VectorLayerBounds {
    pub(super) fn text_body(
        body: &super::vector_text_body::TextBody,
        styles: &[StyleConfig],
        decorators: &[DecoratorConfig],
    ) -> Result<Self, LibraryError> {
        let outsets = appearance_outsets(styles);
        let decorators =
            measure_text_decorator_bounds(&body.layout.metadata, &body.transforms, decorators)?;
        let Some((geometry, content)) = body.local_bounds(outsets.body) else {
            return Ok(Self::empty().with_decorators(decorators));
        };
        // Stroke follows element transforms; antialiasing support is reserved
        // after their union so a strongly scaled-down glyph cannot be clipped.
        let content = expand(runtime_rect(content), ANTIALIAS_OUTSET);
        Ok(Self {
            geometry: runtime_rect(geometry),
            content,
            visual: expand(content, (outsets.visual - outsets.body).max(0.0)),
        }
        .with_decorators(decorators))
    }

    pub(super) fn path(
        geometry: Rect,
        styles: &[StyleConfig],
        path_effects: &[PathEffect],
        decorators: &[DecoratorConfig],
    ) -> Result<Self, LibraryError> {
        let bounds = Self::from_untransformed_geometry(
            geometry,
            appearance_outsets(styles),
            path_effect_outset(path_effects),
        );
        let geometry =
            RuntimeBounds::new(geometry.left, geometry.top, geometry.right, geometry.bottom);
        Ok(bounds.with_decorators(measure_path_decorator_bounds(geometry, decorators)?))
    }

    fn from_untransformed_geometry(
        geometry: Rect,
        outsets: AppearanceOutsets,
        geometry_effect_outset: f32,
    ) -> Self {
        let content = expand(
            geometry,
            geometry_effect_outset + outsets.body + ANTIALIAS_OUTSET,
        );
        Self {
            geometry,
            content,
            visual: expand(content, (outsets.visual - outsets.body).max(0.0)),
        }
    }

    fn empty() -> Self {
        Self {
            geometry: Rect::new_empty(),
            content: Rect::new_empty(),
            visual: Rect::new_empty(),
        }
    }

    fn with_decorators(mut self, decorators: Option<RuntimeBounds>) -> Self {
        if let Some(decorators) = decorators {
            let decorators = expand(runtime_rect(decorators), ANTIALIAS_OUTSET);
            self.visual = union_rect(self.visual, decorators);
        }
        self
    }
}

fn expand(bounds: Rect, amount: f32) -> Rect {
    bounds.with_outset((amount, amount))
}

fn runtime_rect(bounds: RuntimeBounds) -> Rect {
    Rect::new(bounds.left, bounds.top, bounds.right, bounds.bottom)
}

fn union_rect(current: Rect, next: Rect) -> Rect {
    if current.is_empty() {
        return next;
    }
    if next.is_empty() {
        return current;
    }
    Rect::new(
        current.left.min(next.left),
        current.top.min(next.top),
        current.right.max(next.right),
        current.bottom.max(next.bottom),
    )
}

#[cfg(test)]
mod tests {
    use super::VectorLayerBounds;
    use crate::model::BlendMode;
    use crate::model::frame::color::Color;
    use crate::model::frame::draw_type::DrawStyle;
    use crate::model::frame::entity::StyleConfig;
    use skia_safe::Rect;
    use uuid::Uuid;

    #[test]
    fn text_ink_bounds_exclude_surrounding_whitespace() {
        let body =
            super::super::vector_text_body::TextBody::resolve("  M  ", "Arial", 36.0, None, 0.0)
                .expect("shape Text");
        let logical = crate::rendering::text_layout::measure_text_layout("  M  ", "Arial", 36.0);
        let bounds = VectorLayerBounds::text_body(&body, &[], &[]).expect("Text bounds");
        assert!(!bounds.geometry.is_empty());
        assert!(bounds.geometry.left > 0.0);
        assert!(bounds.geometry.right < logical.width);
        assert!(bounds.geometry.height() < logical.height);
    }

    #[test]
    fn small_path_bounds_stay_tight_and_finite() {
        let bounds = VectorLayerBounds::path(Rect::new(11.0, 17.0, 29.0, 41.0), &[], &[], &[])
            .expect("path bounds");

        assert_eq!(bounds.geometry, Rect::new(11.0, 17.0, 29.0, 41.0));
        assert!(bounds.content.left.is_finite());
        assert!(bounds.content.top.is_finite());
        assert!(bounds.content.right.is_finite());
        assert!(bounds.content.bottom.is_finite());
        assert!(bounds.content.width() < 100.0);
        assert!(bounds.content.height() < 100.0);
        assert!(bounds.visual.width() < 100.0);
        assert!(bounds.visual.height() < 100.0);
    }

    #[test]
    fn path_bounds_keep_geometry_separate_from_body_and_shadow_support() {
        let geometry = Rect::new(11.0, 17.0, 29.0, 41.0);
        let styles = [
            StyleConfig {
                id: Uuid::new_v4(),
                style: DrawStyle::Fill {
                    color: Color::white(),
                    offset: 4.0,
                },
            },
            StyleConfig {
                id: Uuid::new_v4(),
                style: DrawStyle::DropShadow {
                    color: Color::black(),
                    opacity: 1.0,
                    blend_mode: BlendMode::Normal,
                    angle: 0.0,
                    distance: 9.0,
                    spread: 0.0,
                    size: 6.0,
                },
            },
        ];

        let bounds = VectorLayerBounds::path(geometry, &styles, &[], &[]).expect("path bounds");

        assert_eq!(bounds.geometry, geometry);
        assert_eq!(bounds.content, Rect::new(6.0, 12.0, 34.0, 46.0));
        assert_eq!(bounds.visual, Rect::new(-9.0, -3.0, 49.0, 61.0));
    }
}
