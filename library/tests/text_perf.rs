use library::SkiaRenderer;
use library::core::rendering::renderer::Renderer;
use library::model::frame::entity::StyleConfig;
use library::model::frame::transform::Transform;
use library::model::frame::color::Color;
use library::model::frame::draw_type::DrawStyle;
use uuid::Uuid;
use std::collections::HashMap;

#[test]
#[ignore]
fn benchmark_ensemble_text_rendering() {
    let mut renderer = SkiaRenderer::new(1920, 1080, Color::default(), false, None);
    let text = "Hello World! This is a test for performance optimization.".repeat(10);
    let styles = vec![StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color { r: 255, g: 255, b: 255, a: 255 },
            offset: 0.0,
        },
    }];
    let transform = Transform::default();

    // Create EnsembleData (enabled)
    let ensemble = library::core::ensemble::EnsembleData {
        enabled: true,
        effector_configs: vec![],
        decorator_configs: vec![],
        patches: HashMap::new(),
    };

    let start = std::time::Instant::now();
    for _ in 0..100 {
        let _ = renderer.rasterize_text_layer(
            &text,
            24.0,
            &"Arial".to_string(),
            &styles,
            Some(&ensemble),
            &transform,
        );
    }
    println!("Elapsed: {:?}", start.elapsed());
}
