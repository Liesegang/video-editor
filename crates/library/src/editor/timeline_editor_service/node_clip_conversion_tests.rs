use super::*;

use std::sync::Arc;

use ordered_float::OrderedFloat;

use crate::cache::CacheManager;
use crate::core::render_plan::{RenderPlanCompiler, evaluate_render_plan_frame};
use crate::editor::{
    AuthoringNodeFactory, ModuleNodeRequest, RenderDestination, RenderService,
    TextEnsembleOperationKind,
};
use crate::model::authoring::{ModuleDefinitionSharing, ModulePortAddress, ShapeKind, ShapeSource};
use crate::model::frame::draw_type::{PathEffect, TrimPathUnits};
use crate::model::frame::entity::{FrameContent, FrameItem, FrameObject};
use crate::model::node::{GeneratorContent, NodeContent};
use crate::model::project::asset::{Asset, AssetKind};
use crate::model::project::property::{ColorValue, Property, Vec2};
use crate::model::project::{BACKGROUND_SHAPE_INPUT_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT};
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_renderer::SkiaRenderer;

fn time(seconds: i64) -> MediaTime {
    MediaTime::new(seconds, 1).expect("whole-second time")
}

fn interval(duration: i64) -> TimelineInterval {
    TimelineInterval::new(MediaTime::zero(), time(duration)).expect("positive interval")
}

fn color(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color { r, g, b, a }
}

fn vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn small_service(name: &str) -> (TimelineEditorService, TimelineTrackId) {
    let project =
        AuthoringProject::new(name, 96, 64, RationalRate::new(30, 1).unwrap(), time(4)).unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    (TimelineEditorService::new(project).unwrap(), track_id)
}

fn rendered_pixels(
    project: &AuthoringProject,
    plugins: Arc<PluginManager>,
    frame_number: u64,
) -> Vec<u8> {
    let timeline = &project.timelines[&project.root_timeline_id];
    let plan = RenderPlanCompiler::compile(project).expect("RenderPlan");
    let frame =
        evaluate_render_plan_frame(project, &plan, plugins.as_ref(), frame_number, 1.0, None)
            .expect("evaluated frame");
    assert!(contains_visible_content(&frame.items));
    let cache = Arc::new(CacheManager::new());
    let renderer = SkiaRenderer::new(
        u32::try_from(timeline.width).unwrap(),
        u32::try_from(timeline.height).unwrap(),
        timeline.background_color.clone(),
        false,
        None,
        Some(Arc::clone(&cache)),
    )
    .expect("CPU renderer");
    let mut render_service = RenderService::new(renderer, plugins, cache);
    let RenderOutput::Image(image) = render_service
        .render_authoring_frame(project, &frame, RenderDestination::Preview)
        .expect("authoring frame")
    else {
        panic!("Preview must be an Image");
    };
    image.data
}

fn contains_visible_content(items: &[FrameItem]) -> bool {
    items.iter().any(|item| match item {
        FrameItem::Object(_) => true,
        FrameItem::Group(group) => contains_visible_content(&group.items),
        FrameItem::Transition(transition) => {
            contains_visible_content(std::slice::from_ref(&transition.from.item))
                || contains_visible_content(std::slice::from_ref(&transition.to.item))
        }
    })
}

struct ShapeFrameView<'a> {
    object: &'a FrameObject,
    path: &'a str,
    ensemble: Option<&'a crate::core::ensemble::EnsembleData>,
    path_effects: &'a [PathEffect],
    transform: &'a crate::model::frame::transform::Transform,
}

fn first_shape(items: &[FrameItem]) -> Option<ShapeFrameView<'_>> {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                if let FrameContent::Shape {
                    path,
                    ensemble,
                    path_effects,
                    transform,
                    ..
                } = &object.content
                {
                    return Some(ShapeFrameView {
                        object,
                        path,
                        ensemble: ensemble.as_ref(),
                        path_effects,
                        transform,
                    });
                }
            }
            FrameItem::Group(group) => {
                if let Some(shape) = first_shape(&group.items) {
                    return Some(shape);
                }
            }
            FrameItem::Transition(transition) => {
                if let Some(shape) = first_shape(std::slice::from_ref(&transition.from.item))
                    .or_else(|| first_shape(std::slice::from_ref(&transition.to.item)))
                {
                    return Some(shape);
                }
            }
        }
    }
    None
}

#[test]
fn text_source_ensemble_and_pre_effect_convert_with_pixel_parity_and_one_undo() {
    let plugins = Arc::new(PluginManager::default());
    let (service, track_id) = small_service("Text Node Clip conversion");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Title".to_string(),
            SourceRef::Text {
                text: "Node Clip".to_string(),
                ensemble_operations: Vec::new(),
            },
            interval(3),
            0,
        )
        .unwrap();
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(item_id),
            "font_family".to_string(),
            PropertyValue::String("Arial".to_string()),
        )
        .unwrap();
    let (source_size_key_a, _) = service
        .upsert_authored_property_keyframe(
            AuthoringPropertyOwner::Item(item_id),
            "size".to_string(),
            MediaTime::zero(),
            PropertyValue::from(18.0),
            None,
        )
        .unwrap();
    let (source_size_key_b, _) = service
        .upsert_authored_property_keyframe(
            AuthoringPropertyOwner::Item(item_id),
            "size".to_string(),
            time(1),
            PropertyValue::from(24.0),
            Some(EasingFunction::EaseInOutQuad),
        )
        .unwrap();
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(item_id),
            "color".to_string(),
            PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color(245, 120, 30, 255))),
        )
        .unwrap();
    let (placement_key_a, _) = service
        .upsert_authored_property_keyframe(
            AuthoringPropertyOwner::Item(item_id),
            "position".to_string(),
            MediaTime::zero(),
            vec2(4.0, 6.0),
            None,
        )
        .unwrap();
    let (placement_key_b, _) = service
        .upsert_authored_property_keyframe(
            AuthoringPropertyOwner::Item(item_id),
            "position".to_string(),
            time(1),
            vec2(10.0, 8.0),
            None,
        )
        .unwrap();
    let (ensemble_id, _) = service
        .add_text_ensemble_operation_by_id(
            plugins.as_ref(),
            item_id,
            TextEnsembleOperationKind::Effector,
            "opacity",
        )
        .unwrap();
    service
        .set_text_ensemble_property(
            plugins.as_ref(),
            item_id,
            ensemble_id,
            "opacity",
            MediaTime::zero(),
            PropertyValue::from(80.0),
        )
        .unwrap();
    let (backplate_id, _) = service
        .add_text_ensemble_operation_by_id(
            plugins.as_ref(),
            item_id,
            TextEnsembleOperationKind::Decorator,
            "backplate",
        )
        .unwrap();
    for (key, value) in [
        ("target", PropertyValue::String("Line".to_string())),
        ("shape", PropertyValue::String("RoundRect".to_string())),
        (
            "color",
            PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color(15, 35, 80, 220))),
        ),
        ("padding", PropertyValue::from(4.0)),
        ("radius", PropertyValue::from(3.0)),
    ] {
        service
            .set_text_ensemble_property(
                plugins.as_ref(),
                item_id,
                backplate_id,
                key,
                MediaTime::zero(),
                value,
            )
            .unwrap();
    }
    let owner = AttachmentOwner::Item { item_id };
    let (pre_effect_id, _) = service
        .add_builtin_effect_by_id(
            plugins.as_ref(),
            owner.clone(),
            AttachmentStage::ItemPreTransform,
            "blur",
        )
        .unwrap();
    service
        .set_builtin_effect_parameter(pre_effect_id, "sigma_x", PropertyValue::from(1.5))
        .unwrap();
    let (post_effect_id, _) = service
        .add_builtin_effect_by_id(
            plugins.as_ref(),
            owner,
            AttachmentStage::ItemPostTransform,
            "blur",
        )
        .unwrap();

    let before = service.snapshot().unwrap();
    let before_item = before.items[&item_id].clone();
    let before_pixels = rendered_pixels(&before, Arc::clone(&plugins), 15);
    let conversion = service
        .convert_source_to_node_clip(plugins.as_ref(), item_id)
        .unwrap();

    assert_eq!(conversion.moved_pre_transform_effects, 1);
    assert_eq!(conversion.retained_post_transform_effects, 1);
    let after = service.snapshot().unwrap();
    let after_item = &after.items[&item_id];
    assert_eq!(after_item.id, before_item.id);
    assert_eq!(after_item.track_id, before_item.track_id);
    assert_eq!(after_item.interval, before_item.interval);
    assert_eq!(after_item.time_map, before_item.time_map);
    assert_eq!(after_item.layer, before_item.layer);
    assert_eq!(after_item.parent, before_item.parent);
    assert_eq!(after_item.blend_mode, before_item.blend_mode);
    assert!(after_item.authored_properties.get("font_family").is_none());
    assert!(after_item.authored_properties.get("size").is_none());
    assert!(after_item.authored_properties.get("color").is_none());
    let placement = after_item.authored_properties.get("position").unwrap();
    assert_eq!(
        placement
            .keyframes()
            .into_iter()
            .map(|key| key.id)
            .collect::<Vec<_>>(),
        vec![placement_key_a, placement_key_b]
    );
    let SourceRef::Module(invocation) = &after_item.source else {
        panic!("converted source must be a Module invocation");
    };
    assert_eq!(invocation.instance_id, conversion.instance_id);
    let moved_source_keys = invocation
        .automation_tracks
        .values()
        .flat_map(|track| track.keyframes.iter().map(|key| key.id))
        .collect::<Vec<_>>();
    assert!(moved_source_keys.contains(&source_size_key_a));
    assert!(moved_source_keys.contains(&source_size_key_b));
    assert!(!after.attachments.contains_key(&pre_effect_id));
    assert_eq!(
        after.attachments[&post_effect_id].stage,
        AttachmentStage::ItemPostTransform
    );
    let definition = &after.module_definitions[&conversion.definition_id];
    assert_eq!(definition.sharing, ModuleDefinitionSharing::Private);
    assert!(definition.graph.nodes.contains_key(&ensemble_id));
    assert!(definition.graph.nodes.contains_key(&backplate_id));
    assert!(
        definition
            .graph
            .nodes
            .contains_key(&pre_effect_id.as_uuid())
    );
    assert!(definition.graph.nodes.values().any(|node| matches!(
        node.content(),
        NodeContent::Generator(GeneratorContent::Text)
    )));
    assert!(
        definition
            .output(conversion.output_id)
            .is_some_and(|output| output.supports(crate::model::project::PortDataType::Image))
    );
    let after_pixels = rendered_pixels(&after, Arc::clone(&plugins), 15);
    assert_eq!(
        after_pixels, before_pixels,
        "conversion changed Preview pixels"
    );

    service.undo().unwrap().expect("one conversion transaction");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn shape_and_image_sources_keep_raster_parity() {
    let plugins = Arc::new(PluginManager::default());
    let (shape_service, shape_track_id) = small_service("Shape conversion");
    let (shape_id, _) = shape_service
        .add_item(
            shape_track_id,
            "Rectangle".to_string(),
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::from([
                        ("width".to_string(), PropertyValue::from(28.0)),
                        ("height".to_string(), PropertyValue::from(20.0)),
                        (
                            "color".to_string(),
                            PropertyValue::Color(color(40, 180, 220, 255)),
                        ),
                    ]),
                },
            },
            interval(2),
            0,
        )
        .unwrap();
    let shape_before = shape_service.snapshot().unwrap();
    let shape_pixels = rendered_pixels(&shape_before, Arc::clone(&plugins), 0);
    shape_service
        .convert_source_to_node_clip(plugins.as_ref(), shape_id)
        .unwrap();
    let shape_after = shape_service.snapshot().unwrap();
    assert_eq!(
        rendered_pixels(&shape_after, Arc::clone(&plugins), 0),
        shape_pixels
    );

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.png");
    let mut pixels = image::RgbaImage::new(12, 8);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        *pixel = image::Rgba([20 + x as u8 * 8, 40 + y as u8 * 12, 170, 255]);
    }
    pixels.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut project = AuthoringProject::new(
        "Image conversion",
        96,
        64,
        RationalRate::new(30, 1).unwrap(),
        time(3),
    )
    .unwrap();
    let image_track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let mut asset = Asset::new("source.png", &path.to_string_lossy(), AssetKind::Image);
    asset.width = Some(12);
    asset.height = Some(8);
    asset.verify_imported_content(&bytes);
    let asset_id = asset.id;
    project.assets.push(asset);
    let image_service = TimelineEditorService::new(project).unwrap();
    let (image_id, _) = image_service
        .add_item(
            image_track_id,
            "Still".to_string(),
            SourceRef::Asset { asset_id },
            interval(2),
            0,
        )
        .unwrap();
    let image_before = image_service.snapshot().unwrap();
    let image_pixels = rendered_pixels(&image_before, Arc::clone(&plugins), 0);
    image_service
        .convert_source_to_node_clip(plugins.as_ref(), image_id)
        .unwrap();
    let image_after = image_service.snapshot().unwrap();
    assert_eq!(
        rendered_pixels(&image_after, Arc::clone(&plugins), 0),
        image_pixels
    );
}

#[test]
fn node_clip_shape_catalog_operations_execute_path_effect_and_xy_transform() {
    let plugins = Arc::new(PluginManager::default());
    let (service, track_id) = small_service("Shape operation runtime");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Rectangle".to_string(),
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::from([
                        ("width".to_string(), PropertyValue::from(28.0)),
                        ("height".to_string(), PropertyValue::from(20.0)),
                        (
                            "color".to_string(),
                            PropertyValue::Color(color(40, 180, 220, 255)),
                        ),
                    ]),
                },
            },
            interval(2),
            0,
        )
        .unwrap();
    let conversion = service
        .convert_source_to_node_clip(plugins.as_ref(), item_id)
        .unwrap();

    let project = service.snapshot().unwrap();
    let definition = &project.module_definitions[&conversion.definition_id];
    let source_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Shape)
            )
        })
        .unwrap()
        .id;
    let fill_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation)
                    if operation.category == crate::plugin::STYLE_CATEGORY
                        && operation.component_id == "fill"
            )
        })
        .unwrap()
        .id;
    let source_to_fill = definition
        .graph
        .connections
        .iter()
        .find(|connection| connection.from.node_id == source_id && connection.to.node_id == fill_id)
        .unwrap()
        .id;
    drop(project);

    service
        .disconnect_instance_module_connection(conversion.instance_id, source_to_fill)
        .unwrap();
    let path_node = plugins.create_path_effect_operation_node("trim").unwrap();
    let path_id = path_node.id;
    service
        .add_instance_module_node(conversion.instance_id, path_node)
        .unwrap();
    let transform_node = plugins.create_shape_transform_operation_node().unwrap();
    let transform_id = transform_node.id;
    service
        .add_instance_module_node(conversion.instance_id, transform_node)
        .unwrap();
    service
        .set_instance_module_node_property(
            conversion.instance_id,
            path_id,
            "end".to_string(),
            Property::constant(PropertyValue::from(0.5)),
        )
        .unwrap();
    for (key, value) in [
        ("position", vec2(11.0, -7.0)),
        ("scale", vec2(250.0, 40.0)),
        ("anchor", vec2(3.0, 5.0)),
        ("rotation", PropertyValue::from(23.0)),
    ] {
        service
            .set_instance_module_node_property(
                conversion.instance_id,
                transform_id,
                key.to_string(),
                Property::constant(value),
            )
            .unwrap();
    }
    for (from, to) in [
        (
            ModulePortAddress {
                node_id: source_id,
                port: SHAPE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: path_id,
                port: SHAPE_INPUT_PORT.to_string(),
            },
        ),
        (
            ModulePortAddress {
                node_id: path_id,
                port: SHAPE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: transform_id,
                port: SHAPE_INPUT_PORT.to_string(),
            },
        ),
        (
            ModulePortAddress {
                node_id: transform_id,
                port: SHAPE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: fill_id,
                port: SHAPE_INPUT_PORT.to_string(),
            },
        ),
    ] {
        service
            .connect_instance_module_ports(conversion.instance_id, from, to, 0)
            .unwrap();
    }

    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame =
        evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 0, 1.0, None).unwrap();
    let shape = first_shape(&frame.items).expect("Fill must rasterize the transformed Shape");
    assert_eq!(shape.object.spatial_transform_node_id, Some(transform_id));
    assert_eq!(
        shape.path_effects,
        &[PathEffect::Trim {
            start: 0.0,
            end: 0.5,
            offset: 0.0,
            units: TrimPathUnits::Normalized,
        }]
    );
    assert_eq!(
        (shape.transform.position.x, shape.transform.position.y),
        (11.0, -7.0)
    );
    assert_eq!(
        (shape.transform.scale.x, shape.transform.scale.y),
        (2.5, 0.4)
    );
    assert_eq!(
        (shape.transform.anchor.x, shape.transform.anchor.y),
        (3.0, 5.0)
    );
    assert_eq!(shape.transform.rotation, 23.0);
}

#[test]
fn node_clip_graph_backplate_consumes_its_background_shape_input() {
    let plugins = Arc::new(PluginManager::default());
    let (service, track_id) = small_service("Graph Backplate runtime");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Title".to_string(),
            SourceRef::Text {
                text: "Backplate".to_string(),
                ensemble_operations: Vec::new(),
            },
            interval(2),
            0,
        )
        .unwrap();
    let conversion = service
        .convert_source_to_node_clip(plugins.as_ref(), item_id)
        .unwrap();

    let project = service.snapshot().unwrap();
    let definition = &project.module_definitions[&conversion.definition_id];
    let text_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Text)
            )
        })
        .unwrap()
        .id;
    let fill_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation)
                    if operation.category == crate::plugin::STYLE_CATEGORY
                        && operation.component_id == "fill"
            )
        })
        .unwrap()
        .id;
    let text_to_fill = definition
        .graph
        .connections
        .iter()
        .find(|connection| connection.from.node_id == text_id && connection.to.node_id == fill_id)
        .unwrap()
        .id;
    drop(project);

    service
        .disconnect_instance_module_connection(conversion.instance_id, text_to_fill)
        .unwrap();
    let background = AuthoringNodeFactory::create(
        plugins.as_ref(),
        ModuleNodeRequest::Shape {
            path: "M 0 0 H 10 V 10 H 0 Z".to_string(),
            width: 10,
            height: 10,
        },
        96,
        64,
    )
    .unwrap();
    let background_id = background.id;
    service
        .add_instance_module_node(conversion.instance_id, background)
        .unwrap();
    let backplate = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    let backplate_id = backplate.id;
    service
        .add_instance_module_node(conversion.instance_id, backplate)
        .unwrap();
    for (from, to) in [
        (
            ModulePortAddress {
                node_id: text_id,
                port: SHAPE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: backplate_id,
                port: SHAPE_INPUT_PORT.to_string(),
            },
        ),
        (
            ModulePortAddress {
                node_id: background_id,
                port: SHAPE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: backplate_id,
                port: BACKGROUND_SHAPE_INPUT_PORT.to_string(),
            },
        ),
        (
            ModulePortAddress {
                node_id: backplate_id,
                port: SHAPE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: fill_id,
                port: SHAPE_INPUT_PORT.to_string(),
            },
        ),
    ] {
        service
            .connect_instance_module_ports(conversion.instance_id, from, to, 0)
            .unwrap();
    }

    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame =
        evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 0, 1.0, None).unwrap();
    let shape = first_shape(&frame.items).expect("Backplate must produce fitted Shape geometry");
    assert_eq!(shape.object.source_node_id, backplate_id);
    assert!(!shape.path.is_empty());
    assert!(
        shape.ensemble.is_none(),
        "graph Backplate must not use paint-time fallback"
    );
    assert!(shape.path_effects.is_empty());
}

#[test]
fn unsupported_source_or_processor_fails_without_any_project_mutation() {
    let plugins = PluginManager::default();
    let (service, track_id) = small_service("Atomic refusal");
    let (composition_id, _composition_track_id, _) = service
        .add_timeline(
            "Nested".to_string(),
            96,
            64,
            RationalRate::new(30, 1).unwrap(),
            time(2),
        )
        .unwrap();
    let (item_id, _) = service
        .add_item(
            track_id,
            "Nested".to_string(),
            SourceRef::Composition(crate::model::authoring::CompositionInstance {
                timeline_id: composition_id,
                duration_policy: crate::model::authoring::DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            interval(2),
            0,
        )
        .unwrap();
    let before = service.snapshot().unwrap();
    let revision = service.revision().unwrap();

    let error = service
        .convert_source_to_node_clip(&plugins, item_id)
        .unwrap_err();

    assert!(error.to_string().contains("Nested Timeline"));
    assert_eq!(service.revision().unwrap(), revision);
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());

    let (processor_service, processor_track_id) = small_service("Atomic processor refusal");
    let (solid_id, _) = processor_service
        .add_item(
            processor_track_id,
            "Solid".to_string(),
            SourceRef::Solid {
                color: color(20, 40, 60, 255),
            },
            interval(2),
            0,
        )
        .unwrap();
    let (effect_id, _) = processor_service
        .add_builtin_effect_by_id(
            &plugins,
            AttachmentOwner::Item { item_id: solid_id },
            AttachmentStage::ItemPreTransform,
            "blur",
        )
        .unwrap();
    processor_service
        .set_attachment_state(effect_id, false, false)
        .unwrap();
    let processor_before = processor_service.snapshot().unwrap();
    let processor_revision = processor_service.revision().unwrap();

    let error = processor_service
        .convert_source_to_node_clip(&plugins, solid_id)
        .unwrap_err();

    assert!(error.to_string().contains("Disabled pre-Transform Effect"));
    assert_eq!(processor_service.revision().unwrap(), processor_revision);
    assert_eq!(
        processor_service.snapshot().unwrap().as_ref(),
        processor_before.as_ref()
    );
}
