use super::*;

#[test]
fn vector_objects_without_effects_use_backend_native_draw_boundaries() {
    let object = |content| {
        FrameItem::Object(FrameObject {
            source_node_id: uuid::Uuid::new_v4(),
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: None,
            content,
        })
    };
    let frame = FrameInfo {
        width: 16,
        height: 16,
        background_color: Color::black(),
        color_profile: String::new(),
        render_scale: OrderedFloat(1.0),
        now_time: OrderedFloat(0.0),
        region: None,
        items: vec![
            object(FrameContent::Text {
                text: "native".into(),
                font: "Arial".into(),
                size: 12.0,
                styles: Vec::new(),
                effects: Vec::new(),
                ensemble: None,
                transform: Transform::default(),
            }),
            object(FrameContent::Shape {
                path: "M 0 0 L 1 0 L 1 1 Z".into(),
                canonical_path: None,
                parts: Vec::new(),
                styles: Vec::new(),
                path_effects: Vec::new(),
                effects: Vec::new(),
                ensemble: None,
                transform: Transform::default(),
            }),
            object(FrameContent::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".into(),
                resolution: (16.0, 16.0),
                color_domain: crate::model::frame::entity::SkSLColorDomain::ProjectWorkingLinear,
                effects: Vec::new(),
                transform: Transform::default(),
            }),
        ],
    };
    let renderer = TexturePathRenderer {
        saw_texture_layer: false,
        shape_part_opacities: Vec::new(),
        native_group_composites: 0,
        direct_text_draws: 0,
        direct_shape_draws: 0,
        direct_sksl_draws: 0,
        direct_particle_draws: 0,
        particle_rasterizations: 0,
    };
    let mut service = RenderService::new(
        renderer,
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );

    service
        .render_from_frame_info(&frame)
        .expect("backend-native vector draws");
    assert_eq!(service.renderer.direct_text_draws, 1);
    assert_eq!(service.renderer.direct_shape_draws, 1);
    assert_eq!(service.renderer.direct_sksl_draws, 1);
}

#[test]
fn grouped_shape_reaches_one_raster_and_one_image_effect_application() {
    let effect_calls = Arc::new(AtomicUsize::new(0));
    let plugin_manager = Arc::new(PluginManager::default());
    plugin_manager.register_effect(Arc::new(CountingEffect {
        calls: Arc::clone(&effect_calls),
    }));
    let part = |path: &str, opacity: f32| crate::model::frame::entity::FramePathPart {
        path: path.to_string(),
        canonical_path: None,
        opacity: OrderedFloat(opacity),
    };
    let frame = FrameInfo {
        width: 16,
        height: 16,
        background_color: Color::black(),
        color_profile: String::new(),
        render_scale: OrderedFloat(1.0),
        now_time: OrderedFloat(0.0),
        region: None,
        items: vec![FrameItem::Object(FrameObject {
            source_node_id: uuid::Uuid::new_v4(),
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: None,
            content: FrameContent::Shape {
                path: "M 1 1 L 7 1 L 7 7 Z M 8 8 L 14 8 L 14 14 Z".into(),
                canonical_path: None,
                parts: vec![
                    part("M 1 1 L 7 1 L 7 7 Z", 1.0),
                    part("M 8 8 L 14 8 L 14 14 Z", 0.4),
                ],
                styles: Vec::new(),
                path_effects: Vec::new(),
                effects: vec![crate::model::frame::effect::ImageEffect {
                    effect_type: "counting_track_effect".to_string(),
                    properties: Default::default(),
                }],
                ensemble: None,
                transform: Transform::default(),
            },
        })],
    };
    let renderer = TexturePathRenderer {
        saw_texture_layer: false,
        shape_part_opacities: Vec::new(),
        native_group_composites: 0,
        direct_text_draws: 0,
        direct_shape_draws: 0,
        direct_sksl_draws: 0,
        direct_particle_draws: 0,
        particle_rasterizations: 0,
    };
    let mut service = RenderService::new(renderer, plugin_manager, Arc::new(CacheManager::new()));

    service
        .render_from_frame_info(&frame)
        .expect("grouped Shape through RenderService");
    assert_eq!(service.renderer.shape_part_opacities, [1.0, 0.4]);
    assert_eq!(effect_calls.load(Ordering::SeqCst), 1);
}
