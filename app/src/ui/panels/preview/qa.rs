use std::collections::{HashMap, HashSet};

use crate::state::context::EditorContext;

use super::clip;

pub(super) fn register_preview_qa_components(
    preview_rect: egui::Rect,
    composition: Option<(uuid::Uuid, u64, u64)>,
    editor_context: &EditorContext,
) {
    if !crate::qa::is_enabled() {
        return;
    }

    let preview_content = composition.and_then(|(composition_id, width, height)| {
        super::support::preview_content_rect(
            preview_rect,
            editor_context.view.pan,
            editor_context.view.zoom,
            egui::vec2(width as f32, height as f32),
        )
        .map(|rect| (composition_id, width, height, rect))
    });
    crate::qa::register_component_with_metadata(
        "preview.canvas",
        "preview_canvas",
        preview_rect,
        true,
        Some(serde_json::json!({
            "pan": {"x": editor_context.view.pan.x, "y": editor_context.view.pan.y},
            "zoom": editor_context.view.zoom,
            "auto_fit": editor_context.interaction.preview_viewport.auto_fit,
            "primary_gesture": format!(
                "{:?}",
                editor_context.interaction.preview_viewport.primary_gesture
            ),
            "is_moving_selected_entity": editor_context.interaction.is_moving_selected_entity,
            "selection_drag_active": editor_context
                .interaction
                .preview_selection_drag_start
                .is_some(),
            "body_drag_active": editor_context.interaction.body_drag_state.is_some(),
            "gizmo_active": editor_context.interaction.gizmo_state.is_some(),
            "composition_id": preview_content.map(|content| content.0),
            "texture_width": editor_context.preview_texture_width,
            "texture_height": editor_context.preview_texture_height,
        })),
    );
    if let Some((composition_id, width, height, content_rect)) = preview_content {
        crate::qa::register_component_with_metadata(
            "preview.content",
            "preview_composition_content",
            content_rect,
            true,
            Some(serde_json::json!({
                "composition_id": composition_id,
                "canvas_width": width,
                "canvas_height": height,
                "pan": {"x": editor_context.view.pan.x, "y": editor_context.view.pan.y},
                "zoom": editor_context.view.zoom,
                "auto_fit": editor_context.interaction.preview_viewport.auto_fit,
            })),
        );
    }
}

pub(super) fn register_preview_tool_component(
    id: &str,
    tool: &str,
    response: &egui::Response,
    selected: bool,
) {
    if !crate::qa::is_enabled() {
        return;
    }
    crate::qa::register_component_with_metadata(
        id,
        "preview_tool",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "tool": tool,
            "selected": selected,
            "action": "activate_preview_tool",
        })),
    );
}

fn preview_visual_screen_corners(
    visual: &clip::PreviewClip,
    to_screen: &impl Fn(egui::Pos2) -> egui::Pos2,
) -> Option<[egui::Pos2; 4]> {
    let (x, y, width, height) = visual.content_bounds?;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let mut screen_points = [egui::Pos2::ZERO; 4];
    for (point, (local_x, local_y)) in screen_points.iter_mut().zip([
        (x, y),
        (x + width, y),
        (x + width, y + height),
        (x, y + height),
    ]) {
        let (world_x, world_y) = visual
            .world_transform
            .map_point(f64::from(local_x), f64::from(local_y));
        *point = to_screen(egui::pos2(world_x as f32, world_y as f32));
    }
    screen_points
        .iter()
        .all(|point| point.x.is_finite() && point.y.is_finite())
        .then_some(screen_points)
}

#[cfg(test)]
pub(super) fn preview_visual_screen_rect(
    visual: &clip::PreviewClip,
    to_screen: &impl Fn(egui::Pos2) -> egui::Pos2,
) -> Option<egui::Rect> {
    let rect = egui::Rect::from_points(&preview_visual_screen_corners(visual, to_screen)?);
    rect.is_positive().then_some(rect)
}

/// Publish only coordinates that traverse the same top-most polygon hit test
/// as a real Preview pointer event. Bounding-box centers are not sufficient:
/// a rotated visual can miss its own box center and a lower visual can be
/// completely occluded by a later Merge input.
pub(super) fn register_preview_visual_qa_components(
    visuals: &[clip::PreviewClip],
    viewport: egui::Rect,
    to_screen: &impl Fn(egui::Pos2) -> egui::Pos2,
) {
    if !crate::qa::is_enabled() {
        return;
    }

    let polygons = visuals
        .iter()
        .map(|visual| preview_visual_screen_corners(visual, to_screen))
        .collect::<Vec<_>>();
    let actionable_points = polygons
        .iter()
        .enumerate()
        .map(|(index, polygon)| {
            polygon.and_then(|polygon| actionable_visual_point(index, polygon, &polygons, viewport))
        })
        .collect::<Vec<_>>();
    let occurrence_keys = instance_occurrence_keys(visuals);
    let mut published_content = HashSet::new();
    let mut published_spatial = HashSet::new();

    for (instance_index, visual) in visuals.iter().enumerate().rev() {
        let Some(corners) = polygons[instance_index] else {
            continue;
        };
        let unclipped_rect = egui::Rect::from_points(&corners);
        let clipped_rect = unclipped_rect.intersect(viewport);
        if !clipped_rect.is_positive() {
            continue;
        }
        let actionable_point = actionable_points[instance_index];
        let component_rect = actionable_point
            .map(|point| egui::Rect::from_center_size(point, egui::Vec2::splat(1.0)))
            .unwrap_or(clipped_rect);
        let editable_spatial_node_id = visual.editable_spatial_id();
        let spatial_layers = visual
            .spatial_layers
            .iter()
            .map(|layer| {
                serde_json::json!({
                    "node_id": layer.node.id,
                    "kind": match layer.kind {
                        clip::PreviewSpatialKind::Content => "content",
                        clip::PreviewSpatialKind::ShapeTransform => "shape_transform",
                        clip::PreviewSpatialKind::ImageTransform => "image_transform",
                    },
                    "editable": visual.spatial_layer(layer.node.id).is_some(),
                    "canonical_edit_layer": editable_spatial_node_id == Some(layer.node.id),
                })
            })
            .collect::<Vec<_>>();
        let metadata = serde_json::json!({
            "content_node_id": visual.content_id(),
            "owner": visual.owner_target,
            "spatial_node_id": visual.spatial_id(),
            "editable_spatial_node_id": editable_spatial_node_id,
            "spatial_layers": spatial_layers,
            "instance_path": &visual.instance_path,
            "instance_index": instance_index,
            "polygon_points": corners.map(|point| serde_json::json!({"x": point.x, "y": point.y})),
            "unclipped_rect_points": {
                "min_x": unclipped_rect.min.x,
                "min_y": unclipped_rect.min.y,
                "max_x": unclipped_rect.max.x,
                "max_y": unclipped_rect.max.y,
            },
            "actionable_point": actionable_point.map(|point| serde_json::json!({"x": point.x, "y": point.y})),
            "hit_test": "topmost_convex_polygon",
            "disabled_reason": actionable_point.is_none().then_some("occluded_or_outside_viewport"),
            "action": "select_or_drag_preview_visual",
        });
        crate::qa::register_component_with_metadata(
            format!(
                "preview.visual.instance:{}",
                occurrence_keys[instance_index]
            ),
            "preview_visual_instance",
            component_rect,
            actionable_point.is_some(),
            Some(metadata.clone()),
        );
        if actionable_point.is_some() && published_content.insert(visual.content_id()) {
            crate::qa::register_component_with_metadata(
                format!("preview.visual.content:{}", visual.content_id()),
                "preview_content_visual",
                component_rect,
                true,
                Some(metadata.clone()),
            );
        }
        for layer in &visual.spatial_layers {
            let canonical = editable_spatial_node_id == Some(layer.node.id);
            if !canonical || actionable_point.is_none() || !published_spatial.insert(layer.node.id)
            {
                continue;
            }
            crate::qa::register_component_with_metadata(
                format!("preview.visual.spatial:{}", layer.node.id),
                "preview_spatial_visual",
                component_rect,
                true,
                Some(metadata.clone()),
            );
        }
    }
}

fn instance_occurrence_keys(visuals: &[clip::PreviewClip]) -> Vec<String> {
    let mut occurrences = HashMap::<String, usize>::new();
    visuals
        .iter()
        .map(|visual| {
            let path = visual
                .instance_path
                .iter()
                .map(uuid::Uuid::to_string)
                .collect::<Vec<_>>()
                .join("/");
            let occurrence = occurrences.entry(path.clone()).or_default();
            let key = format!("{path}:occurrence:{occurrence}");
            *occurrence += 1;
            key
        })
        .collect()
}

fn actionable_visual_point(
    visual_index: usize,
    polygon: [egui::Pos2; 4],
    polygons: &[Option<[egui::Pos2; 4]>],
    viewport: egui::Rect,
) -> Option<egui::Pos2> {
    let bounds = egui::Rect::from_points(&polygon).intersect(viewport);
    if !bounds.is_positive() {
        return None;
    }

    let centroid = polygon
        .iter()
        .fold(egui::Vec2::ZERO, |sum, point| sum + point.to_vec2())
        / 4.0;
    let centroid = centroid.to_pos2();
    let mut candidates = Vec::with_capacity(300);
    candidates.push(bounds.center());
    candidates.push(centroid);
    for corner in polygon {
        candidates.push(corner.lerp(centroid, 0.1));
    }
    for edge in 0..4 {
        candidates.push(
            polygon[edge]
                .lerp(polygon[(edge + 1) % 4], 0.5)
                .lerp(centroid, 0.1),
        );
    }
    const GRID: usize = 17;
    for y in 0..GRID {
        for x in 0..GRID {
            candidates.push(egui::pos2(
                egui::lerp(bounds.x_range(), (x as f32 + 0.5) / GRID as f32),
                egui::lerp(bounds.y_range(), (y as f32 + 0.5) / GRID as f32),
            ));
        }
    }

    candidates.into_iter().find(|point| {
        viewport.contains(*point)
            && point_in_convex_polygon(*point, polygon)
            && topmost_visual_at(*point, polygons) == Some(visual_index)
    })
}

fn topmost_visual_at(point: egui::Pos2, polygons: &[Option<[egui::Pos2; 4]>]) -> Option<usize> {
    polygons
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, polygon)| {
            polygon
                .is_some_and(|polygon| point_in_convex_polygon(point, polygon))
                .then_some(index)
        })
}

pub(super) fn point_in_convex_polygon(point: egui::Pos2, polygon: [egui::Pos2; 4]) -> bool {
    let edge_side = |start: egui::Pos2, end: egui::Pos2| {
        (end.x - start.x) * (point.y - start.y) - (end.y - start.y) * (point.x - start.x)
    };
    let sides = [
        edge_side(polygon[0], polygon[1]),
        edge_side(polygon[1], polygon[2]),
        edge_side(polygon[2], polygon[3]),
        edge_side(polygon[3], polygon[0]),
    ];
    let has_positive = sides.iter().any(|side| *side > 0.0);
    let has_negative = sides.iter().any(|side| *side < 0.0);
    !(has_positive && has_negative)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(min_x: f32, min_y: f32, size: f32) -> [egui::Pos2; 4] {
        [
            egui::pos2(min_x, min_y),
            egui::pos2(min_x + size, min_y),
            egui::pos2(min_x + size, min_y + size),
            egui::pos2(min_x, min_y + size),
        ]
    }

    #[test]
    fn fully_occluded_visual_is_not_coordinate_actionable() {
        let lower = square(10.0, 10.0, 40.0);
        let upper = lower;
        let polygons = vec![Some(lower), Some(upper)];
        let viewport = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(100.0, 100.0));

        assert!(actionable_visual_point(0, lower, &polygons, viewport).is_none());
        assert!(actionable_visual_point(1, upper, &polygons, viewport).is_some());
    }

    #[test]
    fn partially_exposed_visual_publishes_a_real_topmost_point() {
        let lower = square(10.0, 10.0, 60.0);
        let upper = square(40.0, 10.0, 60.0);
        let polygons = vec![Some(lower), Some(upper)];
        let viewport = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(100.0, 100.0));

        let point = actionable_visual_point(0, lower, &polygons, viewport)
            .expect("left side remains exposed");
        assert_eq!(topmost_visual_at(point, &polygons), Some(0));
        assert!(point_in_convex_polygon(point, lower));
        assert!(!point_in_convex_polygon(point, upper));
    }

    #[test]
    fn convex_hit_test_handles_both_windings() {
        let clockwise = square(0.0, 0.0, 10.0);
        let counter_clockwise = [clockwise[0], clockwise[3], clockwise[2], clockwise[1]];
        assert!(point_in_convex_polygon(egui::pos2(5.0, 5.0), clockwise));
        assert!(point_in_convex_polygon(
            egui::pos2(5.0, 5.0),
            counter_clockwise
        ));
        assert!(!point_in_convex_polygon(egui::pos2(15.0, 5.0), clockwise));
    }
}
