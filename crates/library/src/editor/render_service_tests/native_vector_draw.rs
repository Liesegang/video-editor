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
        image_style_composites: Vec::new(),
        last_direct_shape_transform: None,
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
                effects: vec![crate::model::frame::effect::ImageEffect::Plugin {
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
        image_style_composites: Vec::new(),
        last_direct_shape_transform: None,
    };
    let mut service = RenderService::new(renderer, plugin_manager, Arc::new(CacheManager::new()));

    service
        .render_from_frame_info(&frame)
        .expect("grouped Shape through RenderService");
    assert_eq!(service.renderer.shape_part_opacities, [1.0, 0.4]);
    assert_eq!(effect_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn image_style_is_applied_in_local_pixels_before_the_outer_affine() {
    let group_transform = Transform {
        position: crate::model::frame::transform::Position { x: 12.0, y: -4.0 },
        scale: crate::model::frame::transform::Scale { x: 2.0, y: 0.75 },
        rotation: 90.0,
        opacity: 0.8,
        ..Transform::default()
    };
    let child = FrameItem::Object(FrameObject {
        source_node_id: uuid::Uuid::new_v4(),
        spatial_transform_node_id: None,
        spatial_transform: Box::default(),
        content_bounds: None,
        content: FrameContent::Shape {
            path: "M 1 1 L 7 1 L 7 7 Z".into(),
            canonical_path: None,
            parts: Vec::new(),
            styles: Vec::new(),
            path_effects: Vec::new(),
            effects: Vec::new(),
            ensemble: None,
            transform: Transform::default(),
        },
    });
    let style = crate::model::frame::entity::StyleConfig {
        id: uuid::Uuid::new_v4(),
        style: crate::model::frame::draw_type::DrawStyle::DropShadow {
            color: Color::black(),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            angle: 120.0,
            distance: 10.0,
            spread: 0.0,
            size: 4.0,
        },
    };
    let frame = FrameInfo {
        width: 64,
        height: 48,
        background_color: Color::black(),
        color_profile: String::new(),
        render_scale: OrderedFloat(0.5),
        now_time: OrderedFloat(0.0),
        region: None,
        items: vec![FrameItem::Group(FrameGroup {
            source_id: style.id,
            kind: FrameGroupKind::ImageStyle,
            width: 64,
            height: 48,
            background_color: Color::black(),
            transform: group_transform.clone(),
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(0.0),
            effects: vec![crate::model::frame::effect::ImageEffect::LayerStyle(style)],
            items: vec![child],
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
        image_style_composites: Vec::new(),
        last_direct_shape_transform: None,
    };
    let mut service = RenderService::new(
        renderer,
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );

    service
        .render_from_frame_info(&frame)
        .expect("local Image style rendering");

    let child_transform = service
        .renderer
        .last_direct_shape_transform
        .expect("child raster transform");
    assert_eq!(
        (child_transform.scale_x, child_transform.scale_y),
        (0.5, 0.5)
    );
    assert_eq!(
        (child_transform.skew_x, child_transform.skew_y),
        (0.0, 0.0),
        "the child must not receive the outer Image rotation"
    );
    let [(style_scale, actual_transform)] = service.renderer.image_style_composites.as_slice()
    else {
        panic!("expected exactly one backend-native Image style composite")
    };
    assert_eq!(*style_scale, 0.5);
    let expected_transform = Affine2D::scale(0.5, 0.5).compose(Affine2D::from(&group_transform));
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)] {
        let actual = actual_transform.compose(child_transform).map_point(x, y);
        let expected = expected_transform.map_point(x, y);
        assert!(
            (actual.0 - expected.0).abs() < 1.0e-12 && (actual.1 - expected.1).abs() < 1.0e-12,
            "padded raster origin must cancel before the outer affine: {actual:?} != {expected:?}"
        );
    }
}
