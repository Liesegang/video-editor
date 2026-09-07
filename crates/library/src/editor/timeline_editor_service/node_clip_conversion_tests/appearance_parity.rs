use super::*;

use crate::model::property::GradientValue;

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

fn color_overlay(plugins: &PluginManager) -> AppearanceOperation {
    let mut operation = style_with_number(plugins, "color_overlay", "opacity", 0.65);
    operation.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::ColorValue(ColorValue::from_straight_srgba8(
            &color(230, 35, 80, 255),
        ))),
    );
    operation
}

fn gradient_overlay(plugins: &PluginManager) -> AppearanceOperation {
    let mut operation =
        AppearanceOperationFactory::create(plugins, "gradient_overlay").expect("Gradient Overlay");
    operation.properties.set(
        "gradient".to_string(),
        Property::constant(PropertyValue::Gradient(GradientValue::default())),
    );
    operation
}

fn assert_shape_conversion_pixels(
    plugins: &Arc<PluginManager>,
    name: &str,
    appearance_operations: Vec<AppearanceOperation>,
) -> Vec<u8> {
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
    direct_pixels
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

#[test]
fn shadow_and_color_overlay_order_is_pixel_distinct_and_each_order_converts_exactly() {
    let plugins = Arc::new(PluginManager::default());
    let shadow_then_overlay = assert_shape_conversion_pixels(
        &plugins,
        "Fill then Drop Shadow then Color Overlay",
        vec![
            fill(plugins.as_ref(), color(30, 70, 210, 255)),
            shadow(plugins.as_ref()),
            color_overlay(plugins.as_ref()),
        ],
    );
    let overlay_then_shadow = assert_shape_conversion_pixels(
        &plugins,
        "Fill then Color Overlay then Drop Shadow",
        vec![
            fill(plugins.as_ref(), color(30, 70, 210, 255)),
            color_overlay(plugins.as_ref()),
            shadow(plugins.as_ref()),
        ],
    );
    assert_ne!(
        shadow_then_overlay, overlay_then_shadow,
        "the parity oracle must exercise authored Image operation order"
    );
}

#[test]
fn gradient_overlay_text_and_shape_convert_at_identity_and_fractional_placement_scale() {
    let plugins = Arc::new(PluginManager::default());
    for (source_name, fractional, source) in [
        (
            "Shape",
            false,
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::from([
                        ("width".to_string(), PropertyValue::from(42.0)),
                        ("height".to_string(), PropertyValue::from(30.0)),
                    ]),
                    appearance_operations: vec![
                        fill(plugins.as_ref(), color(30, 70, 210, 255)),
                        gradient_overlay(plugins.as_ref()),
                    ],
                },
            },
        ),
        (
            "Shape fractional",
            true,
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::from([
                        ("width".to_string(), PropertyValue::from(42.0)),
                        ("height".to_string(), PropertyValue::from(30.0)),
                    ]),
                    appearance_operations: vec![
                        fill(plugins.as_ref(), color(30, 70, 210, 255)),
                        gradient_overlay(plugins.as_ref()),
                    ],
                },
            },
        ),
        (
            "Text",
            false,
            SourceRef::Text {
                text: "Gradient".to_string(),
                appearance_operations: vec![
                    fill(plugins.as_ref(), color(30, 70, 210, 255)),
                    gradient_overlay(plugins.as_ref()),
                ],
                ensemble_operations: Vec::new(),
            },
        ),
        (
            "Text fractional",
            true,
            SourceRef::Text {
                text: "Gradient".to_string(),
                appearance_operations: vec![
                    fill(plugins.as_ref(), color(30, 70, 210, 255)),
                    gradient_overlay(plugins.as_ref()),
                ],
                ensemble_operations: Vec::new(),
            },
        ),
    ] {
        let (service, track_id) = small_service(source_name);
        let (item_id, _) = service
            .add_item(track_id, source_name.to_string(), source, interval(2), 0)
            .expect("direct Appearance fixture");
        if fractional {
            service
                .set_authored_property_constant(
                    AuthoringPropertyOwner::Item(item_id),
                    "scale".to_string(),
                    vec2(73.5, 118.25),
                )
                .expect("fractional placement scale");
        }
        let render_scale = if fractional { 0.625 } else { 1.0 };
        let before = service.snapshot().expect("direct Project");
        let direct_pixels =
            rendered_pixels_at_scale(&before, Arc::clone(&plugins), 0, render_scale);
        service
            .convert_source_to_node_clip(plugins.as_ref(), item_id)
            .expect("Appearance conversion");
        let converted = service.snapshot().expect("converted Project");
        assert_eq!(
            rendered_pixels_at_scale(&converted, Arc::clone(&plugins), 0, render_scale),
            direct_pixels,
            "{source_name} Gradient Overlay changed after conversion"
        );
    }
}
