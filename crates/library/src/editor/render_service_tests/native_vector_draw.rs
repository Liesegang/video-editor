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
