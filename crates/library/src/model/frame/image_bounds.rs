//! Evaluated image domains shared by rendering and Preview interaction.
//!
//! `geometry` is the undecorated source domain used by normalized paints,
//! `content` is the complete alpha entering the current unary Image stage,
//! and `visual` is all support that the stage can produce.

use crate::model::frame::appearance::SOURCE_RASTER_OUTSET;
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::entity::{
    FrameBounds, FrameContent, FrameGroupKind, FrameItem, FrameObject, StyleConfig,
};
use crate::rendering::renderer::Affine2D;
use std::collections::HashMap;

/// Device-pixel safety margin used when a logical visual bound becomes a
/// transient raster allocation. This was formerly private to vector surfaces.
pub const FRAME_IMAGE_RASTER_GUARD: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameImageBounds {
    pub geometry: FrameBounds,
    pub content: FrameBounds,
    pub visual: FrameBounds,
}

impl FrameImageBounds {
    pub const fn from_rect(bounds: FrameBounds) -> Self {
        Self {
            geometry: bounds,
            content: bounds,
            visual: bounds,
        }
    }

    pub fn transformed(self, transform: Affine2D) -> Option<Self> {
        Some(Self {
            geometry: transform_bounds(self.geometry, transform)?,
            content: transform_bounds(self.content, transform)?,
            visual: transform_bounds(self.visual, transform)?,
        })
    }

    pub fn translated(self, x: f64, y: f64) -> Option<Self> {
        self.transformed(Affine2D::translate(x, y))
    }

    fn union(self, other: Self) -> Option<Self> {
        Some(Self {
            geometry: union(self.geometry, other.geometry)?,
            content: union(self.content, other.content)?,
            visual: union(self.visual, other.visual)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FrameImageBoundsCacheKey {
    item_address: usize,
    time_bits: u64,
}

/// Per-frame memo for recursively evaluated image bounds.
///
/// Keys use only a borrowed item's transient address and the exact evaluation
/// time. Callers must clear the cache before the borrowed frame tree can be
/// replaced or mutated; no pointer is dereferenced from the cache.
#[derive(Debug, Default)]
pub struct FrameImageBoundsCache {
    items: HashMap<FrameImageBoundsCacheKey, Option<FrameImageBounds>>,
}

impl FrameImageBoundsCache {
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Resolve the union of evaluated image items in their caller's local
    /// space, sharing recursive results with other queries in this frame.
    pub fn items_bounds(
        &mut self,
        items: &[FrameItem],
        current_time: f64,
    ) -> Option<FrameImageBounds> {
        items
            .iter()
            .filter_map(|item| self.item_bounds(item, current_time))
            .try_fold(None, |current: Option<FrameImageBounds>, next| {
                Some(Some(match current {
                    Some(current) => current.union(next)?,
                    None => next,
                }))
            })
            .flatten()
    }

    /// Resolve one evaluated image item, including nested Group transforms
    /// and unary image-style support.
    pub fn item_bounds(&mut self, item: &FrameItem, current_time: f64) -> Option<FrameImageBounds> {
        let key = FrameImageBoundsCacheKey {
            item_address: item as *const FrameItem as usize,
            time_bits: current_time.to_bits(),
        };
        if let Some(bounds) = self.items.get(&key) {
            return *bounds;
        }
        let bounds = self.compute_item_bounds(item, current_time);
        self.items.insert(key, bounds);
        bounds
    }

    fn compute_item_bounds(
        &mut self,
        item: &FrameItem,
        current_time: f64,
    ) -> Option<FrameImageBounds> {
        match item {
            FrameItem::Object(object) => object_image_bounds(object, current_time),
            FrameItem::Group(group) => {
                let child_time = group.effect_time.into_inner();
                let mut bounds = self.items_bounds(&group.items, child_time);
                if group.kind == FrameGroupKind::Composition
                    && group.background_color.a != 0
                    && let Some(composition) =
                        frame_bounds(0.0, 0.0, group.width as f64, group.height as f64)
                {
                    let composition = FrameImageBounds::from_rect(composition);
                    bounds = Some(match bounds {
                        Some(bounds) => bounds.union(composition)?,
                        None => composition,
                    });
                }
                let mut bounds = bounds?;
                bounds = image_style_bounds(bounds, &group.effects, 1.0)?;
                bounds.transformed(Affine2D::from(&group.transform))
            }
            FrameItem::Transition(transition) => {
                let from = self.item_bounds(
                    &transition.from.item,
                    transition.from.source_time.into_inner(),
                );
                let to =
                    self.item_bounds(&transition.to.item, transition.to.source_time.into_inner());
                match (from, to) {
                    (Some(from), Some(to)) => from.union(to),
                    (Some(bounds), None) | (None, Some(bounds)) => Some(bounds),
                    (None, None) => None,
                }
            }
        }
    }
}

/// Resolve the union of evaluated image items in their caller's local space.
pub fn frame_image_bounds(items: &[FrameItem], current_time: f64) -> Option<FrameImageBounds> {
    FrameImageBoundsCache::default().items_bounds(items, current_time)
}

/// Resolve one evaluated image item, including nested Group transforms and
/// unary image-style support.
pub fn frame_item_image_bounds(item: &FrameItem, current_time: f64) -> Option<FrameImageBounds> {
    FrameImageBoundsCache::default().item_bounds(item, current_time)
}

/// Apply unary Image effects without losing the stable source geometry used
/// by normalized Gradient/Pattern coordinates.
pub fn image_style_bounds(
    mut input: FrameImageBounds,
    effects: &[ImageEffect],
    scale: f64,
) -> Option<FrameImageBounds> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    for effect in effects {
        match effect {
            ImageEffect::LayerStyle(style) => {
                let upstream = input.visual;
                input.content = upstream;
                input.visual = expand(upstream, f64::from(style.style.visual_outset()) * scale)?;
            }
            ImageEffect::Plugin { .. } => {
                // Plugin effects have no declared spatial-support contract.
                // Preserve known input support rather than inventing an
                // operation-specific radius here.
                input.content = input.visual;
            }
        }
    }
    Some(input)
}

fn object_image_bounds(object: &FrameObject, current_time: f64) -> Option<FrameImageBounds> {
    let declared = object
        .content_bounds
        .or_else(|| fallback_content_bounds(&object.content));
    let measured_geometry = object_geometry_bounds(&object.content, current_time);
    let geometry = if matches!(
        &object.content,
        FrameContent::Text { .. } | FrameContent::Shape { .. }
    ) {
        measured_geometry?
    } else {
        measured_geometry.or(declared)?
    };
    let (body_outset, visual_outset) = object_appearance_outsets(&object.content);
    let raster_outset = if matches!(
        &object.content,
        FrameContent::Text { .. } | FrameContent::Shape { .. }
    ) {
        f64::from(SOURCE_RASTER_OUTSET)
    } else {
        0.0
    };
    let measured_content = expand(geometry, f64::from(body_outset) + raster_outset)?;
    let measured_visual = expand(geometry, f64::from(visual_outset) + raster_outset)?;
    let content = match declared {
        Some(declared) => union(declared, measured_content)?,
        None => measured_content,
    };
    let visual = match declared {
        Some(declared) => union(declared, measured_visual)?,
        None => measured_visual,
    };
    let effects = content_effects(&object.content);
    image_style_bounds(
        FrameImageBounds {
            geometry,
            content,
            visual,
        },
        effects,
        1.0,
    )?
    .transformed(Affine2D::from(object.content.transform()))
}

fn object_geometry_bounds(content: &FrameContent, current_time: f64) -> Option<FrameBounds> {
    match content {
        FrameContent::Text {
            text,
            font,
            size,
            ensemble,
            ..
        } => crate::core::rendering::text_layout::measure_text_geometry_bounds(
            text,
            font,
            *size as f32,
            ensemble.as_ref(),
            current_time as f32,
        )
        .ok()
        .flatten()
        .and_then(runtime_bounds),
        FrameContent::Shape {
            path,
            canonical_path,
            parts,
            ..
        } => {
            let paths = if parts.is_empty() {
                vec![(canonical_path.as_ref(), path.as_str())]
            } else {
                parts
                    .iter()
                    .map(|part| (part.canonical_path.as_ref(), part.path.as_str()))
                    .collect()
            };
            paths
                .into_iter()
                .filter_map(|(canonical, path)| {
                    let path = crate::core::rendering::path_geometry::resolve_renderer_path(
                        canonical, path,
                    )
                    .ok()?;
                    (!path.is_empty()).then(|| path.compute_tight_bounds())
                })
                .filter_map(|bounds| {
                    bounds_from_edges(
                        f64::from(bounds.left),
                        f64::from(bounds.top),
                        f64::from(bounds.right),
                        f64::from(bounds.bottom),
                    )
                })
                .try_fold(None, |current: Option<FrameBounds>, next| {
                    Some(Some(match current {
                        Some(current) => union(current, next)?,
                        None => next,
                    }))
                })
                .flatten()
        }
        FrameContent::SkSL { resolution, .. } => {
            frame_bounds(0.0, 0.0, f64::from(resolution.0), f64::from(resolution.1))
        }
        FrameContent::PointScene { scene, .. } => frame_bounds(
            0.0,
            0.0,
            f64::from(scene.logical_width),
            f64::from(scene.logical_height),
        ),
        FrameContent::Video { .. } | FrameContent::Image { .. } => None,
    }
}

fn fallback_content_bounds(content: &FrameContent) -> Option<FrameBounds> {
    match content {
        FrameContent::SkSL { resolution, .. } => {
            frame_bounds(0.0, 0.0, f64::from(resolution.0), f64::from(resolution.1))
        }
        FrameContent::PointScene { scene, .. } => frame_bounds(
            0.0,
            0.0,
            f64::from(scene.logical_width),
            f64::from(scene.logical_height),
        ),
        FrameContent::Video { .. }
        | FrameContent::Image { .. }
        | FrameContent::Text { .. }
        | FrameContent::Shape { .. } => None,
    }
}

fn content_effects(content: &FrameContent) -> &[ImageEffect] {
    match content {
        FrameContent::Video { surface, .. } | FrameContent::Image { surface } => &surface.effects,
        FrameContent::Text { effects, .. }
        | FrameContent::Shape { effects, .. }
        | FrameContent::SkSL { effects, .. }
        | FrameContent::PointScene { effects, .. } => effects,
    }
}

fn object_appearance_outsets(content: &FrameContent) -> (f32, f32) {
    let (styles, geometry_effect_outset): (&[StyleConfig], f32) = match content {
        FrameContent::Text { styles, .. } => (styles.as_slice(), 0.0),
        FrameContent::Shape {
            styles,
            path_effects,
            ..
        } => (
            styles.as_slice(),
            crate::model::frame::appearance::path_effect_outset(path_effects),
        ),
        FrameContent::Video { .. }
        | FrameContent::Image { .. }
        | FrameContent::SkSL { .. }
        | FrameContent::PointScene { .. } => (&[], 0.0),
    };
    let outsets = crate::model::frame::appearance::appearance_outsets(styles);
    (
        outsets.body + geometry_effect_outset,
        outsets.visual + geometry_effect_outset,
    )
}

fn runtime_bounds(
    bounds: crate::model::frame::runtime_shape::RuntimeBounds,
) -> Option<FrameBounds> {
    bounds_from_edges(
        f64::from(bounds.left),
        f64::from(bounds.top),
        f64::from(bounds.right),
        f64::from(bounds.bottom),
    )
}

fn frame_bounds(x: f64, y: f64, width: f64, height: f64) -> Option<FrameBounds> {
    bounds_from_edges(x, y, x + width, y + height)
}

fn bounds_from_edges(left: f64, top: f64, right: f64, bottom: f64) -> Option<FrameBounds> {
    [left, top, right, bottom]
        .into_iter()
        .all(f64::is_finite)
        .then_some(())?;
    (right > left && bottom > top && [left, top, right, bottom].into_iter().all(fits_f32)).then(
        || {
            FrameBounds::new(
                left as f32,
                top as f32,
                (right - left) as f32,
                (bottom - top) as f32,
            )
        },
    )
}

fn transform_bounds(bounds: FrameBounds, transform: Affine2D) -> Option<FrameBounds> {
    let (x, y, width, height) = bounds.as_tuple();
    let points = [
        transform.map_point(f64::from(x), f64::from(y)),
        transform.map_point(f64::from(x + width), f64::from(y)),
        transform.map_point(f64::from(x + width), f64::from(y + height)),
        transform.map_point(f64::from(x), f64::from(y + height)),
    ];
    let left = points
        .iter()
        .map(|point| point.0)
        .fold(f64::INFINITY, f64::min);
    let top = points
        .iter()
        .map(|point| point.1)
        .fold(f64::INFINITY, f64::min);
    let right = points
        .iter()
        .map(|point| point.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = points
        .iter()
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    bounds_from_edges(left, top, right, bottom)
}

fn union(left: FrameBounds, right: FrameBounds) -> Option<FrameBounds> {
    let (lx, ly, lw, lh) = left.as_tuple();
    let (rx, ry, rw, rh) = right.as_tuple();
    bounds_from_edges(
        f64::from(lx.min(rx)),
        f64::from(ly.min(ry)),
        f64::from((lx + lw).max(rx + rw)),
        f64::from((ly + lh).max(ry + rh)),
    )
}

fn expand(bounds: FrameBounds, amount: f64) -> Option<FrameBounds> {
    if !amount.is_finite() || amount < 0.0 {
        return None;
    }
    let (x, y, width, height) = bounds.as_tuple();
    bounds_from_edges(
        f64::from(x) - amount,
        f64::from(y) - amount,
        f64::from(x + width) + amount,
        f64::from(y + height) + amount,
    )
}

fn fits_f32(value: f64) -> bool {
    value.abs() <= f64::from(f32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BlendMode;
    use crate::model::frame::color::Color;
    use crate::model::frame::draw_type::DrawStyle;
    use crate::model::frame::entity::{FrameGroup, StyleConfig};
    use crate::model::frame::transform::{Position, Transform};
    use ordered_float::OrderedFloat;
    use uuid::Uuid;

    fn image_bounds(x: f32, y: f32, width: f32, height: f32) -> FrameImageBounds {
        let bounds = FrameBounds::new(x, y, width, height);
        FrameImageBounds {
            geometry: bounds,
            content: bounds,
            visual: bounds,
        }
    }

    #[test]
    fn image_style_keeps_geometry_and_expands_complete_upstream_support() {
        let shadow = ImageEffect::LayerStyle(StyleConfig {
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
        });
        let first = image_style_bounds(
            image_bounds(10.0, 20.0, 30.0, 40.0),
            std::slice::from_ref(&shadow),
            1.0,
        )
        .expect("shadow bounds");
        assert_eq!(first.geometry, FrameBounds::new(10.0, 20.0, 30.0, 40.0));
        assert_eq!(first.content, FrameBounds::new(10.0, 20.0, 30.0, 40.0));
        assert_eq!(first.visual, FrameBounds::new(-5.0, 5.0, 60.0, 70.0));

        let second = image_style_bounds(first, std::slice::from_ref(&shadow), 1.0)
            .expect("chained shadow bounds");
        assert_eq!(second.geometry, FrameBounds::new(10.0, 20.0, 30.0, 40.0));
        assert_eq!(second.content, first.visual);
        assert_eq!(second.visual, FrameBounds::new(-20.0, -10.0, 90.0, 100.0));

        let scaled = image_style_bounds(image_bounds(10.0, 20.0, 30.0, 40.0), &[shadow], 2.0)
            .expect("scaled shadow bounds");
        assert_eq!(scaled.visual, FrameBounds::new(-20.0, -10.0, 90.0, 100.0));
        assert!(image_style_bounds(scaled, &[], 0.0).is_none());
    }

    #[test]
    fn recursive_groups_union_domains_and_apply_group_transform() {
        let object = |id, x| {
            FrameItem::Object(FrameObject {
                source_node_id: id,
                spatial_transform_node_id: None,
                spatial_transform: Box::default(),
                content_bounds: Some(FrameBounds::new(0.0, 0.0, 10.0, 8.0)),
                content: FrameContent::SkSL {
                    shader: String::new(),
                    resolution: (10.0, 8.0),
                    color_domain:
                        crate::model::frame::entity::SkSLColorDomain::ProjectWorkingLinear,
                    effects: Vec::new(),
                    transform: Transform {
                        position: Position { x, y: 2.0 },
                        ..Transform::default()
                    },
                },
            })
        };
        let group = FrameItem::Group(FrameGroup {
            source_id: Uuid::new_v4(),
            kind: FrameGroupKind::Merge,
            width: 100,
            height: 100,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            transform: Transform {
                position: Position { x: 5.0, y: -3.0 },
                ..Transform::default()
            },
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(0.0),
            effects: Vec::new(),
            items: vec![object(Uuid::new_v4(), 0.0), object(Uuid::new_v4(), 20.0)],
        });
        let bounds = frame_item_image_bounds(&group, 0.0).expect("group bounds");
        assert_eq!(bounds.visual, FrameBounds::new(5.0, -1.0, 30.0, 8.0));
    }

    #[test]
    fn transparent_composition_size_does_not_replace_tight_image_geometry() {
        let child = FrameItem::Object(FrameObject {
            source_node_id: Uuid::new_v4(),
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: Some(FrameBounds::new(0.0, 0.0, 40.0, 50.0)),
            content: FrameContent::SkSL {
                shader: String::new(),
                resolution: (40.0, 50.0),
                color_domain: crate::model::frame::entity::SkSLColorDomain::ProjectWorkingLinear,
                effects: Vec::new(),
                transform: Transform {
                    position: Position { x: 20.0, y: 30.0 },
                    ..Transform::default()
                },
            },
        });
        let group = FrameItem::Group(FrameGroup {
            source_id: Uuid::new_v4(),
            kind: FrameGroupKind::Composition,
            width: 4_000,
            height: 2_000,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            transform: Transform::default(),
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(0.0),
            effects: Vec::new(),
            items: vec![child],
        });
        let bounds = frame_item_image_bounds(&group, 0.0).expect("Composition child bounds");
        assert_eq!(bounds.geometry, FrameBounds::new(20.0, 30.0, 40.0, 50.0));
        assert_eq!(bounds.visual, bounds.geometry);
    }

    #[test]
    fn invisible_text_does_not_resurrect_a_stale_declared_bound() {
        let item = FrameItem::Object(FrameObject {
            source_node_id: Uuid::new_v4(),
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: Some(FrameBounds::new(0.0, 0.0, 200.0, 80.0)),
            content: FrameContent::Text {
                text: String::new(),
                font: "Arial".to_string(),
                size: 48.0,
                styles: vec![StyleConfig {
                    id: Uuid::new_v4(),
                    style: DrawStyle::Fill {
                        color: Color::white(),
                        offset: 0.0,
                    },
                }],
                effects: Vec::new(),
                ensemble: None,
                transform: Transform::default(),
            },
        });
        assert!(frame_item_image_bounds(&item, 0.0).is_none());
    }

    #[test]
    fn tight_and_preexpanded_shape_declarations_share_source_raster_support() {
        let shape = |declared| {
            FrameItem::Object(FrameObject {
                source_node_id: Uuid::new_v4(),
                spatial_transform_node_id: None,
                spatial_transform: Box::default(),
                content_bounds: Some(declared),
                content: FrameContent::Shape {
                    path: "M 0 0 H 20 V 30 H 0 Z".to_string(),
                    canonical_path: None,
                    parts: Vec::new(),
                    styles: vec![StyleConfig {
                        id: Uuid::new_v4(),
                        style: DrawStyle::Fill {
                            color: Color::white(),
                            offset: 0.0,
                        },
                    }],
                    path_effects: Vec::new(),
                    effects: Vec::new(),
                    ensemble: None,
                    transform: Transform::default(),
                },
            })
        };
        let direct = shape(FrameBounds::new(0.0, 0.0, 20.0, 30.0));
        let node = shape(FrameBounds::new(-1.0, -1.0, 22.0, 32.0));
        let direct = frame_item_image_bounds(&direct, 0.0).expect("direct Shape bounds");
        let node = frame_item_image_bounds(&node, 0.0).expect("Node Shape bounds");

        assert_eq!(direct, node);
        assert_eq!(direct.geometry, FrameBounds::new(0.0, 0.0, 20.0, 30.0));
        assert_eq!(direct.visual, FrameBounds::new(-1.0, -1.0, 22.0, 32.0));
    }

    #[test]
    fn per_frame_cache_reuses_results_until_explicitly_cleared() {
        let mut item = FrameItem::Object(FrameObject {
            source_node_id: Uuid::new_v4(),
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: None,
            content: FrameContent::SkSL {
                shader: String::new(),
                resolution: (20.0, 30.0),
                color_domain: crate::model::frame::entity::SkSLColorDomain::ProjectWorkingLinear,
                effects: Vec::new(),
                transform: Transform::default(),
            },
        });
        let mut cache = FrameImageBoundsCache::default();
        assert_eq!(
            cache.item_bounds(&item, 0.0).map(|bounds| bounds.visual),
            Some(FrameBounds::new(0.0, 0.0, 20.0, 30.0))
        );

        let FrameItem::Object(object) = &mut item else {
            panic!("fixture is an object");
        };
        let FrameContent::SkSL { resolution, .. } = &mut object.content else {
            panic!("fixture is SkSL");
        };
        *resolution = (40.0, 50.0);
        assert_eq!(
            cache.item_bounds(&item, 0.0).map(|bounds| bounds.visual),
            Some(FrameBounds::new(0.0, 0.0, 20.0, 30.0))
        );

        cache.clear();
        assert_eq!(
            cache.item_bounds(&item, 0.0).map(|bounds| bounds.visual),
            Some(FrameBounds::new(0.0, 0.0, 40.0, 50.0))
        );
    }
}
