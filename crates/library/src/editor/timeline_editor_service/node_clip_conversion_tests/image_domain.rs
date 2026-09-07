use super::*;

fn rectangle(
    width: u64,
    height: u64,
    operations: Vec<AppearanceOperation>,
) -> (TimelineEditorService, TimelineItemId) {
    let project = AuthoringProject::new(
        "Image domain",
        width,
        height,
        RationalRate::new(30, 1).unwrap(),
        time(4),
    )
    .unwrap();
    let track = project.timelines[&project.root_timeline_id].track_order[0];
    let service = TimelineEditorService::new(project).unwrap();
    let (item, _) = service
        .add_item(
            track,
            "Rectangle".to_string(),
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::from([
                        ("width".to_string(), PropertyValue::from(24.0)),
                        ("height".to_string(), PropertyValue::from(18.0)),
                    ]),
                    appearance_operations: operations,
                },
            },
            interval(2),
            0,
        )
        .unwrap();
    for (key, value) in [("position", vec2(36.0, 24.0)), ("anchor", vec2(0.0, 0.0))] {
        service
            .set_authored_property_constant(
                AuthoringPropertyOwner::Item(item),
                key.to_string(),
                value,
            )
            .unwrap();
    }
    (service, item)
}

fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> &[u8] {
    let index = (y * width + x) * 4;
    &pixels[index..index + 4]
}

#[test]
fn left_cast_shadow_survives_image_isolation_before_placement() {
    let plugins = Arc::new(PluginManager::default());
    let mut shadow = AppearanceOperationFactory::create(&plugins, "drop_shadow").unwrap();
    for (key, value) in [
        ("angle", 0.0),
        ("distance", 12.0),
        ("size", 0.0),
        ("spread", 0.0),
    ] {
        shadow.properties.set(
            key.to_string(),
            Property::constant(PropertyValue::from(value)),
        );
    }
    shadow.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::ColorValue(ColorValue::from_straight_srgba8(
            &color(255, 0, 0, 255),
        ))),
    );
    let (service, item) = rectangle(
        96,
        64,
        vec![fill(&plugins, color(255, 255, 255, 255)), shadow],
    );
    let direct = rendered_pixels(&service.snapshot().unwrap(), Arc::clone(&plugins), 0);
    let cast = pixel(&direct, 96, 30, 32);
    assert!(
        cast[0] > 180 && cast[1] < 40 && cast[2] < 40,
        "left shadow was clipped: {cast:?}"
    );
    service.convert_source_to_node_clip(&plugins, item).unwrap();
    assert_eq!(
        rendered_pixels(&service.snapshot().unwrap(), plugins, 0),
        direct
    );
}

#[test]
fn image_opacity_does_not_clip_negative_stroke_support() {
    let plugins = Arc::new(PluginManager::default());
    let body = stroke(&plugins, color(240, 120, 20, 255), 8.0);
    let (plain, _) = rectangle(96, 64, vec![body.clone()]);
    let (styled, item) = rectangle(
        96,
        64,
        vec![
            body,
            AppearanceOperationFactory::create(&plugins, "image_opacity").unwrap(),
        ],
    );
    let expected = rendered_pixels(&plain.snapshot().unwrap(), Arc::clone(&plugins), 0);
    assert!(pixel(&expected, 96, 33, 32)[0] > 180);
    let actual = rendered_pixels(&styled.snapshot().unwrap(), Arc::clone(&plugins), 0);
    assert_eq!(
        actual, expected,
        "identity Image stage clipped the Stroke raster"
    );
    styled.convert_source_to_node_clip(&plugins, item).unwrap();
    assert_eq!(
        rendered_pixels(&styled.snapshot().unwrap(), plugins, 0),
        expected
    );
}

#[test]
fn gradient_domain_is_source_ink_not_composition_resolution() {
    let plugins = Arc::new(PluginManager::default());
    let operations = vec![
        fill(&plugins, color(255, 255, 255, 255)),
        AppearanceOperationFactory::create(&plugins, "gradient_overlay").unwrap(),
    ];
    let (small, small_item) = rectangle(96, 64, operations.clone());
    let (large, large_item) = rectangle(192, 128, operations);
    for converted in [false, true] {
        if converted {
            small
                .convert_source_to_node_clip(&plugins, small_item)
                .unwrap();
            large
                .convert_source_to_node_clip(&plugins, large_item)
                .unwrap();
        }
        let first = rendered_pixels(&small.snapshot().unwrap(), Arc::clone(&plugins), 0);
        let second = rendered_pixels(&large.snapshot().unwrap(), Arc::clone(&plugins), 0);
        for y in 25..41 {
            for x in 37..59 {
                assert_eq!(
                    pixel(&first, 96, x, y),
                    pixel(&second, 192, x, y),
                    "composition changed Gradient at ({x},{y}), converted={converted}"
                );
            }
        }
    }
}
