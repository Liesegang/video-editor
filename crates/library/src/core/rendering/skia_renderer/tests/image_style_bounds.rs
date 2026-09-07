use super::*;
use crate::model::frame::entity::FrameBounds;
use crate::model::frame::image_bounds::FrameImageBounds;
use crate::model::property::{GradientGeometry, GradientSpread, Vec2};
use crate::rendering::renderer::ImageStyleContext;
use ordered_float::OrderedFloat;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 32;

fn point(x: f64, y: f64) -> Vec2 {
    Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    }
}

fn opaque(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

fn source_image() -> Image {
    let mut pixels = vec![0; WIDTH as usize * HEIGHT as usize * 4];
    for y in 8..24 {
        for x in 20..44 {
            let offset = (y * WIDTH + x) as usize * 4;
            pixels[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Image::new(WIDTH, HEIGHT, pixels)
}

fn pixels(output: RenderOutput) -> Image {
    let RenderOutput::Image(image) = output else {
        panic!("unmanaged CPU style output must be encoded pixels");
    };
    image
}

fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let offset = (y * image.width + x) as usize * 4;
    image.data[offset..offset + 4]
        .try_into()
        .expect("RGBA pixel")
}

fn bounds(geometry: FrameBounds, content: FrameBounds, visual: FrameBounds) -> ImageStyleContext {
    ImageStyleContext {
        render_scale: 1.0,
        bounds: FrameImageBounds {
            geometry,
            content,
            visual,
        },
    }
}

#[test]
fn image_gradient_uses_supplied_ink_bounds_instead_of_the_surface_extent() {
    let mut renderer =
        SkiaRenderer::new(WIDTH, HEIGHT, transparent(), false, None, None).expect("CPU renderer");
    let style = StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::GradientOverlay {
            gradient: gradient_value(
                GradientGeometry::Linear {
                    start: point(0.0, 0.5),
                    end: point(1.0, 0.5),
                },
                GradientSpread::Pad,
                &[(0.0, opaque(255, 0, 0)), (1.0, opaque(0, 0, 255))],
            ),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
        },
    };
    let body = FrameBounds::new(20.0, 8.0, 24.0, 16.0);
    let image = pixels(
        renderer
            .apply_image_style(
                &RenderOutput::Image(source_image()),
                &style,
                bounds(body, body, body),
            )
            .expect("ink-relative Gradient Overlay"),
    );

    let left = pixel(&image, 20, 16);
    let right = pixel(&image, 43, 16);
    assert!(
        left[0] > 245 && left[2] < 64,
        "left ink edge must be the first Gradient stop, not a canvas-relative sample: {left:?}"
    );
    assert!(
        right[2] > 245 && right[0] < 64,
        "right ink edge must reach the last Gradient stop: {right:?}"
    );
}

#[test]
fn padded_visual_bounds_preserve_an_outward_style_before_later_transform() {
    let mut renderer =
        SkiaRenderer::new(WIDTH, HEIGHT, transparent(), false, None, None).expect("CPU renderer");
    let style = StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::DropShadow {
            color: opaque(255, 0, 0),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            angle: 0.0,
            distance: 8.0,
            spread: 0.0,
            size: 0.0,
        },
    };
    let body = FrameBounds::new(20.0, 8.0, 24.0, 16.0);
    let visual = FrameBounds::new(12.0, 8.0, 32.0, 16.0);
    let image = pixels(
        renderer
            .apply_image_style(
                &RenderOutput::Image(source_image()),
                &style,
                bounds(body, body, visual),
            )
            .expect("padded Drop Shadow"),
    );

    assert!(
        pixel(&image, 12, 16)[3] > 250,
        "leftward style pixels were clipped at the upstream body edge"
    );
    assert_eq!(pixel(&image, 4, 16), [0, 0, 0, 0]);
    assert_eq!(pixel(&image, 24, 16), [255, 255, 255, 255]);
}

#[test]
fn image_style_rejects_bounds_outside_the_allocated_padded_layer() {
    let mut renderer =
        SkiaRenderer::new(WIDTH, HEIGHT, transparent(), false, None, None).expect("CPU renderer");
    let style = StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::ColorOverlay {
            color: Color::white(),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
        },
    };
    let body = FrameBounds::new(20.0, 8.0, 24.0, 16.0);
    let external_geometry = FrameBounds::new(-20.0, -8.0, 104.0, 48.0);
    renderer
        .apply_image_style(
            &RenderOutput::Image(source_image()),
            &style,
            bounds(external_geometry, body, body),
        )
        .expect("paint geometry may extend outside visible Image support");

    let outside = FrameBounds::new(20.0, 8.0, 60.0, 16.0);
    let error = renderer
        .apply_image_style(
            &RenderOutput::Image(source_image()),
            &style,
            bounds(outside, outside, outside),
        )
        .expect_err("out-of-allocation bounds must fail closed");
    assert!(error.to_string().contains("exceed the 64x32 padded layer"));
}
