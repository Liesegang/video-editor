//! Tight local bounds for normalized vector-layer styles.

use skia_safe::{Font, Rect};

use crate::core::ensemble::TransformData;
use crate::core::ensemble::types::DecoratorConfig;
use crate::error::LibraryError;
use crate::model::frame::appearance::{AppearanceOutsets, appearance_outsets, path_effect_outset};
use crate::model::frame::draw_type::PathEffect;
use crate::model::frame::entity::StyleConfig;
use crate::model::frame::runtime_shape::{
    RuntimeBounds, RuntimeTextShape, measure_path_decorator_bounds, measure_text_decorator_bounds,
    text_element_center, transform_bounds,
};
use crate::rendering::text_layout::measure_text_ink_bounds;

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
    pub(super) fn text(text: &str, font_name: &str, size: f32, styles: &[StyleConfig]) -> Self {
        let geometry = measure_text_ink_bounds(text, font_name, size);
        if geometry.is_empty() {
            return Self::empty();
        }
        Self::from_untransformed_geometry(geometry, appearance_outsets(styles), 0.0)
    }

    pub(super) fn ensemble(
        text: &RuntimeTextShape,
        transforms: &[TransformData],
        font: &Font,
        styles: &[StyleConfig],
        decorators: &[DecoratorConfig],
    ) -> Result<Self, LibraryError> {
        let outsets = appearance_outsets(styles);
        let mut geometry = None;
        let mut content = None;
        for (element, transform) in text.elements.iter().zip(transforms) {
            if transform.opacity <= 0.0 {
                continue;
            }
            let (_, ink) = font.measure_str(&element.source, None);
            if ink.is_empty() {
                continue;
            }
            let ink = RuntimeBounds::new(
                ink.left + element.bounds.left,
                ink.top + element.baseline,
                ink.right + element.bounds.left,
                ink.bottom + element.baseline,
            );
            let center = text_element_center(element);
            let transformed_ink = transform_bounds(ink, center, transform);
            geometry = Some(union_runtime(geometry, transformed_ink));
            let transformed_body = transform_bounds(ink.expand(outsets.body), center, transform);
            content = Some(union_runtime(content, transformed_body));
        }

        let Some(geometry) = geometry else {
            return Ok(Self::empty()
                .with_decorators(measure_text_decorator_bounds(text, transforms, decorators)?));
        };
        // Stroke support follows the element transform, but antialiasing is a
        // device-pixel concern. Reserve it after the transformed union so a
        // strongly scaled-down glyph still cannot clip its edge coverage.
        let content = expand(runtime_rect(content.unwrap_or(geometry)), ANTIALIAS_OUTSET);
        let decoration = (outsets.visual - outsets.body).max(0.0);
        Ok(Self {
            geometry: runtime_rect(geometry),
            content,
            visual: expand(content, decoration),
        }
        .with_decorators(measure_text_decorator_bounds(text, transforms, decorators)?))
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

fn union_runtime(current: Option<RuntimeBounds>, next: RuntimeBounds) -> RuntimeBounds {
    current.map_or(next, |current| current.union(next))
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
