use library::core::render_plan::{RenderPlanCompiler, evaluate_render_plan_frame};
use library::editor::{AppearanceOperationFactory, AuthoringPropertyOwner, TimelineEditorService};
use library::model::authoring::{MediaTime, ShapeKind, ShapeSource, SourceRef, TimelineInterval};
use library::model::frame::color::Color;
use library::model::frame::draw_type::DrawStyle;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::property::{ColorValue, PropertyValue};
use library::plugin::PluginManager;
use std::collections::HashMap;

fn source_shapes(items: &[FrameItem]) -> Vec<(&str, Color)> {
    let mut result = Vec::new();
    for item in items {
        match item {
            FrameItem::Group(group) => result.extend(source_shapes(&group.items)),
            FrameItem::Transition(transition) => {
                result.extend(source_shapes(std::slice::from_ref(&transition.from.item)));
                result.extend(source_shapes(std::slice::from_ref(&transition.to.item)));
            }
            FrameItem::Object(object) => {
                if let FrameContent::Shape { path, styles, .. } = &object.content {
                    for style in styles {
                        if let DrawStyle::Fill { color, .. } = &style.style {
                            result.push((path.as_str(), color.clone()));
                        }
                    }
                }
            }
        }
    }
    result
}

#[test]
fn solid_and_shape_source_controls_evaluate_timeline_keys_without_changing_siblings() {
    let plugins = PluginManager::default();
    let rectangle_fill =
        AppearanceOperationFactory::create(&plugins, "fill").expect("Rectangle Fill");
    let ellipse_fill = AppearanceOperationFactory::create(&plugins, "fill").expect("Ellipse Fill");
    for (source, appearance_operation_id) in [
        (
            SourceRef::Solid {
                color: Color::white(),
            },
            None,
        ),
        (
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::new(),
                    appearance_operations: vec![rectangle_fill.clone()],
                },
            },
            Some(rectangle_fill.id),
        ),
        (
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Ellipse,
                    parameters: HashMap::new(),
                    appearance_operations: vec![ellipse_fill.clone()],
                },
            },
            Some(ellipse_fill.id),
        ),
    ] {
        let service = TimelineEditorService::create_default("Source properties").unwrap();
        let project = service.snapshot().unwrap();
        let track = project.timelines[&project.root_timeline_id].track_order[0];
        let interval =
            TimelineInterval::new(MediaTime::zero(), MediaTime::from_whole_seconds(3)).unwrap();
        let (item, _) = service
            .add_item(track, "Edited".into(), source.clone(), interval, 0)
            .unwrap();
        let (sibling, _) = service
            .add_item(track, "Sibling".into(), source.clone(), interval, 1)
            .unwrap();
        let red = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        };
        let blue = Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        };
        for (seconds, color) in [(0, red.clone()), (1, blue.clone())] {
            let owner = appearance_operation_id.map_or(
                AuthoringPropertyOwner::Item(item),
                |operation_id| AuthoringPropertyOwner::Appearance {
                    item_id: item,
                    operation_id,
                },
            );
            let value = appearance_operation_id.map_or_else(
                || PropertyValue::Color(color.clone()),
                |_| PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color)),
            );
            service
                .upsert_authored_property_keyframe(
                    owner,
                    "color".into(),
                    MediaTime::from_whole_seconds(seconds),
                    value,
                    None,
                )
                .unwrap();
        }
        if matches!(source, SourceRef::Shape { .. }) {
            for (key, value) in [("width", 240.0), ("height", 80.0)] {
                service
                    .set_authored_property_constant(
                        AuthoringPropertyOwner::Item(item),
                        key.into(),
                        PropertyValue::from(value),
                    )
                    .unwrap();
            }
        }
        let project = service.snapshot().unwrap();
        if appearance_operation_id.is_none() {
            assert_eq!(project.items[&item].source, source);
        } else {
            assert_ne!(project.items[&item].source, source);
        }
        assert_eq!(project.items[&sibling].source, source);
        assert!(
            project.items[&sibling]
                .authored_properties
                .iter()
                .next()
                .is_none()
        );
        assert!(project.module_definitions.is_empty());
        let plan = RenderPlanCompiler::compile(&project).unwrap();
        let fps = project.timelines[&project.root_timeline_id].fps.to_f64() as u64;
        for (frame, color) in [(0, red), (fps, blue)] {
            let frame =
                evaluate_render_plan_frame(&project, &plan, &plugins, frame, 1.0, None).unwrap();
            let shapes = source_shapes(&frame.items);
            assert!(shapes.iter().any(|(_, actual)| actual == &color));
            assert!(shapes.iter().any(|(_, actual)| actual == &Color::white()));
            if matches!(source, SourceRef::Shape { .. }) {
                assert!(
                    shapes
                        .iter()
                        .any(|(path, actual)| actual == &color && path.contains("240"))
                );
            }
        }
    }
}
