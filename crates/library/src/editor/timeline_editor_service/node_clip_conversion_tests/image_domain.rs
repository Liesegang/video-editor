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

#[test]
fn fill_and_stroke_paints_render_spatially_and_survive_conversion_undo_and_reload() {
    use crate::model::property::{GradientValue, Paint, PatternValue};

    let plugins = Arc::new(PluginManager::default());
    for component in ["fill", "stroke"] {
        for paint in [
            Paint::Gradient(GradientValue::default()),
            Paint::Pattern(PatternValue::default()),
        ] {
            let mut operation = AppearanceOperationFactory::create(&plugins, component).unwrap();
            operation.properties.set(
                "paint".to_string(),
                Property::constant(PropertyValue::Paint(paint.clone())),
            );
            if component == "stroke" {
                operation.properties.set(
                    "width".to_string(),
                    Property::constant(PropertyValue::from(8.0)),
                );
            }
            let (service, item) = rectangle(96, 64, vec![operation]);
            let direct_project = service.snapshot().unwrap();
            let direct = rendered_pixels(&direct_project, Arc::clone(&plugins), 0);
            let y = if component == "fill" { 32 } else { 22 };
            let left = pixel(&direct, 96, 38, y);
            let right = pixel(&direct, 96, 56, y);
            assert!(
                (i16::from(left[0]) - i16::from(right[0])).abs() > 60,
                "{component} {paint:?} was flattened or missing: {left:?}, {right:?}"
            );
            service.convert_source_to_node_clip(&plugins, item).unwrap();
            let converted = service.snapshot().unwrap();
            assert_eq!(
                rendered_pixels(&converted, Arc::clone(&plugins), 0),
                direct,
                "{component} {paint:?} changed pixels during explicit conversion"
            );
            let serialized = serde_json::to_string(converted.as_ref()).unwrap();
            let loaded: AuthoringProject = serde_json::from_str(&serialized).unwrap();
            loaded.validate().unwrap();
            assert_eq!(&loaded, converted.as_ref());
            assert_eq!(rendered_pixels(&loaded, Arc::clone(&plugins), 0), direct);
            service.undo().unwrap().unwrap();
            assert_eq!(
                service.snapshot().unwrap().as_ref(),
                direct_project.as_ref()
            );
            service.redo().unwrap().unwrap();
            assert_eq!(service.snapshot().unwrap().as_ref(), converted.as_ref());
        }
    }
}

#[test]
fn gradient_node_injects_losslessly_into_converted_fill_paint() {
    use crate::model::node::DataContent;
    use crate::model::project::connection::DATA_VALUE_OUTPUT_PORT;
    use crate::model::property::{GradientValue, Paint};

    let plugins = Arc::new(PluginManager::default());
    let mut operation = fill(&plugins, Color::white());
    operation.properties.set(
        "paint".to_string(),
        Property::constant(PropertyValue::Paint(Paint::Gradient(
            GradientValue::default(),
        ))),
    );
    let (service, item) = rectangle(96, 64, vec![operation]);
    let expected = rendered_pixels(&service.snapshot().unwrap(), Arc::clone(&plugins), 0);
    let conversion = service.convert_source_to_node_clip(&plugins, item).unwrap();
    let snapshot = service.snapshot().unwrap();
    let definition = &snapshot.module_definitions[&conversion.definition_id];
    let fill_id = definition.graph.nodes.values().find(|node| {
        matches!(node.content(), NodeContent::PluginOperation(operation) if operation.component_id == "fill")
    }).unwrap().id;
    let paint_parameter = definition
        .interface
        .parameters
        .iter()
        .find(|parameter| {
            parameter.target.node_id == fill_id
                && parameter.target.port == crate::plugin::property_port_key("paint")
        })
        .unwrap()
        .id;
    service
        .edit_instance_module_interface(
            conversion.instance_id,
            crate::editor::ModuleInterfaceCommand::UnpublishParameter {
                parameter_id: paint_parameter,
            },
        )
        .unwrap();
    let gradient = crate::model::Node::new_data("Gradient", DataContent::Gradient);
    let gradient_id = gradient.id;
    service
        .add_instance_module_node(conversion.instance_id, gradient)
        .unwrap();
    service
        .connect_instance_module_ports(
            conversion.instance_id,
            ModulePortAddress {
                node_id: gradient_id,
                port: DATA_VALUE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: fill_id,
                port: crate::plugin::property_port_key("paint"),
            },
            0,
        )
        .unwrap();
    let connected = service.snapshot().unwrap();
    assert_eq!(
        rendered_pixels(&connected, Arc::clone(&plugins), 0),
        expected
    );
    assert!(pixel(&expected, 96, 56, 32)[0] > pixel(&expected, 96, 38, 32)[0] + 60);
}
