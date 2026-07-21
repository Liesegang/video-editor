    #[test]
    fn style_wire_conversion_covers_fill_and_stroke_and_rejects_invalid_output() {
        use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType};

        let source_id = uuid::Uuid::new_v4();
        let fill = style_config_from_wire(
            StyleOutputV1::Fill {
                color: ColorV1 {
                    r: 1,
                    g: 2,
                    b: 3,
                    a: 4,
                },
                offset: 2.5,
            },
            source_id,
        )
        .expect("finite Fill converts")
        .expect("Fill produces a config");
        assert_eq!(fill.id, source_id, "the host owns Style config identity");
        assert_eq!(
            fill.style,
            DrawStyle::Fill {
                color: crate::model::frame::color::Color {
                    r: 1,
                    g: 2,
                    b: 3,
                    a: 4,
                },
                offset: 2.5,
            }
        );

        let stroke = style_config_from_wire(
            StyleOutputV1::Stroke {
                color: ColorV1 {
                    r: 5,
                    g: 6,
                    b: 7,
                    a: 8,
                },
                width: 3.0,
                offset: -1.0,
                cap: StrokeCapV1::Butt,
                join: StrokeJoinV1::Bevel,
                miter: 4.0,
                dash_array: vec![2.0, 1.0],
                dash_offset: 0.5,
            },
            source_id,
        )
        .expect("finite Stroke converts")
        .expect("Stroke produces a config");
        assert_eq!(
            stroke.style,
            DrawStyle::Stroke {
                color: crate::model::frame::color::Color {
                    r: 5,
                    g: 6,
                    b: 7,
                    a: 8,
                },
                width: 3.0,
                offset: -1.0,
                cap: CapType::Butt,
                join: JoinType::Bevel,
                miter: 4.0,
                dash_array: vec![2.0, 1.0],
                dash_offset: 0.5,
            }
        );

        assert!(
            style_config_from_wire(
                StyleOutputV1::Fill {
                    color: ColorV1 {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 0,
                    },
                    offset: f64::INFINITY,
                },
                source_id,
            )
            .is_err(),
            "non-finite output must not reach host StyleConfig"
        );
        assert!(
            style_config_from_wire(
                StyleOutputV1::Fill {
                    color: ColorV1 {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                    offset: f64::MAX,
                },
                source_id,
            )
            .is_err(),
            "finite f64 that overflows the renderer's scalar must be NoOutput"
        );
        assert!(
            style_config_from_wire(
                StyleOutputV1::Stroke {
                    color: ColorV1 {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                    width: f32::MAX as f64,
                    offset: f32::MAX as f64,
                    cap: StrokeCapV1::Round,
                    join: StrokeJoinV1::Round,
                    miter: 4.0,
                    dash_array: Vec::new(),
                    dash_offset: 0.0,
                },
                source_id,
            )
            .is_err(),
            "derived effective width must remain a finite renderer scalar"
        );
        for invalid_dash in [vec![1.0], vec![1.0, 0.0], vec![1.0, -1.0]] {
            assert!(
                style_config_from_wire(
                    StyleOutputV1::Stroke {
                        color: ColorV1 {
                            r: 0,
                            g: 0,
                            b: 0,
                            a: 255,
                        },
                        width: 1.0,
                        offset: 0.0,
                        cap: StrokeCapV1::Round,
                        join: StrokeJoinV1::Round,
                        miter: 4.0,
                        dash_array: invalid_dash,
                        dash_offset: 0.0,
                    },
                    source_id,
                )
                .is_err(),
                "unsafe dash config must become NoOutput"
            );
        }
        assert!(valid_stroke_dash_pattern(&[], 0.0));
        assert!(valid_stroke_dash_pattern(&[2.0, 1.0], 0.0));
        assert!(
            skia_safe::PathEffect::dash(&[1.0], 0.0).is_none(),
            "Skia rejects an odd number of dash intervals"
        );
        assert!(
            skia_safe::PathEffect::dash(&[0.0, 0.0], 0.0).is_none(),
            "Skia rejects an all-zero dash definition"
        );
        assert!(
            skia_safe::PathEffect::dash(&[2.0, 1.0], 0.0).is_some(),
            "the ABI's accepted dash shape is executable by Skia"
        );
        assert!(
            style_config_from_response(
                serde_json::json!({
                    "type": "fill",
                    "color": {"r": 0, "g": 0, "b": 0, "a": 255},
                    "offset": 0.0,
                    "undeclared": true
                }),
                source_id,
            )
            .is_err(),
            "undeclared plugin output fields are rejected"
        );
        assert!(
            style_config_from_response(serde_json::json!({"type": "future_style"}), source_id)
                .is_err(),
            "unknown output variants are rejected"
        );
        assert!(
            safe_style_config_from_response(
                serde_json::json!({
                    "type": "stroke",
                    "color": {"r": 0, "g": 0, "b": 0, "a": 255},
                    "width": 1.0,
                    "offset": 0.0,
                    "cap": "round",
                    "join": "round",
                    "miter": 4.0,
                    "dash_array": [1.0],
                    "dash_offset": 0.0
                }),
                source_id,
                "test runtime Style"
            )
            .is_none(),
            "a decoded but unsafe dash response must fail safely as NoOutput"
        );
    }

    #[test]
    fn stroke_rejects_overflow_in_each_renderer_derived_width() {
        let source_id = uuid::Uuid::new_v4();
        let renderer_limit = f32::MAX as f64;

        assert!(
            !valid_stroke_render_geometry(1.0, -renderer_limit),
            "a negative offset can overflow the shape outer/inner widths even when text clamps to zero"
        );
        assert!(
            !valid_stroke_render_geometry(1.0, renderer_limit),
            "a positive offset can overflow both text and shape widths"
        );
        assert!(
            safe_style_config_from_response(
                serde_json::json!({
                    "type": "stroke",
                    "color": {"r": 0, "g": 0, "b": 0, "a": 255},
                    "width": 1.0,
                    "offset": -renderer_limit,
                    "cap": "round",
                    "join": "round",
                    "miter": 4.0,
                    "dash_array": [],
                    "dash_offset": 0.0
                }),
                source_id,
                "test runtime Style"
            )
            .is_none(),
            "unsafe derived widths must turn a decoded response into NoOutput"
        );

        let safe_boundary = renderer_limit / 4.0;
        for offset in [-safe_boundary, safe_boundary] {
            assert!(
                valid_stroke_render_geometry(1.0, offset),
                "large positive and negative offsets remain valid below every f32 derived-width limit"
            );
            assert!(
                style_config_from_wire(
                    StyleOutputV1::Stroke {
                        color: ColorV1 {
                            r: 0,
                            g: 0,
                            b: 0,
                            a: 255,
                        },
                        width: 1.0,
                        offset,
                        cap: StrokeCapV1::Round,
                        join: StrokeJoinV1::Round,
                        miter: 4.0,
                        dash_array: Vec::new(),
                        dash_offset: 0.0,
                    },
                    source_id,
                )
                .expect("boundary-safe Stroke converts")
                .is_some()
            );

            let outer_width = ((offset.abs() + 0.5) * 2.0) as f32;
            let mut paint = skia_safe::Paint::default();
            paint.set_style(skia_safe::PaintStyle::Stroke);
            paint.set_stroke_width(outer_width);
            assert_eq!(
                paint.stroke_width(),
                outer_width,
                "the boundary-safe derived width is retained by an actual Skia paint"
            );
        }
    }

    #[test]
    fn stroke_dash_rejects_f32_period_overflow_and_caps_work() {
        let source_id = uuid::Uuid::new_v4();
        let overflowing = [f32::MAX as f64, f32::MAX as f64];
        assert!(
            !valid_stroke_dash_pattern(&overflowing, 0.0),
            "individually finite intervals can still overflow their f32 period"
        );
        assert!(
            skia_safe::PathEffect::dash(&[f32::MAX, f32::MAX], 0.0).is_none(),
            "Skia rejects the overflowing period instead of producing a dash effect"
        );
        assert!(
            safe_style_config_from_response(
                serde_json::json!({
                    "type": "stroke",
                    "color": {"r": 0, "g": 0, "b": 0, "a": 255},
                    "width": 1.0,
                    "offset": 0.0,
                    "cap": "round",
                    "join": "round",
                    "miter": 4.0,
                    "dash_array": overflowing,
                    "dash_offset": 0.0
                }),
                source_id,
                "test runtime Style"
            )
            .is_none(),
            "an overflowing dash response must be NoOutput, not a silent solid stroke"
        );

        let at_limit = vec![1.0; MAX_STYLE_DASH_INTERVALS_V1];
        assert!(valid_stroke_dash_pattern(&at_limit, 0.0));
        assert!(
            skia_safe::PathEffect::dash(&vec![1.0_f32; MAX_STYLE_DASH_INTERVALS_V1], 0.0).is_some(),
            "the maximum accepted interval count constructs a real Skia effect"
        );
        assert!(
            !valid_stroke_dash_pattern(&vec![1.0; MAX_STYLE_DASH_INTERVALS_V1 + 2], 0.0),
            "dash work is bounded even for otherwise-valid pairs"
        );
        assert!(!valid_stroke_dash_pattern(&[], f64::INFINITY));
    }

    #[test]
    fn decorator_v2_wire_conversion_covers_backplate_without_exposing_parts() {
        use crate::core::ensemble::decorators::{BackplateFit, BackplateTarget};
        use crate::core::ensemble::types::DecoratorConfig;
        use ruvie_plugin_api::{BackplateFitV2, BackplateOffsetV2, InsetsV2};

        let output = decorator_config_from_wire_v2(DecoratorOutputV2::Backplate {
            target: DecoratorTargetV2::Char,
            padding: InsetsV2 {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            },
            offset: BackplateOffsetV2 { x: 5.0, y: -6.0 },
            fit: BackplateFitV2::Cover,
        })
        .expect("finite Backplate converts")
        .expect("Backplate produces a config");
        assert_eq!(
            output,
            DecoratorConfig::Backplate {
                target: BackplateTarget::Char,
                padding: (1.0, 2.0, 3.0, 4.0),
                offset: (5.0, -6.0),
                fit: BackplateFit::Cover,
            }
        );

        assert!(
            decorator_config_from_wire_v2(DecoratorOutputV2::Backplate {
                target: DecoratorTargetV2::Block,
                padding: InsetsV2 {
                    top: f32::NAN,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
                offset: BackplateOffsetV2 { x: 0.0, y: 0.0 },
                fit: BackplateFitV2::Stretch,
            })
            .is_err(),
            "non-finite Backplate output must not reach the renderer"
        );
        assert!(
            decorator_config_from_response_v2(serde_json::json!({
                "type": "backplate",
                "target": "parts",
                "padding": {"top": 0.0, "right": 0.0, "bottom": 0.0, "left": 0.0},
                "offset": {"x": 0.0, "y": 0.0},
                "fit": "stretch"
            }))
            .is_err(),
            "the unsupported Parts target is not an ABI-v1 config"
        );
        assert!(
            safe_decorator_config_from_response_v2(
                serde_json::json!({"type": "future_decorator"}),
                "test runtime Decorator"
            )
            .is_none(),
            "unknown Decorator output must fail safely as NoOutput"
        );
    }

    #[test]
    fn frozen_decorator_v1_descriptor_and_output_keep_one_shape_appearance() {
        use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
        use crate::core::ensemble::types::DecoratorConfig;

        let component = legacy_decorator_component();
        validate_descriptor(&descriptor_with(component.clone()))
            .expect("frozen v1 descriptor and appearance properties remain valid");
        assert_eq!(
            RuntimeDecoratorProtocol::negotiate(&component),
            Some(RuntimeDecoratorProtocol::V1)
        );
        let operation = runtime_decorator_for_test(component)
            .descriptor()
            .expect("v1 operation descriptor is valid");
        assert!(
            !operation
                .declared_ports()
                .iter()
                .any(|port| { port.key == crate::model::project::BACKGROUND_SHAPE_INPUT_PORT })
        );

        let output = decorator_config_from_response(serde_json::json!({
            "type": "backplate",
            "target": "line",
            "shape": "rounded_rect",
            "color": {"r": 10, "g": 20, "b": 30, "a": 40},
            "padding": {"top": -1.0, "right": 2.0, "bottom": 3.0, "left": 4.0},
            "corner_radius": 5.0
        }))
        .expect("frozen v1 output parses")
        .expect("v1 Backplate produces legacy config");
        assert_eq!(
            output,
            DecoratorConfig::LegacyBackplate {
                target: BackplateTarget::Line,
                shape: BackplateShape::RoundedRect,
                color: crate::model::frame::color::Color {
                    r: 10,
                    g: 20,
                    b: 30,
                    a: 40,
                },
                padding: (-1.0, 2.0, 3.0, 4.0),
                corner_radius: 5.0,
            }
        );
    }

    fn runtime_decorator_for_test(component: ComponentDescriptorV1) -> RuntimeDecoratorPlugin {
        let definitions = property_definitions(&component).expect("test properties are valid");
        let protocol = RuntimeDecoratorProtocol::negotiate(&component)
            .expect("test decorator advertises a supported protocol");
        let pending = pending_bundle(descriptor_with(component.clone()));
        RuntimeDecoratorPlugin {
            component: RuntimeComponent {
                descriptor: component,
                library: pending.library,
            },
            definitions,
            protocol,
        }
    }

    #[test]
    fn v2_only_decorator_uses_two_shape_descriptor() {
        let component = decorator_component();
        validate_descriptor(&descriptor_with(component.clone()))
            .expect("v2-only Decorator descriptor is valid");
        assert_eq!(
            RuntimeDecoratorProtocol::negotiate(&component),
            Some(RuntimeDecoratorProtocol::V2)
        );
        let descriptor = runtime_decorator_for_test(component)
            .descriptor()
            .expect("v2 operation descriptor is valid");
        assert!(descriptor.declared_ports().iter().any(|port| {
            port.key == crate::model::project::BACKGROUND_SHAPE_INPUT_PORT
                && port.direction == crate::model::project::PortDirection::Input
        }));
    }

    #[test]
    fn dual_decorator_prefers_v2_regardless_of_operation_order() {
        for operations in [
            vec![
                DECORATOR_EVALUATE_V1.to_string(),
                DECORATOR_EVALUATE_V2.to_string(),
            ],
            vec![
                DECORATOR_EVALUATE_V2.to_string(),
                DECORATOR_EVALUATE_V1.to_string(),
            ],
        ] {
            let mut component = decorator_component();
            component.operations = operations;
            assert_eq!(
                RuntimeDecoratorProtocol::negotiate(&component),
                Some(RuntimeDecoratorProtocol::V2)
            );
            let descriptor = runtime_decorator_for_test(component)
                .descriptor()
                .expect("dual Decorator negotiates a descriptor");
            assert!(
                descriptor
                    .declared_ports()
                    .iter()
                    .any(|port| { port.key == crate::model::project::BACKGROUND_SHAPE_INPUT_PORT })
            );
        }
    }

    #[test]
    fn malformed_v2_output_is_rejected_before_host_dispatch() {
        for malformed in [
            serde_json::json!({
                "type": "backplate",
                "target": "block",
                "padding": {"top": 0.0, "right": 0.0, "bottom": 0.0, "left": 0.0},
                "offset": {"x": 0.0, "y": 0.0},
                "fit": "stretch",
                "color": {"r": 0, "g": 0, "b": 0, "a": 255}
            }),
            serde_json::json!({
                "type": "backplate",
                "target": "block",
                "padding": {"top": 0.0, "right": 0.0, "bottom": 0.0, "left": 0.0},
                "offset": {"x": 0.0, "y": 0.0, "z": 0.0},
                "fit": "stretch"
            }),
        ] {
            assert!(decorator_config_from_response_v2(malformed).is_err());
        }
    }

    #[test]
    fn accepted_style_configs_execute_in_skia() {
        use skia_safe::{Paint, PaintStyle, PathBuilder};

        let mut surface = skia_safe::surfaces::raster_n32_premul((32, 32))
            .expect("create a CPU Skia surface for runtime config validation");
        surface.canvas().clear(skia_safe::Color::TRANSPARENT);

        let mut stroke = Paint::default();
        stroke.set_anti_alias(true);
        stroke.set_color(skia_safe::Color::WHITE);
        stroke.set_style(PaintStyle::Stroke);
        stroke.set_stroke_width(2.0);
        let dash = skia_safe::PathEffect::dash(&[2.0, 1.0], 0.5)
            .expect("accepted dash config constructs a Skia path effect");
        stroke.set_path_effect(dash);
        let mut path_builder = PathBuilder::new();
        path_builder.move_to((2.0, 6.0));
        path_builder.line_to((30.0, 6.0));
        let path = path_builder.detach();
        surface.canvas().draw_path(&path, &stroke);

        let image = crate::core::rendering::skia_utils::surface_to_image(&mut surface, 32, 32)
            .expect("read pixels rendered by accepted runtime configs");
        assert!(
            image.data.chunks_exact(4).any(|pixel| pixel[3] != 0),
            "accepted configs must produce visible Skia output"
        );
    }

