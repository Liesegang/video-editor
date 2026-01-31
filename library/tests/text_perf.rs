use library::core::ensemble::EnsembleData;
use library::model::frame::color::Color;
use library::model::frame::transform::Transform;
use library::model::frame::entity::StyleConfig;
use library::model::frame::draw_type::DrawStyle;
use library::rendering::renderer::Renderer;
use library::SkiaRenderer;
use std::time::Instant;
use uuid::Uuid;

#[test]
#[ignore]
fn test_ensemble_text_performance() {
    let width = 1920;
    let height = 1080;
    let bg_color = Color { r: 0, g: 0, b: 0, a: 255 };

    // We use CPU renderer for this test to avoid GPU context setup complexity in tests
    let mut renderer = SkiaRenderer::new(width, height, bg_color, false, None);

    // Create a long text to amplify the performance impact
    // 45 chars * 200 = 9000 chars
    let text = "The quick brown fox jumps over the lazy dog. ".repeat(200);
    let size = 24.0;
    let font_name = "Arial".to_string();

    let styles = vec![StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color { r: 255, g: 255, b: 255, a: 255 },
            offset: 0.0,
        },
    }];

    let transform = Transform::default();

    let mut ensemble = EnsembleData::default();
    ensemble.enabled = true;

    // Warmup
    for _ in 0..5 {
        let _ = renderer.rasterize_text_layer(
            &text,
            size,
            &font_name,
            &styles,
            Some(&ensemble),
            &transform
        );
    }

    let iterations = 50;
    let start = Instant::now();

    for _ in 0..iterations {
        let _ = renderer.rasterize_text_layer(
            &text,
            size,
            &font_name,
            &styles,
            Some(&ensemble),
            &transform
        );
    }

    let duration = start.elapsed();
    println!("Time taken for {} iterations: {:?}", iterations, duration);
    let avg = duration / iterations as u32;
    println!("Average time per iteration: {:?}", avg);
}
