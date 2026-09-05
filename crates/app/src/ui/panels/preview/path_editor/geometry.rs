use library::model::authoring::TimelineItemId;
use library::model::frame::entity::{FrameContent, FrameGroupKind, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::rendering::renderer::Affine2D;

/// Find the local-to-composition transform of the canonical Path object that
/// produced the displayed pixels for one Timeline item.
pub(super) fn item_path_transform(frame: &FrameInfo, item_id: TimelineItemId) -> Option<Affine2D> {
    find_path(&frame.items, item_id, Affine2D::IDENTITY, false)
}

fn find_path(
    items: &[FrameItem],
    item_id: TimelineItemId,
    parent: Affine2D,
    inside_item: bool,
) -> Option<Affine2D> {
    for item in items {
        match item {
            FrameItem::Object(object)
                if inside_item
                    && object.source_node_id == item_id.as_uuid()
                    && matches!(
                        &object.content,
                        FrameContent::Shape {
                            canonical_path: Some(_),
                            ..
                        }
                    ) =>
            {
                return Some(parent.compose(Affine2D::from(object.content.transform())));
            }
            FrameItem::Object(_) => {}
            FrameItem::Group(group) => {
                let transform = parent.compose(Affine2D::from(&group.transform));
                let inside_item = inside_item
                    || (group.kind == FrameGroupKind::Clip && group.source_id == item_id.as_uuid());
                if let Some(found) = find_path(&group.items, item_id, transform, inside_item) {
                    return Some(found);
                }
            }
            FrameItem::Transition(transition) => {
                if let Some(found) = find_path(
                    std::slice::from_ref(&transition.from.item),
                    item_id,
                    parent,
                    inside_item,
                ) {
                    return Some(found);
                }
                if let Some(found) = find_path(
                    std::slice::from_ref(&transition.to.item),
                    item_id,
                    parent,
                    inside_item,
                ) {
                    return Some(found);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use ordered_float::OrderedFloat;

    use super::*;
    use library::model::frame::color::Color;
    use library::model::frame::entity::{FrameGroup, FrameObject};
    use library::model::frame::transform::{Position, Scale, Transform};
    use library::model::path::{FillRule, PathValue};
    use library::model::BlendMode;

    fn transparent() -> Color {
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        }
    }

    #[test]
    fn path_transform_uses_the_displayed_parent_and_item_stack() {
        let item_id = TimelineItemId::new();
        let object = FrameItem::Object(FrameObject {
            source_node_id: item_id.as_uuid(),
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: None,
            content: FrameContent::Shape {
                path: String::new(),
                canonical_path: Some(PathValue::empty(FillRule::NonZero)),
                parts: Vec::new(),
                styles: Vec::new(),
                path_effects: Vec::new(),
                effects: Vec::new(),
                ensemble: None,
                transform: Transform::default(),
            },
        });
        let clip = FrameItem::Group(FrameGroup {
            source_id: item_id.as_uuid(),
            kind: FrameGroupKind::Clip,
            width: 640,
            height: 360,
            background_color: transparent(),
            transform: Transform {
                position: Position { x: 30.0, y: 40.0 },
                scale: Scale { x: 2.0, y: 3.0 },
                ..Transform::default()
            },
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(0.0),
            effects: Vec::new(),
            items: vec![object],
        });
        let frame = FrameInfo {
            width: 640,
            height: 360,
            background_color: transparent(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![clip],
        };

        let transform = item_path_transform(&frame, item_id).expect("displayed Path transform");
        assert_eq!(transform.map_point(10.0, 20.0), (50.0, 100.0));
    }
}
