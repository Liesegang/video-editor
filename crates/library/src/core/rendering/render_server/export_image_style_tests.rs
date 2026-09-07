use super::*;

use std::collections::HashMap;

use crate::editor::AppearanceOperationFactory;
use crate::model::authoring::{AppearanceOperation, ShapeKind, ShapeSource};
use crate::model::property::{ColorValue, GradientValue, Property, PropertyValue};

fn color(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color { r, g, b, a }
}

fn style(plugins: &PluginManager, component: &str) -> AppearanceOperation {
    AppearanceOperationFactory::create(plugins, component)
        .unwrap_or_else(|error| panic!("cannot create {component}: {error}"))
}

fn set_number(operation: &mut AppearanceOperation, key: &str, value: f64) {
    operation.properties.set(
        key.to_string(),
        Property::constant(PropertyValue::from(value)),
    );
}

fn fill(plugins: &PluginManager) -> AppearanceOperation {
    let mut operation = style(plugins, "fill");
    operation.properties.set(
        "paint".to_string(),
        Property::constant(PropertyValue::Paint(crate::model::property::Paint::Solid(
            ColorValue::from_straight_srgba8(&color(30, 80, 220, 255)),
        ))),
    );
    operation
}

fn shadow(plugins: &PluginManager) -> AppearanceOperation {
    let mut operation = style(plugins, "drop_shadow");
    set_number(&mut operation, "angle", 120.0);
    set_number(&mut operation, "distance", 9.0);
    set_number(&mut operation, "size", 3.0);
    operation
}

fn color_overlay(plugins: &PluginManager) -> AppearanceOperation {
    let mut operation = style(plugins, "color_overlay");
    operation.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::ColorValue(ColorValue::from_straight_srgba8(
            &color(240, 35, 80, 255),
        ))),
    );
    set_number(&mut operation, "opacity", 0.72);
    operation
}

fn gradient_overlay(plugins: &PluginManager) -> AppearanceOperation {
    let mut operation = style(plugins, "gradient_overlay");
    operation.properties.set(
        "gradient".to_string(),
        Property::constant(PropertyValue::Gradient(GradientValue::default())),
    );
    set_number(&mut operation, "opacity", 0.65);
    operation
}

fn converted_style_project(
    plugins: &PluginManager,
    name: &str,
    appearance_operations: Vec<AppearanceOperation>,
) -> Arc<AuthoringProject> {
    let project = AuthoringProject::new(
        name,
        96,
        64,
        RationalRate::new(30, 1).unwrap(),
        MediaTime::new(2, 1).unwrap(),
    )
    .unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let service = TimelineEditorService::new(project).unwrap();
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
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(2, 1).unwrap()).unwrap(),
            0,
        )
        .unwrap();
    service
        .convert_source_to_node_clip(plugins, item_id)
        .expect("explicit Shape to Node Clip conversion");
    let project = service.snapshot().unwrap();
    assert!(
        matches!(project.items[&item_id].source, SourceRef::Module(_)),
        "fixture must render the converted production Node graph"
    );
    project.validate().unwrap();
    project
}

#[test]
fn converted_image_style_order_is_distinct_and_each_preview_matches_png() {
    let plugins = Arc::new(PluginManager::default());
    let shadow_then_overlays = converted_style_project(
        plugins.as_ref(),
        "Shadow then overlays",
        vec![
            fill(plugins.as_ref()),
            shadow(plugins.as_ref()),
            gradient_overlay(plugins.as_ref()),
            color_overlay(plugins.as_ref()),
        ],
    );
    let overlays_then_shadow = converted_style_project(
        plugins.as_ref(),
        "Overlays then shadow",
        vec![
            fill(plugins.as_ref()),
            color_overlay(plugins.as_ref()),
            gradient_overlay(plugins.as_ref()),
            shadow(plugins.as_ref()),
        ],
    );

    let first = assert_authoring_png_matches_preview(
        RenderServer::new_with_cpu_preview(Arc::clone(&plugins), Arc::new(CacheManager::new())),
        shadow_then_overlays,
        0,
    );
    let second = assert_authoring_png_matches_preview(
        RenderServer::new_with_cpu_preview(plugins, Arc::new(CacheManager::new())),
        overlays_then_shadow,
        0,
    );
    assert_ne!(
        first, second,
        "authored Shadow/GradientOverlay/ColorOverlay order must change the rendered image"
    );
}

#[test]
fn converted_fill_and_stroke_paints_match_png_export_without_flattening() {
    use crate::model::property::{Paint, PatternValue};

    let plugins = Arc::new(PluginManager::default());
    for component in ["fill", "stroke"] {
        let mut rendered = Vec::new();
        for paint in [
            Paint::Gradient(GradientValue::default()),
            Paint::Pattern(PatternValue::default()),
        ] {
            let mut operation = style(&plugins, component);
            operation.properties.set(
                "paint".to_string(),
                Property::constant(PropertyValue::Paint(paint)),
            );
            set_number(&mut operation, "opacity", 0.65);
            if component == "stroke" {
                set_number(&mut operation, "width", 8.0);
            }
            let project = converted_style_project(
                &plugins,
                "Spatial Paint export",
                vec![operation, shadow(&plugins)],
            );
            rendered.push(assert_authoring_png_matches_preview(
                RenderServer::new_with_cpu_preview(
                    Arc::clone(&plugins),
                    Arc::new(CacheManager::new()),
                ),
                project,
                0,
            ));
        }
        assert_ne!(
            rendered[0], rendered[1],
            "{component} Gradient and Pattern must remain distinct through Image styles and export"
        );
    }
}
