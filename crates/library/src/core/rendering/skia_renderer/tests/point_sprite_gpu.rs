use super::point_support::test_random;
use super::*;
use crate::model::frame::point::{PointGridParameters, PointRenderStyle, SpriteSelection};
use crate::model::point::{
    NumericBinaryOperation, PointAttributeSchema, PointInstruction, PointRenderProgram,
};
use crate::model::property::{ColorValue, ImageCollectionValue, PropertyValue};
use crate::rendering::renderer::ManagedImageResource;
use glow::HasContext;
use std::sync::Arc;

const WIDTH: u32 = 160;
const HEIGHT: u32 = 96;

fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

fn sprite(config: &str, width: u32, height: u32, pixel: [f32; 4]) -> Arc<ManagedImageResource> {
    sprite_pixels(
        config,
        width,
        height,
        vec![pixel; width as usize * height as usize],
    )
}

fn sprite_pixels(
    config: &str,
    width: u32,
    height: u32,
    values: Vec<[f32; 4]>,
) -> Arc<ManagedImageResource> {
    let pixels = LinearWorkingImage::from_premultiplied_rgba_f32(width, height, values).unwrap();
    // SAFETY: the fixture samples are authored in the exact linear-sRGB
    // identity installed on the renderer and are already premultiplied.
    let image = unsafe {
        ManagedLinearWorkingImage::from_working_pixels_unchecked(working_identity(config), pixels)
    };
    Arc::new(ManagedImageResource::new(image))
}

fn grid_scene(count: u32) -> PointSceneFrame {
    PointSceneFrame {
        invocation: SceneInvocationKey {
            instance_path: InstancePath::root(TimelineId::from_uuid(Uuid::from_u128(1))),
            module_instance_id: ModuleInstanceId::from_uuid(Uuid::from_u128(2)),
            state_slot_id: Uuid::from_u128(3),
            output_id: ModuleOutputId::from_uuid(Uuid::from_u128(4)),
        },
        source_node_id: Uuid::from_u128(5),
        logical_width: WIDTH,
        logical_height: HEIGHT,
        source: PointSceneSource::Grid(PointGridParameters {
            counts: [count, 1, 1],
            spacing: particle_vec3(32.0, 0.0, 0.0),
            center: particle_vec3(0.0, 0.0, 0.0),
            size: OrderedFloat(20.0),
            seed: 29,
        }),
        color: Color::white(),
        render_style: PointRenderStyle::default(),
        point_program: None,
    }
}

fn set_sprites(
    scene: &mut PointSceneFrame,
    images: ImageCollectionValue,
    selection: SpriteSelection,
) {
    scene.render_style = PointRenderStyle::Sprites { images, selection };
}

fn set_sprite_selection(scene: &mut PointSceneFrame, selection: SpriteSelection) {
    let PointRenderStyle::Sprites {
        selection: current, ..
    } = &mut scene.render_style
    else {
        panic!("Sprite test fixture must retain Sprite render style")
    };
    *current = selection;
}

fn selector_program(invalid: bool) -> PointRenderProgram {
    let mut instructions = vec![
        PointInstruction::Constant {
            value: PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color::white())),
        },
        PointInstruction::Random { channel: 0 },
    ];
    let selection = if invalid {
        instructions.extend([
            PointInstruction::Constant {
                value: PropertyValue::Number(1.0.into()),
            },
            PointInstruction::Constant {
                value: PropertyValue::Number(0.0.into()),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Divide,
                left: 2,
                right: 3,
            },
        ]);
        4
    } else {
        1
    };
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions,
        ramps: Vec::new(),
        color_register: 0,
        position_register: None,
        size_register: None,
        sprite_selection_register: Some(selection),
    }
}

fn renderer(config: &str) -> SkiaRenderer {
    let mut renderer = SkiaRenderer::new(WIDTH, HEIGHT, transparent(), true, None, None).unwrap();
    assert!(
        renderer.gpu_context.is_some(),
        "Sprite QA requires a real GPU"
    );
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();
    renderer
}

fn render(
    renderer: &mut SkiaRenderer,
    scene: &PointSceneFrame,
    sprites: &[Arc<ManagedImageResource>],
) -> ManagedLinearWorkingImage {
    match renderer
        .rasterize_point_layer(PointRasterRequest {
            scene,
            transform: &Affine2D::IDENTITY,
            sprites,
        })
        .unwrap()
    {
        RenderOutput::Working(image) => image,
        output => panic!("expected managed Sprite output, got {output:?}"),
    }
}

fn pixel(image: &ManagedLinearWorkingImage, x: u32, y: u32) -> [f32; 4] {
    image.pixels().pixels()[y as usize * image.pixels().width() as usize + x as usize]
}

fn expected_index(factor: f32, count: usize) -> usize {
    ((factor.clamp(0.0, 1.0) * count as f32).floor() as usize).min(count - 1)
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_sprite_collection_uses_stable_random_or_value_field_per_point() {
    let config = "point-sprite-selection";
    let sprites = [
        sprite(config, 1, 1, [1.0, 0.0, 0.0, 1.0]),
        sprite(config, 1, 1, [0.0, 1.0, 0.0, 1.0]),
        sprite(config, 1, 1, [0.0, 0.0, 1.0, 1.0]),
    ];
    let colors = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    ];
    let mut scene = grid_scene(5);
    set_sprites(
        &mut scene,
        ImageCollectionValue::new((10..13).map(Uuid::from_u128).collect()).unwrap(),
        SpriteSelection::Random,
    );
    scene.point_program = Some(selector_program(false));
    let mut renderer = renderer(config);

    let random = render(&mut renderer, &scene, &sprites);
    let seed = crate::rendering::scene_runtime::invocation_seed(&scene);
    let serials = match &scene.source {
        PointSceneSource::Grid(grid) => (0..5)
            .map(|x| grid.point_serial([x, 0, 0]).unwrap())
            .collect::<Vec<_>>(),
        PointSceneSource::Particle { .. } => panic!("expected Grid Sprite fixture"),
    };
    for (x, serial) in serials.iter().copied().enumerate() {
        let expected = colors[expected_index(test_random(seed, serial, 2), sprites.len())];
        assert_pixel_near(pixel(&random, 16 + x as u32 * 32, HEIGHT / 2), expected);
    }
    let pipeline_count = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .compiled_point_pipeline_count();

    set_sprite_selection(&mut scene, SpriteSelection::Value(0.0.into()));
    let selected = render(&mut renderer, &scene, &sprites);
    let mut differs_from_random = false;
    for (x, serial) in serials.into_iter().enumerate() {
        let expected = colors[expected_index(test_random(seed, serial, 0), sprites.len())];
        let actual = pixel(&selected, 16 + x as u32 * 32, HEIGHT / 2);
        assert_pixel_near(actual, expected);
        differs_from_random |= actual != pixel(&random, 16 + x as u32 * 32, HEIGHT / 2);
    }
    assert!(differs_from_random);
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count(),
        pipeline_count
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_random_ignores_invalid_selector_branch_but_value_fails_transparent() {
    let config = "point-sprite-invalid-selector";
    let sprites = [sprite(config, 2, 2, [0.25, 0.5, 1.0, 1.0])];
    let mut scene = grid_scene(1);
    set_sprites(
        &mut scene,
        ImageCollectionValue::new(vec![Uuid::from_u128(20)]).unwrap(),
        SpriteSelection::Random,
    );
    scene.point_program = Some(selector_program(true));
    let mut renderer = renderer(config);

    let random = render(&mut renderer, &scene, &sprites);
    assert!(pixel(&random, WIDTH / 2, HEIGHT / 2)[3] > 0.99);
    set_sprite_selection(&mut scene, SpriteSelection::Value(0.5.into()));
    let value = render(&mut renderer, &scene, &sprites);
    assert!(value.pixels().pixels().iter().all(|pixel| pixel[3] == 0.0));
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_sprite_atlas_preserves_aspect_hdr_and_pixel_unpack_state() {
    let config = "point-sprite-atlas-state";
    let sprites = [sprite(config, 4, 2, [300_000.0, 0.5, 0.0, 0.5])];
    let mut renderer = renderer(config);
    renderer.activate_graphics_context().unwrap();
    renderer
        .gpu_context
        .as_mut()
        .unwrap()
        .direct_context
        .flush_and_submit();
    let gl = renderer.gpu_context.as_ref().unwrap().create_glow_context();
    // SAFETY: the fixture owns the current idle GL context. These legal
    // desktop GL 4.3 values intentionally differ from atlas upload defaults.
    unsafe {
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 8);
        gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 7);
        gl.pixel_store_i32(glow::UNPACK_IMAGE_HEIGHT, 9);
        gl.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 2);
        gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 3);
        gl.pixel_store_i32(glow::UNPACK_SKIP_IMAGES, 1);
        gl.pixel_store_i32(glow::UNPACK_SWAP_BYTES, 1);
        gl.pixel_store_i32(glow::UNPACK_LSB_FIRST, 1);
    }
    renderer
        .scene_runtime
        .as_mut()
        .unwrap()
        .preflight_sprites(&sprites)
        .unwrap();
    // SAFETY: the same fixture-owned context remains current. Queries verify
    // isolation, then the test returns pixel-store state to ordinary defaults.
    unsafe {
        assert_eq!(gl.get_parameter_i32(glow::UNPACK_ALIGNMENT), 8);
        assert_eq!(gl.get_parameter_i32(glow::UNPACK_ROW_LENGTH), 7);
        assert_eq!(gl.get_parameter_i32(glow::UNPACK_IMAGE_HEIGHT), 9);
        assert_eq!(gl.get_parameter_i32(glow::UNPACK_SKIP_ROWS), 2);
        assert_eq!(gl.get_parameter_i32(glow::UNPACK_SKIP_PIXELS), 3);
        assert_eq!(gl.get_parameter_i32(glow::UNPACK_SKIP_IMAGES), 1);
        assert_eq!(gl.get_parameter_i32(glow::UNPACK_SWAP_BYTES), 1);
        assert_eq!(gl.get_parameter_i32(glow::UNPACK_LSB_FIRST), 1);
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
        gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
        gl.pixel_store_i32(glow::UNPACK_IMAGE_HEIGHT, 0);
        gl.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
        gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
        gl.pixel_store_i32(glow::UNPACK_SKIP_IMAGES, 0);
        gl.pixel_store_i32(glow::UNPACK_SWAP_BYTES, 0);
        gl.pixel_store_i32(glow::UNPACK_LSB_FIRST, 0);
    }
    renderer
        .gpu_context
        .as_mut()
        .unwrap()
        .direct_context
        .reset(None);

    let mut scene = grid_scene(1);
    set_sprites(
        &mut scene,
        ImageCollectionValue::new(vec![Uuid::from_u128(30)]).unwrap(),
        SpriteSelection::Value(0.0.into()),
    );
    scene.point_program = Some(PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![PointInstruction::Constant {
            value: PropertyValue::ColorValue(
                ColorValue::new(
                    crate::model::property::ColorSpaceRef::linear_srgb(),
                    [0.5, 0.25, 1.0, 0.5],
                )
                .unwrap(),
            ),
        }],
        ramps: Vec::new(),
        color_register: 0,
        position_register: None,
        size_register: None,
        sprite_selection_register: None,
    });
    // Inspect the actual atlas sampler in an F32 target. Ganesh's existing
    // device-dependent F16 fallback is a separate, lower-range boundary.
    let runtime = renderer.scene_runtime.as_mut().unwrap();
    runtime
        .render_point(
            PointRasterRequest {
                scene: &scene,
                transform: &Affine2D::IDENTITY,
                sprites: &sprites,
            },
            WIDTH,
            HEIGHT,
            crate::rendering::scene_runtime::SceneTextureFormat::LinearRgbaF32,
            [1.0; 4],
        )
        .unwrap();
    let center = runtime
        .read_target_pixel_f32(WIDTH / 2, HEIGHT / 2)
        .unwrap();
    assert!(
        center[0].is_finite() && center[0] > 65_504.0,
        "RGBA32F atlas lost HDR value: {center:?}"
    );
    for (actual, expected) in center.into_iter().zip([75_000.0_f32, 0.0625, 0.0, 0.25]) {
        let tolerance = expected.abs().max(1.0) * 2e-5;
        assert!(
            (actual - expected).abs() <= tolerance,
            "texture premultiplication and Point tint alpha must each apply exactly once: {center:?}"
        );
    }
    renderer
        .gpu_context
        .as_mut()
        .unwrap()
        .direct_context
        .reset(None);
    // Also exercise the full Skia bridge with HDR values inside both supported
    // float surface ranges; this must retain tint and alpha without clipping to 1.
    let image = render(
        &mut renderer,
        &scene,
        &[sprite(config, 4, 2, [12.0, 0.5, 0.0, 0.5])],
    );
    assert_pixel_near(
        pixel(&image, WIDTH / 2, HEIGHT / 2),
        [3.0, 0.0625, 0.0, 0.25],
    );
    let mut min_x = WIDTH;
    let mut max_x = 0;
    let mut min_y = HEIGHT;
    let mut max_y = 0;
    for (index, pixel) in image.pixels().pixels().iter().enumerate() {
        if pixel[3] > 0.01 {
            let x = index as u32 % WIDTH;
            let y = index as u32 / WIDTH;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    assert!(
        max_x - min_x > max_y - min_y,
        "natural 4:2 aspect was not preserved"
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_sprite_content_and_selector_changes_preserve_particle_history() {
    let config = "point-sprite-render-only-invalidation";
    let red = sprite(config, 1, 1, [1.0, 0.0, 0.0, 1.0]);
    let green = sprite(config, 1, 1, [0.0, 1.0, 0.0, 1.0]);
    let blue = sprite(config, 1, 1, [0.0, 0.0, 1.0, 1.0]);
    let mut scene = particle_scene(240);
    set_sprites(
        &mut scene,
        ImageCollectionValue::new(vec![Uuid::from_u128(40), Uuid::from_u128(41)]).unwrap(),
        SpriteSelection::Value(0.0.into()),
    );
    let mut renderer = SkiaRenderer::new(
        scene.logical_width,
        scene.logical_height,
        transparent(),
        true,
        None,
        None,
    )
    .unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();

    let first = render(&mut renderer, &scene, &[red.clone(), green.clone()]);
    let before = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .invocation_stats(&scene.invocation)
        .unwrap();
    set_sprite_selection(&mut scene, SpriteSelection::Value(1.0.into()));
    let selected = render(&mut renderer, &scene, &[red.clone(), green]);
    assert_ne!(selected.pixels().pixels(), first.pixels().pixels());
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .invocation_stats(&scene.invocation)
            .unwrap(),
        before
    );

    let changed = render(&mut renderer, &scene, &[red, blue]);
    assert_ne!(changed.pixels().pixels(), selected.pixels().pixels());
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .invocation_stats(&scene.invocation)
            .unwrap(),
        before
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_sprite_atlas_keeps_top_down_orientation_and_extruded_edges() {
    let config = "point-sprite-atlas-orientation";
    let red = [1.0, 0.0, 0.0, 1.0];
    let green = [0.0, 1.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    let yellow = [1.0, 1.0, 0.0, 1.0];
    let mut quadrants = Vec::new();
    for y in 0..4 {
        for x in 0..4 {
            quadrants.push(match (x < 2, y < 2) {
                (true, true) => red,
                (false, true) => green,
                (true, false) => blue,
                (false, false) => yellow,
            });
        }
    }
    let sprites = [
        sprite_pixels(config, 4, 4, quadrants),
        sprite(config, 4, 4, [1.0, 0.0, 1.0, 1.0]),
    ];
    let mut scene = grid_scene(1);
    let PointSceneSource::Grid(grid) = &mut scene.source else {
        panic!("expected Grid Sprite fixture")
    };
    grid.size = 40.0.into();
    set_sprites(
        &mut scene,
        ImageCollectionValue::new(vec![Uuid::from_u128(50), Uuid::from_u128(51)]).unwrap(),
        SpriteSelection::Value(0.0.into()),
    );
    let mut renderer = renderer(config);
    let image = render(&mut renderer, &scene, &sprites);
    let center = (WIDTH / 2, HEIGHT / 2);

    for (offset, expected) in [
        ((-12, -12), red),
        ((12, -12), green),
        ((-12, 12), blue),
        ((12, 12), yellow),
        // The right edge samples the atlas gutter. It must extrude this
        // Sprite's texel rather than blending the adjacent magenta image.
        ((19, -12), green),
        ((19, 12), yellow),
    ] {
        let actual = pixel(
            &image,
            center.0.checked_add_signed(offset.0).unwrap(),
            center.1.checked_add_signed(offset.1).unwrap(),
        );
        assert_pixel_near(actual, expected);
    }
}
