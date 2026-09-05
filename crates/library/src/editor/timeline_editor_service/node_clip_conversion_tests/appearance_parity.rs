use super::*;

fn style_with_number(
    plugins: &PluginManager,
    component_id: &str,
    key: &str,
    value: f64,
) -> AppearanceOperation {
    let mut operation = AppearanceOperationFactory::create(plugins, component_id)
        .expect("production Appearance operation");
    operation.properties.set(
        key.to_string(),
        Property::constant(PropertyValue::from(value)),
    );
    operation
}

fn shadow(plugins: &PluginManager) -> AppearanceOperation {
    let mut operation = style_with_number(plugins, "drop_shadow", "distance", 7.0);
    operation.properties.set(
        "size".to_string(),
        Property::constant(PropertyValue::from(3.0)),
    );
    operation
}

fn assert_shape_conversion_pixels(
    plugins: &Arc<PluginManager>,
    name: &str,
    appearance_operations: Vec<AppearanceOperation>,
) {
    let (service, track_id) = small_service(name);
    let (item_id, _) = service
        .add_item(
            track_id,
            name.to_string(),
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::from([
                        ("width".to_string(), PropertyValue::from(42.0)),
                        ("height".to_string(), PropertyValue::from(30.0)),
                    ]),
                    appearance_operations,
                },
            },
            interval(2),
            0,
        )
        .expect("direct Shape fixture");
    let before = service.snapshot().expect("direct Project");
    let direct_pixels = rendered_pixels(&before, Arc::clone(plugins), 0);
    service
        .convert_source_to_node_clip(plugins.as_ref(), item_id)
        .expect("Shape to Node Clip conversion");
    let converted = service.snapshot().expect("converted Project");
    assert_eq!(
        rendered_pixels(&converted, Arc::clone(plugins), 0),
        direct_pixels,
        "{name} conversion changed exact Preview pixels"
    );
}

#[test]
fn stroke_only_then_shadow_conversion_preserves_composed_alpha_pixels() {
    let plugins = Arc::new(PluginManager::default());
    assert_shape_conversion_pixels(
        &plugins,
        "Stroke-only then Drop Shadow",
        vec![
            stroke(plugins.as_ref(), color(240, 120, 20, 255), 7.0),
            shadow(plugins.as_ref()),
        ],
    );
}

#[test]
fn offset_fill_then_shadow_conversion_preserves_composed_alpha_pixels() {
    let plugins = Arc::new(PluginManager::default());
    let mut fill = fill(plugins.as_ref(), color(30, 70, 210, 255));
    fill.properties.set(
        "offset".to_string(),
        Property::constant(PropertyValue::from(8.0)),
    );
    assert_shape_conversion_pixels(
        &plugins,
        "Offset Fill then Drop Shadow",
        vec![fill, shadow(plugins.as_ref())],
    );
}

#[test]
fn partial_alpha_fill_then_shadow_conversion_preserves_composed_alpha_pixels() {
    let plugins = Arc::new(PluginManager::default());
    assert_shape_conversion_pixels(
        &plugins,
        "Partial-alpha Fill then Drop Shadow",
        vec![
            fill(plugins.as_ref(), color(30, 70, 210, 96)),
            shadow(plugins.as_ref()),
        ],
    );
}
