use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use library::SkiaRenderer;
use library::cache::CacheManager;
use library::core::ensemble::EnsembleData;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::draw_type::{CapType, DrawStyle, JoinType};
use library::model::frame::entity::{
    FrameContent, FrameGroup, FrameGroupKind, FrameItem, FrameObject, StyleConfig,
};
use library::model::frame::frame::FrameInfo;
use library::model::frame::transform::{Position, Scale, Transform};
use library::model::property::PropertyMap;
use library::plugin::PluginManager;
use library::plugin::entity_converter::{measure_shape_visual_bounds, measure_text_size};
use library::rendering::renderer::{
    Affine2D, RenderOutput, Renderer, ShapeRasterRequest, TextRasterRequest,
};
use library::rendering::text_layout::text_style_outset;
use library::{RenderService, model::BlendMode};
use ordered_float::OrderedFloat;
use uuid::Uuid;

const WIDTH: u32 = 360;
const HEIGHT: u32 = 260;

fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

fn fill(color: Color, offset: f64) -> StyleConfig {
    StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill { color, offset },
    }
}

fn stroke(color: Color, width: f64, offset: f64) -> StyleConfig {
    StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Stroke {
            color,
            width,
            offset,
            cap: CapType::Round,
            join: JoinType::Round,
            miter: 4.0,
            dash_array: Vec::new(),
            dash_offset: 0.0,
        },
    }
}

fn cpu_renderer() -> SkiaRenderer {
    SkiaRenderer::new(WIDTH, HEIGHT, transparent(), false, None, None).unwrap()
}

fn image(output: RenderOutput) -> Image {
    match output {
        RenderOutput::Image(image) => image,
        RenderOutput::Texture(_) => panic!("CPU renderer unexpectedly returned a texture"),
    }
}

fn alpha_bounds(image: &Image) -> Option<(i32, i32, i32, i32)> {
    let mut left = i32::MAX;
    let mut top = i32::MAX;
    let mut right = i32::MIN;
    let mut bottom = i32::MIN;
    for (index, pixel) in image.data.chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let x = index as i32 % image.width as i32;
        let y = index as i32 / image.width as i32;
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + 1);
        bottom = bottom.max(y + 1);
    }
    (left != i32::MAX).then_some((left, top, right, bottom))
}

fn assert_clean_transparency(image: &Image) {
    assert_eq!(
        image.data.len(),
        image.width as usize * image.height as usize * 4
    );
    let mut visible = 0_usize;
    for pixel in image.data.chunks_exact(4) {
        if pixel[3] == 0 {
            assert_eq!(pixel, &[0, 0, 0, 0], "transparent RGB carried dirty color");
        } else {
            visible += 1;
        }
    }
    assert!(visible > 0, "the test layer did not paint any pixels");

    for (x, y) in [
        (0, 0),
        (image.width - 1, 0),
        (0, image.height - 1),
        (image.width - 1, image.height - 1),
    ] {
        let offset = ((y * image.width + x) * 4) as usize;
        assert_eq!(&image.data[offset..offset + 4], &[0, 0, 0, 0]);
    }
}

fn artifact_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/render-quality")
}

fn save_artifact(name: &str, image: &Image) {
    let directory = artifact_directory();
    fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        &image.data,
        image.width,
        image.height,
        image::ColorType::Rgba8,
    )
    .unwrap();

    let mut checker = Vec::with_capacity(image.data.len());
    for (index, pixel) in image.data.chunks_exact(4).enumerate() {
        let x = index as u32 % image.width;
        let y = index as u32 / image.width;
        let background = if (x / 12 + y / 12).is_multiple_of(2) {
            214_u8
        } else {
            164_u8
        };
        let alpha = u16::from(pixel[3]);
        for channel in &pixel[..3] {
            checker.push(
                ((u16::from(*channel) * alpha + u16::from(background) * (255 - alpha) + 127) / 255)
                    as u8,
            );
        }
        checker.push(255);
    }
    image::save_buffer(
        directory.join(format!("{name}-checker.png")),
        &checker,
        image.width,
        image.height,
        image::ColorType::Rgba8,
    )
    .unwrap();
}

fn map_local_bounds(bounds: (f32, f32, f32, f32), transform: &Transform) -> (f32, f32, f32, f32) {
    let (x, y, width, height) = bounds;
    let radians = (transform.rotation as f32).to_radians();
    let (sin, cos) = radians.sin_cos();
    let mut left = f32::INFINITY;
    let mut top = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    for (local_x, local_y) in [
        (x, y),
        (x + width, y),
        (x + width, y + height),
        (x, y + height),
    ] {
        let scaled_x = (local_x - transform.anchor.x as f32) * transform.scale.x as f32;
        let scaled_y = (local_y - transform.anchor.y as f32) * transform.scale.y as f32;
        let world_x = transform.position.x as f32 + scaled_x * cos - scaled_y * sin;
        let world_y = transform.position.y as f32 + scaled_x * sin + scaled_y * cos;
        left = left.min(world_x);
        top = top.min(world_y);
        right = right.max(world_x);
        bottom = bottom.max(world_y);
    }
    (left, top, right, bottom)
}

fn assert_pixels_inside_selection(
    actual: (i32, i32, i32, i32),
    selection: (f32, f32, f32, f32),
    tolerance: f32,
) {
    let (actual_left, actual_top, actual_right, actual_bottom) = actual;
    let (left, top, right, bottom) = selection;
    assert!(
        actual_left as f32 >= left - tolerance,
        "{actual:?} vs {selection:?}"
    );
    assert!(
        actual_top as f32 >= top - tolerance,
        "{actual:?} vs {selection:?}"
    );
    assert!(
        actual_right as f32 <= right + tolerance,
        "{actual:?} vs {selection:?}"
    );
    assert!(
        actual_bottom as f32 <= bottom + tolerance,
        "{actual:?} vs {selection:?}"
    );
}

fn assert_bounds_close(first: (i32, i32, i32, i32), second: (i32, i32, i32, i32), tolerance: i32) {
    for (first, second) in [
        (first.0, second.0),
        (first.1, second.1),
        (first.2, second.2),
        (first.3, second.3),
    ] {
        assert!(
            (first - second).abs() <= tolerance,
            "bounds differ: {first:?} vs {second:?}"
        );
    }
}

fn alpha_edge_energy(image: &Image) -> u64 {
    let alpha = |x: u32, y: u32| image.data[((y * image.width + x) * 4 + 3) as usize];
    let mut energy = 0_u64;
    for y in 0..image.height {
        for x in 0..image.width {
            let value = alpha(x, y);
            if x > 0 {
                energy += u64::from(value.abs_diff(alpha(x - 1, y)));
            }
            if y > 0 {
                energy += u64::from(value.abs_diff(alpha(x, y - 1)));
            }
        }
    }
    energy
}

fn transition_pixel_count(image: &Image) -> usize {
    image
        .data
        .chunks_exact(4)
        .filter(|pixel| (8..=247).contains(&pixel[3]))
        .count()
}

fn mean_alpha_difference(first: &Image, second: &Image) -> f64 {
    assert_eq!((first.width, first.height), (second.width, second.height));
    let difference = first
        .data
        .chunks_exact(4)
        .zip(second.data.chunks_exact(4))
        .map(|(first, second)| u64::from(first[3].abs_diff(second[3])))
        .sum::<u64>();
    difference as f64 / (first.width * first.height) as f64
}

fn frame(items: Vec<FrameItem>) -> FrameInfo {
    FrameInfo {
        width: u64::from(WIDTH),
        height: u64::from(HEIGHT),
        background_color: transparent(),
        color_profile: "sRGB".to_string(),
        render_scale: OrderedFloat(1.0),
        now_time: OrderedFloat(0.0),
        region: None,
        items,
    }
}

fn render_frame(frame: &FrameInfo) -> Image {
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )
    .unwrap();
    let plugins = Arc::new(PluginManager::default());
    let mut service = RenderService::new(renderer, plugins, Arc::new(CacheManager::new()));
    image(service.render_from_frame_info(frame).unwrap())
}

fn vector_object(is_text: bool, transform: Transform, styles: &[StyleConfig]) -> FrameItem {
    let content = if is_text {
        FrameContent::Text {
            text: "Ag".to_string(),
            font: "Arial".to_string(),
            size: 24.0,
            styles: styles.to_vec(),
            effects: Vec::new(),
            ensemble: None,
            transform,
        }
    } else {
        FrameContent::Shape {
            path: "M 1 2 L 43 5 L 38 29 L 4 26 Z".to_string(),
            styles: styles.to_vec(),
            path_effects: Vec::new(),
            effects: Vec::new(),
            ensemble: None,
            transform,
        }
    };
    FrameItem::Object(FrameObject {
        source_node_id: Uuid::new_v4(),
        content_bounds: None,
        content,
        properties: PropertyMap::new(),
    })
}

fn group(kind: FrameGroupKind, transform: Transform, items: Vec<FrameItem>) -> FrameItem {
    FrameItem::Group(FrameGroup {
        source_id: Uuid::new_v4(),
        kind,
        width: u64::from(WIDTH),
        height: u64::from(HEIGHT),
        background_color: transparent(),
        transform,
        blend_mode: BlendMode::Normal,
        effect_time: OrderedFloat(0.0),
        effects: Vec::new(),
        items,
    })
}

fn scaled_transform(scale: f64, position: (f64, f64)) -> Transform {
    Transform {
        position: Position {
            x: position.0,
            y: position.1,
        },
        scale: Scale { x: scale, y: scale },
        anchor: Position { x: 0.0, y: 0.0 },
        rotation: 0.0,
        opacity: 1.0,
    }
}

#[test]
fn transparent_text_and_shape_layers_use_clean_straight_rgba() {
    let text_styles = vec![
        fill(
            Color {
                r: 240,
                g: 80,
                b: 20,
                a: 128,
            },
            0.0,
        ),
        stroke(
            Color {
                r: 20,
                g: 70,
                b: 240,
                a: 180,
            },
            3.0,
            0.0,
        ),
    ];
    let text_transform = Transform {
        position: Position { x: 42.0, y: 35.0 },
        ..Transform::default()
    };
    let mut renderer = cpu_renderer();
    let text = image(
        renderer
            .rasterize_text_layer(TextRasterRequest {
                text: "Ag\nTy",
                size: 52.0,
                font_name: "Arial",
                styles: &text_styles,
                ensemble: None,
                transform: Affine2D::from(&text_transform),
                current_time: 0.0,
            })
            .unwrap(),
    );
    assert_clean_transparency(&text);
    save_artifact("text-standard-transparent", &text);

    let shape_styles = vec![
        fill(
            Color {
                r: 240,
                g: 80,
                b: 20,
                a: 128,
            },
            0.0,
        ),
        stroke(
            Color {
                r: 30,
                g: 80,
                b: 245,
                a: 192,
            },
            4.0,
            0.0,
        ),
    ];
    let shape_transform = Transform {
        position: Position { x: 95.0, y: 100.0 },
        rotation: -17.0,
        scale: Scale { x: 1.2, y: 0.9 },
        anchor: Position { x: 52.0, y: 37.0 },
        ..Transform::default()
    };
    let mut renderer = cpu_renderer();
    let shape = image(
        renderer
            .rasterize_shape_layer(ShapeRasterRequest {
                path_data: "M 12 12 L 92 12 L 92 62 L 12 62 Z",
                styles: &shape_styles,
                path_effects: &[],
                ensemble: None,
                transform: Affine2D::from(&shape_transform),
            })
            .unwrap(),
    );
    assert_clean_transparency(&shape);
    assert!(shape.data.chunks_exact(4).any(|pixel| {
        pixel[3] == 128 && pixel[0].abs_diff(240) <= 2 && pixel[1].abs_diff(80) <= 2
    }));
    save_artifact("shape-transparent", &shape);
}

#[test]
fn standard_and_ensemble_multiline_text_share_selection_metrics() {
    let styles = vec![
        fill(Color::white(), 0.0),
        stroke(
            Color {
                r: 20,
                g: 120,
                b: 255,
                a: 255,
            },
            4.0,
            0.0,
        ),
    ];
    let (width, height) = measure_text_size("Ag\nTy", "Arial", 52.0);
    let outset = text_style_outset(&styles);
    let local_bounds = (
        -outset,
        -outset,
        width + outset * 2.0,
        height + outset * 2.0,
    );
    let transform = Transform {
        position: Position { x: 170.0, y: 125.0 },
        scale: Scale { x: 1.35, y: 0.82 },
        anchor: Position {
            x: width as f64 / 2.0,
            y: height as f64 / 2.0,
        },
        rotation: 23.0,
        opacity: 1.0,
    };
    let selection = map_local_bounds(local_bounds, &transform);

    let mut renderer = cpu_renderer();
    let standard = image(
        renderer
            .rasterize_text_layer(TextRasterRequest {
                text: "Ag\nTy",
                size: 52.0,
                font_name: "Arial",
                styles: &styles,
                ensemble: None,
                transform: Affine2D::from(&transform),
                current_time: 0.0,
            })
            .unwrap(),
    );

    let mut ensemble = EnsembleData::default();
    ensemble.enabled = true;
    let mut renderer = cpu_renderer();
    let ensemble_image = image(
        renderer
            .rasterize_text_layer(TextRasterRequest {
                text: "Ag\nTy",
                size: 52.0,
                font_name: "Arial",
                styles: &styles,
                ensemble: Some(&ensemble),
                transform: Affine2D::from(&transform),
                current_time: 0.0,
            })
            .unwrap(),
    );

    assert_clean_transparency(&standard);
    assert_clean_transparency(&ensemble_image);
    let standard_bounds = alpha_bounds(&standard).unwrap();
    let ensemble_bounds = alpha_bounds(&ensemble_image).unwrap();
    assert_pixels_inside_selection(standard_bounds, selection, 2.0);
    assert_pixels_inside_selection(ensemble_bounds, selection, 2.0);
    assert_bounds_close(standard_bounds, ensemble_bounds, 3);
    save_artifact("text-standard-transformed", &standard);
    save_artifact("text-ensemble-transformed", &ensemble_image);
}

#[test]
fn transformed_shape_pixels_fit_stroke_aware_selection_bounds() {
    let styles = vec![
        fill(
            Color {
                r: 255,
                g: 190,
                b: 30,
                a: 255,
            },
            3.0,
        ),
        stroke(
            Color {
                r: 40,
                g: 100,
                b: 255,
                a: 255,
            },
            8.0,
            2.0,
        ),
    ];
    let path = "M 12 12 L 92 12 L 92 62 L 12 62 Z";
    let local_bounds = measure_shape_visual_bounds(path, &styles, &[]).unwrap();
    assert_eq!(local_bounds, (6.0, 6.0, 92.0, 62.0));
    let transform = Transform {
        position: Position { x: 180.0, y: 130.0 },
        scale: Scale { x: 1.45, y: 0.78 },
        anchor: Position { x: 52.0, y: 37.0 },
        rotation: -28.0,
        opacity: 1.0,
    };
    let selection = map_local_bounds(local_bounds, &transform);

    let mut renderer = cpu_renderer();
    let rendered = image(
        renderer
            .rasterize_shape_layer(ShapeRasterRequest {
                path_data: path,
                styles: &styles,
                path_effects: &[],
                ensemble: None,
                transform: Affine2D::from(&transform),
            })
            .unwrap(),
    );
    assert_clean_transparency(&rendered);
    assert_pixels_inside_selection(alpha_bounds(&rendered).unwrap(), selection, 2.0);
    save_artifact("shape-stroke-bounds-transformed", &rendered);
}

#[test]
fn track_and_clip_2x_4x_vector_transforms_match_direct_parent_raster_quality() {
    let styles = vec![
        fill(Color::white(), 0.0),
        stroke(
            Color {
                r: 25,
                g: 120,
                b: 255,
                a: 255,
            },
            1.5,
            0.0,
        ),
    ];

    for container_kind in [FrameGroupKind::Track, FrameGroupKind::Clip] {
        for scale in [2.0_f64, 4.0] {
            for is_text in [true, false] {
                let local_position = (12.0, 10.0);
                let child = vector_object(is_text, scaled_transform(1.0, local_position), &styles);
                let container = group(
                    container_kind,
                    scaled_transform(scale, (0.0, 0.0)),
                    vec![child],
                );
                let through_container = render_frame(&frame(vec![container]));

                let direct = vector_object(
                    is_text,
                    scaled_transform(scale, (local_position.0 * scale, local_position.1 * scale)),
                    &styles,
                );
                let direct = render_frame(&frame(vec![direct]));

                assert_clean_transparency(&through_container);
                assert_clean_transparency(&direct);
                let container_bounds = alpha_bounds(&through_container).unwrap();
                let direct_bounds = alpha_bounds(&direct).unwrap();
                assert_bounds_close(container_bounds, direct_bounds, 1);
                assert!(
                    mean_alpha_difference(&through_container, &direct) <= 0.05,
                    "{container_kind:?} {scale}x {} accumulated raster blur",
                    if is_text { "Text" } else { "Shape" }
                );
                let container_energy = alpha_edge_energy(&through_container) as f64;
                let direct_energy = alpha_edge_energy(&direct) as f64;
                let relative_energy_difference =
                    (container_energy - direct_energy).abs() / direct_energy.max(1.0);
                assert!(
                    relative_energy_difference <= 0.005,
                    "{container_kind:?} {scale}x {} edge energy drifted by {:.3}%",
                    if is_text { "Text" } else { "Shape" },
                    relative_energy_difference * 100.0
                );

                if scale == 4.0 {
                    let kind = format!("{container_kind:?}").to_lowercase();
                    let content = if is_text { "text" } else { "shape" };
                    save_artifact(
                        &format!("{kind}-{content}-4x-final-resolution"),
                        &through_container,
                    );
                    save_artifact(&format!("{kind}-{content}-4x-direct-reference"), &direct);
                }
            }
        }
    }
}

#[test]
fn isolated_track_opacity_uses_final_target_resolution_without_softening() {
    let styles = vec![fill(Color::white(), 0.0)];
    let local_position = (12.0, 10.0);
    let child = vector_object(false, scaled_transform(1.0, local_position), &styles);
    let mut container_transform = scaled_transform(4.0, (0.0, 0.0));
    container_transform.opacity = 0.65;
    let isolated = group(FrameGroupKind::Track, container_transform, vec![child]);
    let isolated = render_frame(&frame(vec![isolated]));

    let mut direct_transform =
        scaled_transform(4.0, (local_position.0 * 4.0, local_position.1 * 4.0));
    direct_transform.opacity = 0.65;
    let direct = render_frame(&frame(vec![vector_object(
        false,
        direct_transform,
        &styles,
    )]));

    assert_bounds_close(
        alpha_bounds(&isolated).unwrap(),
        alpha_bounds(&direct).unwrap(),
        1,
    );
    save_artifact("track-shape-4x-isolated-opacity", &isolated);
    save_artifact("track-shape-4x-direct-opacity-reference", &direct);
    let mean_difference = mean_alpha_difference(&isolated, &direct);
    assert!(
        mean_difference <= 0.01,
        "mean alpha difference was {mean_difference}"
    );
    let relative_energy_difference =
        (alpha_edge_energy(&isolated) as f64 - alpha_edge_energy(&direct) as f64).abs()
            / (alpha_edge_energy(&direct) as f64).max(1.0);
    assert!(relative_energy_difference <= 0.005);
}

#[test]
fn nested_composition_keeps_its_configured_resolution_raster_boundary() {
    let styles = vec![fill(Color::white(), 0.0)];
    let local_position = (4.0, 5.0);
    let child = vector_object(false, scaled_transform(1.0, local_position), &styles);
    let nested = FrameItem::Group(FrameGroup {
        source_id: Uuid::new_v4(),
        kind: FrameGroupKind::Composition,
        width: 64,
        height: 48,
        background_color: transparent(),
        transform: scaled_transform(4.0, (0.0, 0.0)),
        blend_mode: BlendMode::Normal,
        effect_time: OrderedFloat(0.0),
        effects: Vec::new(),
        items: vec![child],
    });
    let nested = render_frame(&frame(vec![nested]));

    let direct = vector_object(
        false,
        scaled_transform(4.0, (local_position.0 * 4.0, local_position.1 * 4.0)),
        &styles,
    );
    let direct = render_frame(&frame(vec![direct]));

    assert_clean_transparency(&nested);
    assert_clean_transparency(&direct);
    assert!(
        transition_pixel_count(&nested) > transition_pixel_count(&direct) + 100,
        "configured-resolution Composition no longer behaves as a raster boundary: nested={} direct={}",
        transition_pixel_count(&nested),
        transition_pixel_count(&direct)
    );
    assert!(
        mean_alpha_difference(&nested, &direct) > 0.1,
        "nested Composition unexpectedly matched direct vector pixels"
    );
    save_artifact("composition-configured-resolution-4x", &nested);
    save_artifact("composition-direct-vector-4x-reference", &direct);
}
