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
                parts: &[],
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
            parts: &[],
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
        parts: &[],
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
fn render_layer_mask_case(
    styles: &[StyleConfig],
    transform: Affine2D,
    use_gpu: bool,
) -> Vec<[f32; 4]> {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let context = use_gpu.then(|| {
        create_gpu_context(None, None).expect("actual OpenGL context required for GPU parity")
    });
    let mut renderer = SkiaRenderer::new(72, 72, transparent, use_gpu, context, None)
        .expect("layer-mask parity renderer");
    renderer
        .use_project_linear_surface(working_contract("gpu-layer-mask-parity"))
        .expect("Project working surface");
    if use_gpu {
        assert!(
            renderer
                .is_gpu_backed()
                .expect("query root surface backing"),
            "test must not silently use a CPU raster surface"
        );
    }
    renderer.clear().expect("clear Project surface");
    renderer
        .draw_shape_layer(
            ShapeRasterRequest {
                path_data: "M 20 20 L 44 20 L 44 44 L 20 44 Z",
                canonical_path: None,
                parts: &[],
                styles,
                path_effects: &[],
                ensemble: None,
                transform,
            },
            1.0,
            BlendMode::Normal,
        )
        .expect("draw layer-mask parity shape");
    working_pixels(&mut renderer)
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn layer_mask_white_fill() -> StyleConfig {
    StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color::white(),
            offset: 0.0,
        },
    }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn layer_mask_shadow(distance: f64, size: f64) -> StyleConfig {
    StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::DropShadow {
            color: Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            angle: 180.0,
            distance,
            spread: 0.0,
            size,
        },
    }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn assert_gpu_layer_mask_near(cpu: &[[f32; 4]], gpu: &[[f32; 4]]) {
    assert_eq!(cpu.len(), gpu.len());
    let mut max_difference = 0.0_f32;
    let mut max_index = 0;
    let mut max_channel = 0;
    let mut differing_channels = 0;
    let mut difference_sum = 0.0_f32;
    for (index, (cpu, gpu)) in cpu.iter().zip(gpu).enumerate() {
        for channel in 0..4 {
            let difference = (cpu[channel] - gpu[channel]).abs();
            difference_sum += difference;
            if difference > 2.0e-3 {
                differing_channels += 1;
            }
            if difference > max_difference {
                max_difference = difference;
                max_index = index;
                max_channel = channel;
            }
            // Both surfaces are premultiplied project-linear RGBAF32. The
            // tolerance permits GPU coverage/filter rounding, but remains
            // below one 8-bit alpha step so a visible mask error cannot pass.
        }
    }
    assert!(
        max_difference <= 2.0e-3,
        "GPU layer-mask max difference={max_difference} at pixel {max_index} channel {max_channel}: CPU={:?} GPU={:?}; differing channels={differing_channels}, sum={difference_sum}",
        cpu[max_index],
        gpu[max_index]
    );
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn assert_gpu_layer_mask_near_outside_baseline_edges(
    cpu: &[[f32; 4]],
    gpu: &[[f32; 4]],
    baseline_cpu: &[[f32; 4]],
    baseline_gpu: &[[f32; 4]],
) {
    assert_eq!(cpu.len(), gpu.len());
    assert_eq!(cpu.len(), baseline_cpu.len());
    assert_eq!(cpu.len(), baseline_gpu.len());
    let is_partial_coverage = |alpha: f32| alpha > 2.0e-3 && alpha < 1.0 - 2.0e-3;
    let coverage_edges = baseline_cpu
        .iter()
        .zip(baseline_gpu)
        .map(|(cpu, gpu)| is_partial_coverage(cpu[3]) || is_partial_coverage(gpu[3]))
        .collect::<Vec<_>>();
    assert!(
        coverage_edges.iter().any(|edge| *edge),
        "opaque baseline must identify the backend-specific AA contour"
    );
    for (index, (cpu, gpu)) in cpu.iter().zip(gpu).enumerate() {
        if coverage_edges[index] {
            continue;
        }
        for channel in 0..4 {
            assert!(
                (cpu[channel] - gpu[channel]).abs() <= 2.0e-3,
                "non-contour GPU layer-mask pixel {index} channel {channel}: CPU={cpu:?} GPU={gpu:?}"
            );
        }
    }

    // CPU and GPU Skia use different AA coverage on the narrow baseline
    // contour. Compare its conserved position and channel energy separately,
    // without relaxing tolerance for the mask interior, hole, or shadow.
    let alpha_bounds = |pixels: &[[f32; 4]]| {
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for (index, pixel) in pixels.iter().enumerate() {
            if pixel[3] <= 2.0e-3 {
                continue;
            }
            let point = (index % 72, index / 72);
            bounds = Some(match bounds {
                None => (point.0, point.1, point.0, point.1),
                Some((min_x, min_y, max_x, max_y)) => (
                    min_x.min(point.0),
                    min_y.min(point.1),
                    max_x.max(point.0),
                    max_y.max(point.1),
                ),
            });
        }
        bounds
    };
    assert_eq!(alpha_bounds(cpu), alpha_bounds(gpu));
    for channel in 0..4 {
        let cpu_sum = cpu.iter().map(|pixel| pixel[channel]).sum::<f32>();
        let gpu_sum = gpu.iter().map(|pixel| pixel[channel]).sum::<f32>();
        assert!(
            (cpu_sum - gpu_sum).abs() <= cpu_sum.abs().max(1.0) * 5.0e-3,
            "GPU layer-mask channel {channel} energy differs: CPU={cpu_sum} GPU={gpu_sum}"
        );
    }
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

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL GPU"]
fn gpu_layer_mask_matches_cpu_for_stroke_hole_partial_alpha_and_transform() {
    let stroke = StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Stroke {
            color: Color::white(),
            width: 4.0,
            offset: 0.0,
            cap: Default::default(),
            join: Default::default(),
            miter: 4.0,
            dash_array: Vec::new(),
            dash_offset: 0.0,
        },
    };
    // Keep boundaries on whole device pixels. CPU and GPU rasterizers are
    // allowed to choose different subpixel coverage, which is orthogonal to
    // the transformed LayerMask/filter parity asserted here.
    let transform = Affine2D::translate(8.0, 6.0);
    let baseline_cpu = render_layer_mask_case(std::slice::from_ref(&stroke), transform, false);
    let baseline_gpu = render_layer_mask_case(std::slice::from_ref(&stroke), transform, true);

    let stroke_styles = [stroke, layer_mask_shadow(0.0, 0.0)];
    let stroke_cpu = render_layer_mask_case(&stroke_styles, transform, false);
    let stroke_gpu = render_layer_mask_case(&stroke_styles, transform, true);
    assert_gpu_layer_mask_near_outside_baseline_edges(
        &stroke_cpu,
        &stroke_gpu,
        &baseline_cpu,
        &baseline_gpu,
    );
    let (center_x, center_y) = transform.map_point(32.0, 32.0);
    let center = center_y as usize * 72 + center_x as usize;
    assert!(
        stroke_cpu[center][3] <= 1.0e-6 && stroke_gpu[center][3] <= 1.0e-6,
        "the transformed Stroke-only mask must retain its transparent center"
    );

    let partial_styles = [
        StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::Fill {
                color: Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 128,
                },
                offset: 0.0,
            },
        },
        layer_mask_shadow(10.0, 0.0),
    ];
    let partial_cpu = render_layer_mask_case(&partial_styles, Affine2D::IDENTITY, false);
    let partial_gpu = render_layer_mask_case(&partial_styles, Affine2D::IDENTITY, true);
    assert_gpu_layer_mask_near(&partial_cpu, &partial_gpu);
    let cast = 32 * 72 + 50;
    let expected_alpha = 128.0 / 255.0;
    assert!(
        (partial_cpu[cast][3] - expected_alpha).abs() < 0.02
            && (partial_gpu[cast][3] - expected_alpha).abs() < 0.02,
        "Drop Shadow must preserve the composed Fill alpha on CPU and GPU"
    );
    let outside = 8 * 72 + 8;
    assert!(
        partial_cpu[outside][3] <= 1.0e-6 && partial_gpu[outside][3] <= 1.0e-6,
        "transparent pixels outside the mask must remain transparent"
    );

    let blurred_styles = [
        StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::Fill {
                color: Color::white(),
                offset: 0.0,
            },
        },
        layer_mask_shadow(8.0, 6.0),
    ];
    let blurred_cpu = render_layer_mask_case(&blurred_styles, Affine2D::IDENTITY, false);
    let blurred_gpu = render_layer_mask_case(&blurred_styles, Affine2D::IDENTITY, true);
    assert_gpu_layer_mask_near(&blurred_cpu, &blurred_gpu);
    assert!(
        blurred_cpu[32 * 72 + 49][0] > 0.01 && blurred_gpu[32 * 72 + 49][0] > 0.01,
        "blurred Drop Shadow must remain visible outside the Fill on CPU and GPU"
    );

    // Nonuniform scaling is deliberately chosen so the source and translated
    // shadow boundaries still land on whole device pixels. This isolates GPU
    // LayerMask transform parity from backend-specific subpixel AA coverage.
    let nonuniform_transform = Affine2D::translate(-8.0, 18.0).compose(Affine2D::scale(1.5, 0.5));
    let fill = layer_mask_white_fill();
    let nonuniform_baseline_cpu =
        render_layer_mask_case(std::slice::from_ref(&fill), nonuniform_transform, false);
    let nonuniform_baseline_gpu =
        render_layer_mask_case(std::slice::from_ref(&fill), nonuniform_transform, true);
    let nonuniform_styles = [fill, layer_mask_shadow(10.0, 0.0)];
    let nonuniform_cpu = render_layer_mask_case(&nonuniform_styles, nonuniform_transform, false);
    let nonuniform_gpu = render_layer_mask_case(&nonuniform_styles, nonuniform_transform, true);
    assert_gpu_layer_mask_near(&nonuniform_baseline_cpu, &nonuniform_baseline_gpu);
    assert_gpu_layer_mask_near(&nonuniform_cpu, &nonuniform_gpu);
    let (cast_x, cast_y) = nonuniform_transform.map_point(50.0, 32.0);
    let cast = cast_y.round() as usize * 72 + cast_x.round() as usize;
    assert!(
        nonuniform_baseline_cpu[cast][3] <= 1.0e-6
            && nonuniform_baseline_gpu[cast][3] <= 1.0e-6
            && nonuniform_cpu[cast][0] > 0.1
            && nonuniform_gpu[cast][0] > 0.1,
        "nonuniform off-origin Drop Shadow must extend beyond the Fill on CPU and GPU"
    );
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "known issue: fractional nonuniform transforms produce CPU/GPU blur-AA divergence"]
fn gpu_layer_mask_fractional_nonuniform_blur_matches_cpu() {
    let transform = Affine2D::translate(-6.0, 18.0).compose(Affine2D::scale(1.35, 0.55));
    let fill = layer_mask_white_fill();
    let baseline_cpu = render_layer_mask_case(std::slice::from_ref(&fill), transform, false);
    let baseline_gpu = render_layer_mask_case(std::slice::from_ref(&fill), transform, true);
    let styles = [fill, layer_mask_shadow(10.0, 2.0)];
    let cpu = render_layer_mask_case(&styles, transform, false);
    let gpu = render_layer_mask_case(&styles, transform, true);
    assert_gpu_layer_mask_near_outside_baseline_edges(&cpu, &gpu, &baseline_cpu, &baseline_gpu);
}
