//! Geometry extraction for the selected Timeline Item.
//!
//! The evaluated `FrameInfo` is the same hierarchy consumed by the renderer,
//! so this module never substitutes the enclosing Composition dimensions for
//! an Item's visual bounds.

use std::collections::HashSet;

use library::model::authoring::TimelineItemId;
use library::model::frame::entity::{FrameContent, FrameGroupKind, FrameItem, FrameObject};
use library::model::frame::frame::FrameInfo;
use library::model::frame::transform::Transform;
use library::rendering::renderer::Affine2D;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ItemGizmoGeometry {
    pub outlines: Vec<[egui::Pos2; 4]>,
    pub control_outline: [egui::Pos2; 4],
    pub anchor: egui::Pos2,
    pub local_bounds: egui::Rect,
    pub parent_transform: Affine2D,
    pub item_transform: Transform,
    local_outlines: Vec<[egui::Pos2; 4]>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ProjectedGizmoGeometry {
    pub outlines: Vec<[egui::Pos2; 4]>,
    pub control_outline: [egui::Pos2; 4],
    pub anchor: egui::Pos2,
}

impl ItemGizmoGeometry {
    /// Reprojects the rendered source geometry through an authored transform.
    /// During a pointer gesture this previews Scale/Rotation without mutating
    /// the Project or waiting for a renderer round trip.
    pub fn projected(&self, transform: &Transform) -> Option<ProjectedGizmoGeometry> {
        let world = self.parent_transform.compose(Affine2D::from(transform));
        let outlines = self
            .local_outlines
            .iter()
            .map(|outline| map_outline(world, *outline))
            .collect::<Option<Vec<_>>>()?;
        Some(ProjectedGizmoGeometry {
            outlines,
            control_outline: map_rect(world, self.local_bounds)?,
            anchor: map_point(world, transform.anchor.x as f32, transform.anchor.y as f32)?,
        })
    }
}

pub(super) fn item_gizmo_geometry(
    frame: &FrameInfo,
    item_id: TimelineItemId,
) -> Option<ItemGizmoGeometry> {
    find_item_geometry(&frame.items, item_id, Affine2D::IDENTITY)
}

/// Hit-test the pixels represented by `frame` in their final render order.
///
/// A nested Composition may contain Item IDs from another Timeline. The
/// caller supplies exactly the active Timeline's selectable IDs, so clicking
/// its rendered contents selects the outer Composition Item until the user
/// explicitly opens that nested Timeline.
pub(super) fn hit_test_item(
    frame: &FrameInfo,
    selectable: &HashSet<TimelineItemId>,
    world_position: egui::Pos2,
) -> Option<TimelineItemId> {
    let mut render_order = Vec::new();
    collect_render_order(&frame.items, selectable, &mut render_order);
    render_order.into_iter().rev().find(|item_id| {
        item_gizmo_geometry(frame, *item_id).is_some_and(|geometry| {
            geometry
                .outlines
                .iter()
                .any(|outline| convex_quad_contains(*outline, world_position))
        })
    })
}

fn collect_render_order(
    items: &[FrameItem],
    selectable: &HashSet<TimelineItemId>,
    output: &mut Vec<TimelineItemId>,
) {
    for item in items {
        let FrameItem::Group(group) = item else {
            continue;
        };
        let candidate = TimelineItemId::from_uuid(group.source_id);
        if group.kind == FrameGroupKind::Clip && selectable.contains(&candidate) {
            if !output.contains(&candidate) {
                output.push(candidate);
            }
            // This Clip is the active Timeline's selectable facade. Children
            // belong to its source (possibly a nested Timeline), not to the
            // current selection depth.
            continue;
        }
        collect_render_order(&group.items, selectable, output);
    }
}

fn convex_quad_contains(corners: [egui::Pos2; 4], point: egui::Pos2) -> bool {
    if !point.is_finite() || corners.iter().any(|corner| !corner.is_finite()) {
        return false;
    }
    let mut positive = false;
    let mut negative = false;
    for edge in 0..corners.len() {
        let first = corners[edge];
        let second = corners[(edge + 1) % corners.len()];
        let cross =
            (second.x - first.x) * (point.y - first.y) - (second.y - first.y) * (point.x - first.x);
        positive |= cross > f32::EPSILON;
        negative |= cross < -f32::EPSILON;
        if positive && negative {
            return false;
        }
    }
    true
}

fn find_item_geometry(
    items: &[FrameItem],
    item_id: TimelineItemId,
    parent: Affine2D,
) -> Option<ItemGizmoGeometry> {
    for item in items {
        let FrameItem::Group(group) = item else {
            continue;
        };
        let transform = parent.compose(Affine2D::from(&group.transform));
        if group.kind == FrameGroupKind::Clip && group.source_id == item_id.as_uuid() {
            // Parent wrappers retain the child Item ID. Prefer the deepest
            // matching Clip, which is the one that owns this Item's authored
            // transform; `transform` still contains all parent transforms.
            if let Some(inner) = find_item_geometry(&group.items, item_id, transform) {
                return Some(inner);
            }
            let local_outlines = collect_outlines(&group.items, Affine2D::IDENTITY);
            if local_outlines.is_empty() {
                return None;
            }
            let local_points = local_outlines
                .iter()
                .flat_map(|outline| outline.iter().copied())
                .collect::<Vec<_>>();
            let local_bounds = egui::Rect::from_points(&local_points);
            if !local_bounds.is_positive() {
                return None;
            }
            let outlines = local_outlines
                .iter()
                .map(|outline| map_outline(transform, *outline))
                .collect::<Option<Vec<_>>>()?;
            let control_outline = map_rect(transform, local_bounds)?;
            let anchor = map_point(
                transform,
                group.transform.anchor.x as f32,
                group.transform.anchor.y as f32,
            )?;
            return Some(ItemGizmoGeometry {
                outlines,
                control_outline,
                anchor,
                local_bounds,
                parent_transform: parent,
                item_transform: group.transform.clone(),
                local_outlines,
            });
        }
        if let Some(found) = find_item_geometry(&group.items, item_id, transform) {
            return Some(found);
        }
    }
    None
}

fn collect_outlines(items: &[FrameItem], parent: Affine2D) -> Vec<[egui::Pos2; 4]> {
    let mut outlines = Vec::new();
    for item in items {
        match item {
            FrameItem::Object(object) => {
                let Some(bounds) = object_bounds(object) else {
                    continue;
                };
                let transform = parent.compose(Affine2D::from(object.content.transform()));
                if let Some(outline) = map_rect(transform, bounds) {
                    outlines.push(outline);
                }
            }
            FrameItem::Group(group) if group.kind == FrameGroupKind::Composition => {
                let transform = parent.compose(Affine2D::from(&group.transform));
                let bounds = egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(group.width as f32, group.height as f32),
                );
                if let Some(outline) = map_rect(transform, bounds) {
                    outlines.push(outline);
                }
            }
            FrameItem::Group(group) => {
                let transform = parent.compose(Affine2D::from(&group.transform));
                outlines.extend(collect_outlines(&group.items, transform));
            }
        }
    }
    outlines
}

fn object_bounds(object: &FrameObject) -> Option<egui::Rect> {
    if let Some(bounds) = object.content_bounds {
        let (x, y, width, height) = bounds.as_tuple();
        return positive_rect(x, y, width, height);
    }
    match &object.content {
        FrameContent::Text {
            text, font, size, ..
        } => {
            let (width, height) =
                library::plugin::entity_converter::measure_text_size(text, font, *size as f32);
            positive_rect(0.0, 0.0, width, height)
        }
        FrameContent::SkSL { resolution, .. } => {
            positive_rect(0.0, 0.0, resolution.0, resolution.1)
        }
        FrameContent::Video { .. }
        | FrameContent::Image { .. }
        | FrameContent::Shape { .. }
        | FrameContent::ParticleScene { .. } => None,
    }
}

fn positive_rect(x: f32, y: f32, width: f32, height: f32) -> Option<egui::Rect> {
    (x.is_finite()
        && y.is_finite()
        && width.is_finite()
        && height.is_finite()
        && width > 0.0
        && height > 0.0)
        .then(|| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height)))
}

fn map_rect(transform: Affine2D, bounds: egui::Rect) -> Option<[egui::Pos2; 4]> {
    Some([
        map_point(transform, bounds.min.x, bounds.min.y)?,
        map_point(transform, bounds.max.x, bounds.min.y)?,
        map_point(transform, bounds.max.x, bounds.max.y)?,
        map_point(transform, bounds.min.x, bounds.max.y)?,
    ])
}

fn map_outline(transform: Affine2D, outline: [egui::Pos2; 4]) -> Option<[egui::Pos2; 4]> {
    Some([
        map_point(transform, outline[0].x, outline[0].y)?,
        map_point(transform, outline[1].x, outline[1].y)?,
        map_point(transform, outline[2].x, outline[2].y)?,
        map_point(transform, outline[3].x, outline[3].y)?,
    ])
}

fn map_point(transform: Affine2D, x: f32, y: f32) -> Option<egui::Pos2> {
    let (x, y) = transform.map_point(f64::from(x), f64::from(y));
    (x.is_finite()
        && y.is_finite()
        && x.abs() <= f64::from(f32::MAX)
        && y.abs() <= f64::from(f32::MAX))
    .then(|| egui::pos2(x as f32, y as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::frame::color::Color;
    use library::model::frame::entity::{FrameBounds, FrameGroup, FrameObject, StyleConfig};
    use library::model::frame::transform::{Position, Scale, Transform};
    use library::model::BlendMode;
    use ordered_float::OrderedFloat;
    use uuid::Uuid;

    fn transparent() -> Color {
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        }
    }

    fn group(source_id: Uuid, kind: FrameGroupKind, items: Vec<FrameItem>) -> FrameItem {
        FrameItem::Group(FrameGroup {
            source_id,
            kind,
            width: 1920,
            height: 1080,
            background_color: transparent(),
            transform: Transform::default(),
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(0.0),
            effects: Vec::new(),
            items,
        })
    }

    fn bounded_shape(id: Uuid, width: f32, height: f32) -> FrameItem {
        FrameItem::Object(FrameObject {
            source_node_id: id,
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: Some(FrameBounds::new(10.0, 20.0, width, height)),
            content: FrameContent::Shape {
                path: String::new(),
                canonical_path: None,
                styles: Vec::<StyleConfig>::new(),
                path_effects: Vec::new(),
                effects: Vec::new(),
                ensemble: None,
                transform: Transform::default(),
            },
        })
    }

    fn frame(items: Vec<FrameItem>) -> FrameInfo {
        FrameInfo {
            width: 1920,
            height: 1080,
            background_color: transparent(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items,
        }
    }

    #[test]
    fn clip_uses_source_bounds_instead_of_composition_dimensions() {
        let item_id = TimelineItemId::new();
        let mut clip = match group(
            item_id.as_uuid(),
            FrameGroupKind::Clip,
            vec![bounded_shape(item_id.as_uuid(), 100.0, 50.0)],
        ) {
            FrameItem::Group(group) => group,
            _ => unreachable!(),
        };
        clip.transform = Transform {
            position: Position { x: 200.0, y: 100.0 },
            scale: Scale { x: 2.0, y: 1.0 },
            ..Transform::default()
        };
        let frame = frame(vec![group(
            Uuid::new_v4(),
            FrameGroupKind::Composition,
            vec![FrameItem::Group(clip)],
        )]);

        let geometry = item_gizmo_geometry(&frame, item_id).expect("item geometry");
        assert_eq!(geometry.outlines.len(), 1);
        assert_eq!(geometry.outlines[0][0], egui::pos2(220.0, 120.0));
        assert_eq!(geometry.outlines[0][2], egui::pos2(420.0, 170.0));
        assert_ne!(geometry.outlines[0][2], egui::pos2(3840.0, 1080.0));
    }

    #[test]
    fn nested_composition_uses_its_own_canvas_size() {
        let item_id = TimelineItemId::new();
        let nested = FrameItem::Group(FrameGroup {
            source_id: Uuid::new_v4(),
            kind: FrameGroupKind::Composition,
            width: 320,
            height: 180,
            background_color: transparent(),
            transform: Transform::default(),
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(0.0),
            effects: Vec::new(),
            items: Vec::new(),
        });
        let frame = frame(vec![group(
            item_id.as_uuid(),
            FrameGroupKind::Clip,
            vec![nested],
        )]);

        let geometry = item_gizmo_geometry(&frame, item_id).expect("nested geometry");
        assert_eq!(geometry.outlines[0][0], egui::Pos2::ZERO);
        assert_eq!(geometry.outlines[0][2], egui::pos2(320.0, 180.0));
    }

    #[test]
    fn hit_test_uses_topmost_render_order_and_ignores_other_timeline_items() {
        let bottom_id = TimelineItemId::new();
        let top_id = TimelineItemId::new();
        let nested_id = TimelineItemId::new();
        let frame = frame(vec![
            group(
                bottom_id.as_uuid(),
                FrameGroupKind::Clip,
                vec![bounded_shape(bottom_id.as_uuid(), 100.0, 100.0)],
            ),
            group(
                top_id.as_uuid(),
                FrameGroupKind::Clip,
                vec![group(
                    nested_id.as_uuid(),
                    FrameGroupKind::Clip,
                    vec![bounded_shape(nested_id.as_uuid(), 100.0, 100.0)],
                )],
            ),
        ]);
        let selectable = HashSet::from([bottom_id, top_id]);

        assert_eq!(
            hit_test_item(&frame, &selectable, egui::pos2(50.0, 50.0)),
            Some(top_id)
        );
        assert_eq!(
            hit_test_item(&frame, &HashSet::from([bottom_id]), egui::pos2(50.0, 50.0)),
            Some(bottom_id)
        );
        assert_eq!(
            hit_test_item(&frame, &selectable, egui::pos2(500.0, 500.0)),
            None
        );
    }

    #[test]
    fn rotated_outline_hit_test_is_not_replaced_by_axis_aligned_bounds() {
        let item_id = TimelineItemId::new();
        let mut clip = match group(
            item_id.as_uuid(),
            FrameGroupKind::Clip,
            vec![bounded_shape(item_id.as_uuid(), 100.0, 50.0)],
        ) {
            FrameItem::Group(group) => group,
            _ => unreachable!(),
        };
        clip.transform.rotation = 45.0;
        let frame = frame(vec![FrameItem::Group(clip)]);
        let selectable = HashSet::from([item_id]);
        let geometry = item_gizmo_geometry(&frame, item_id).expect("geometry");
        let inside = geometry.outlines[0]
            .iter()
            .fold(egui::Vec2::ZERO, |sum, point| sum + point.to_vec2())
            / 4.0;

        assert_eq!(
            hit_test_item(&frame, &selectable, inside.to_pos2()),
            Some(item_id)
        );
        assert_eq!(
            hit_test_item(&frame, &selectable, egui::pos2(-30.0, 10.0)),
            None
        );
    }
}
