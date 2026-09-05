use super::*;

fn working_pixels(renderer: &mut SkiaRenderer) -> Vec<[f32; 4]> {
    let RenderOutput::Working(output) = renderer.finalize().expect("final working image") else {
        panic!("Project-linear renderer must retain working pixels");
    };
    output.pixels().pixels().to_vec()
}

fn renderer(label: &str, width: u32, height: u32) -> SkiaRenderer {
    let mut renderer = SkiaRenderer::new(width, height, Color::black(), false, None, None)
        .expect("CPU Skia renderer");
    renderer
        .use_project_linear_surface(working_contract(label))
        .expect("Project working surface");
    renderer.clear().expect("clear Project surface");
    renderer
}

fn assert_pixels_near(left: &[[f32; 4]], right: &[[f32; 4]]) {
    assert_eq!(left.len(), right.len());
    for (index, (left, right)) in left.iter().zip(right).enumerate() {
        for channel in 0..4 {
            assert!(
                (left[channel] - right[channel]).abs() <= 1.0e-6,
                "pixel {index} channel {channel}: direct={} boundary={}",
                left[channel],
                right[channel]
            );
        }
    }
}

#[test]
fn backend_native_vector_draws_match_the_owned_output_boundary() {
    let styles = [StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color {
                r: 48,
                g: 210,
                b: 96,
                a: 220,
            },
            offset: 0.0,
        },
    }];

    let mut direct_shape = renderer("native-shape", 32, 32);
    direct_shape
        .draw_shape_layer(
            ShapeRasterRequest {
                path_data: "M 0 0 L 9 0 L 9 7 L 0 7 Z",
                canonical_path: None,
                styles: &styles,
                path_effects: &[],
                ensemble: None,
                transform: Affine2D::translate(5.0, 8.0),
            },
            0.65,
            BlendMode::Normal,
        )
        .expect("native shape draw");
    let direct_shape = working_pixels(&mut direct_shape);
    let mut boundary_shape = renderer("boundary-shape", 32, 32);
    let shape = boundary_shape
        .rasterize_shape_layer(ShapeRasterRequest {
            path_data: "M 0 0 L 9 0 L 9 7 L 0 7 Z",
            canonical_path: None,
            styles: &styles,
            path_effects: &[],
            ensemble: None,
            transform: Affine2D::translate(5.0, 8.0),
        })
        .expect("owned shape output");
    boundary_shape
        .draw_layer_affine_with_blend(&shape, &Affine2D::IDENTITY, 0.65, BlendMode::Normal)
        .expect("boundary shape draw");
    assert_pixels_near(&direct_shape, &working_pixels(&mut boundary_shape));

    let mut direct_text = renderer("native-text", 48, 24);
    direct_text
        .draw_text_layer(
            TextRasterRequest {
                text: "Native",
                size: 12.0,
                font_name: "Arial",
                styles: &styles,
                ensemble: None,
                transform: Affine2D::translate(2.0, 2.0),
                current_time: 0.0,
            },
            0.8,
            BlendMode::Normal,
        )
        .expect("native text draw");
    let direct_text = working_pixels(&mut direct_text);
    let mut boundary_text = renderer("boundary-text", 48, 24);
    let text = boundary_text
        .rasterize_text_layer(TextRasterRequest {
            text: "Native",
            size: 12.0,
            font_name: "Arial",
            styles: &styles,
            ensemble: None,
            transform: Affine2D::translate(2.0, 2.0),
            current_time: 0.0,
        })
        .expect("owned text output");
    boundary_text
        .draw_layer_affine_with_blend(&text, &Affine2D::IDENTITY, 0.8, BlendMode::Normal)
        .expect("boundary text draw");
    assert_pixels_near(&direct_text, &working_pixels(&mut boundary_text));

    let shader = "half4 main(float2 p) { return half4(0.3, 0.7, 0.2, 0.75); }";
    let mut direct_sksl = renderer("native-sksl", 8, 8);
    direct_sksl
        .draw_sksl_layer(
            SkSLRasterRequest {
                shader_code: shader,
                resolution: (5.0, 6.0),
                time: 0.0,
                transform: &Affine2D::translate(1.0, 1.0),
                color_domain: SkSLColorDomain::ProjectWorkingLinear,
            },
            0.55,
            BlendMode::Normal,
        )
        .expect("native SkSL draw");
    let direct_sksl = working_pixels(&mut direct_sksl);
    let mut boundary_sksl = renderer("boundary-sksl", 8, 8);
    let sksl = boundary_sksl
        .rasterize_sksl_layer(SkSLRasterRequest {
            shader_code: shader,
            resolution: (5.0, 6.0),
            time: 0.0,
            transform: &Affine2D::translate(1.0, 1.0),
            color_domain: SkSLColorDomain::ProjectWorkingLinear,
        })
        .expect("owned SkSL output");
    boundary_sksl
        .draw_layer_affine_with_blend(&sksl, &Affine2D::IDENTITY, 0.55, BlendMode::Normal)
        .expect("boundary SkSL draw");
    assert_pixels_near(&direct_sksl, &working_pixels(&mut boundary_sksl));
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn render_layered_gpu_vectors(native: bool) -> Option<Vec<[f32; 4]>> {
    let styles = [StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color {
                r: 64,
                g: 180,
                b: 230,
                a: 210,
            },
            offset: 0.0,
        },
    }];
    let mut renderer =
        SkiaRenderer::new(96, 54, Color::black(), true, None, None).expect("GPU Skia renderer");
    renderer
        .use_project_linear_surface(working_contract("gpu-native-vector-parity"))
        .expect("GPU Project working surface");
    if !renderer
        .is_gpu_backed()
        .expect("query root surface backing")
    {
        eprintln!("skipping unsupported device: Project surface fell back to CPU raster");
        return None;
    }
    renderer.clear().expect("clear GPU Project surface");

    let shape_request = ShapeRasterRequest {
        path_data: "M 0 0 L 52 0 L 52 31 L 0 31 Z",
        canonical_path: None,
        styles: &styles,
        path_effects: &[],
        ensemble: None,
        transform: Affine2D::translate(7.0, 9.0),
    };
    if native {
        renderer
            .draw_shape_layer(shape_request, 0.72, BlendMode::Normal)
            .expect("native GPU shape");
    } else {
        let output = renderer
            .rasterize_shape_layer(shape_request)
            .expect("boundary GPU shape");
        renderer
            .draw_layer_affine_with_blend(&output, &Affine2D::IDENTITY, 0.72, BlendMode::Normal)
            .expect("composite boundary GPU shape");
    }

    let text_request = TextRasterRequest {
        text: "GPU",
        size: 22.0,
        font_name: "Arial",
        styles: &styles,
        ensemble: None,
        transform: Affine2D::translate(18.0, 11.0),
        current_time: 0.0,
    };
    if native {
        renderer
            .draw_text_layer(text_request, 0.61, BlendMode::LinearDodge)
            .expect("native GPU text");
    } else {
        let output = renderer
            .rasterize_text_layer(text_request)
            .expect("boundary GPU text");
        renderer
            .draw_layer_affine_with_blend(
                &output,
                &Affine2D::IDENTITY,
                0.61,
                BlendMode::LinearDodge,
            )
            .expect("composite boundary GPU text");
    }

    let transform = Affine2D::translate(49.0, 4.0);
    let sksl_request = SkSLRasterRequest {
        shader_code: "half4 main(float2 p) { return half4(0.55, 0.2, 0.7, 0.8); }",
        resolution: (38.0, 43.0),
        time: 0.0,
        transform: &transform,
        color_domain: SkSLColorDomain::ProjectWorkingLinear,
    };
    if native {
        renderer
            .draw_sksl_layer(sksl_request, 0.68, BlendMode::Multiply)
            .expect("native GPU SkSL");
    } else {
        let output = renderer
            .rasterize_sksl_layer(sksl_request)
            .expect("boundary GPU SkSL");
        renderer
            .draw_layer_affine_with_blend(&output, &Affine2D::IDENTITY, 0.68, BlendMode::Multiply)
            .expect("composite boundary GPU SkSL");
    }
    Some(working_pixels(&mut renderer))
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL GPU"]
fn gpu_backend_native_vector_draws_preserve_layered_pixels() {
    let Some(native) = render_layered_gpu_vectors(true) else {
        return;
    };
    let boundary = render_layered_gpu_vectors(false).expect("same GPU remains available");
    assert_eq!(native.len(), boundary.len());
    for (index, (native, boundary)) in native.iter().zip(&boundary).enumerate() {
        for channel in 0..4 {
            assert!(
                (native[channel] - boundary[channel]).abs() <= 2.0e-3,
                "GPU pixel {index} channel {channel}: native={} boundary={}",
                native[channel],
                boundary[channel]
            );
        }
    }
}
