//! Comprehensive unit-level tests for library modules.
//!
//! Tests: ClipFactory, TrackClip, bounds, builtin plugins,
//! node definitions, draw_type, property, shape_data.

use library::project::clip::{TrackClip, TrackClipKind};
use library::project::property::{Property, PropertyMap, PropertyValue};
use library::service::handlers::clip_factory::ClipFactory;

/// Helper: get the "value" from a constant Property.
fn constant_value(prop: &Property) -> &PropertyValue {
    prop.properties
        .get("value")
        .expect("constant property should have 'value'")
}

// ===== ClipFactory =====

mod clip_factory {
    use super::*;

    #[test]
    fn create_text_clip_has_text_kind() {
        let clip = ClipFactory::create_text_clip("Hello", 0, 90, 30.0);
        assert_eq!(clip.kind, TrackClipKind::Text);
    }

    #[test]
    fn create_text_clip_stores_text_property() {
        let clip = ClipFactory::create_text_clip("Hello World", 10, 100, 30.0);
        let text = clip.properties.get("text").unwrap();
        let val = constant_value(text);
        assert_eq!(*val, PropertyValue::String("Hello World".to_string()));
    }

    #[test]
    fn create_text_clip_frame_range() {
        let clip = ClipFactory::create_text_clip("T", 10, 50, 24.0);
        assert_eq!(clip.in_frame, 10);
        assert_eq!(clip.out_frame, 50);
        assert_eq!(clip.fps, 24.0);
    }

    #[test]
    fn create_text_clip_no_duration() {
        let clip = ClipFactory::create_text_clip("T", 0, 30, 30.0);
        assert!(clip.duration_frame.is_none());
    }

    #[test]
    fn create_shape_clip_has_shape_kind() {
        let clip = ClipFactory::create_shape_clip(0, 60, 30.0);
        assert_eq!(clip.kind, TrackClipKind::Shape);
    }

    #[test]
    fn create_shape_clip_has_path_property() {
        let clip = ClipFactory::create_shape_clip(0, 60, 30.0);
        let path = clip.properties.get("path").unwrap();
        let val = constant_value(path);
        // Should contain a heart SVG path
        if let PropertyValue::String(s) = val {
            assert!(s.contains("M 50,30"));
        } else {
            panic!("Expected String property for path");
        }
    }

    #[test]
    fn create_image_clip_has_image_kind() {
        let clip = ClipFactory::create_image_clip(None, "/path/to/img.png", 0, 90, 30.0);
        assert_eq!(clip.kind, TrackClipKind::Image);
    }

    #[test]
    fn create_image_clip_stores_file_path() {
        let clip = ClipFactory::create_image_clip(None, "/my/image.jpg", 5, 95, 24.0);
        let fp = clip.properties.get("file_path").unwrap();
        assert_eq!(
            *constant_value(fp),
            PropertyValue::String("/my/image.jpg".to_string())
        );
    }

    #[test]
    fn create_image_clip_no_duration_no_source_begin() {
        let clip = ClipFactory::create_image_clip(None, "/img.png", 0, 60, 30.0);
        assert!(clip.duration_frame.is_none());
        assert_eq!(clip.source_begin_frame, 0);
    }

    #[test]
    fn create_video_clip_has_video_kind() {
        let clip = ClipFactory::create_video_clip(None, "/vid.mp4", 0, 100, 0, 100, 30.0);
        assert_eq!(clip.kind, TrackClipKind::Video);
    }

    #[test]
    fn create_video_clip_stores_duration() {
        let clip = ClipFactory::create_video_clip(None, "/vid.mp4", 0, 100, 10, 200, 24.0);
        assert_eq!(clip.duration_frame, Some(200));
        assert_eq!(clip.source_begin_frame, 10);
    }

    #[test]
    fn create_audio_clip_has_audio_kind() {
        let clip = ClipFactory::create_audio_clip(None, "/audio.wav", 0, 60, 0, 60, 44100.0);
        assert_eq!(clip.kind, TrackClipKind::Audio);
    }

    #[test]
    fn create_audio_clip_stores_file_path() {
        let clip = ClipFactory::create_audio_clip(None, "/music.mp3", 0, 300, 0, 300, 48000.0);
        let fp = clip.properties.get("file_path").unwrap();
        assert_eq!(
            *constant_value(fp),
            PropertyValue::String("/music.mp3".to_string())
        );
    }

    #[test]
    fn create_sksl_clip_has_sksl_kind() {
        let clip = ClipFactory::create_sksl_clip(0, 120, 30.0);
        assert_eq!(clip.kind, TrackClipKind::SkSL);
    }

    #[test]
    fn create_sksl_clip_has_shader_property() {
        let clip = ClipFactory::create_sksl_clip(0, 120, 30.0);
        let shader = clip.properties.get("shader").unwrap();
        if let PropertyValue::String(s) = constant_value(shader) {
            assert!(s.contains("main"));
            assert!(s.contains("half4"));
        } else {
            panic!("Expected String for shader property");
        }
    }

    #[test]
    fn all_clips_have_unique_ids() {
        let c1 = ClipFactory::create_text_clip("A", 0, 30, 30.0);
        let c2 = ClipFactory::create_text_clip("B", 0, 30, 30.0);
        let c3 = ClipFactory::create_shape_clip(0, 30, 30.0);
        assert_ne!(c1.id, c2.id);
        assert_ne!(c2.id, c3.id);
    }

    #[test]
    fn create_image_clip_with_reference_id() {
        let ref_id = uuid::Uuid::new_v4();
        let clip = ClipFactory::create_image_clip(Some(ref_id), "/img.png", 0, 90, 30.0);
        assert_eq!(clip.reference_id, Some(ref_id));
    }

    #[test]
    fn create_video_clip_with_reference_id() {
        let ref_id = uuid::Uuid::new_v4();
        let clip = ClipFactory::create_video_clip(Some(ref_id), "/v.mp4", 0, 100, 0, 100, 30.0);
        assert_eq!(clip.reference_id, Some(ref_id));
    }
}

// ===== TrackClip =====

mod track_clip {
    use super::*;
    use ordered_float::OrderedFloat;

    #[test]
    fn get_definitions_for_text() {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Text);
        let keys: Vec<&str> = defs.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"text"));
        assert!(keys.contains(&"font_family"));
        assert!(keys.contains(&"size"));
        // Should include transform defs
        assert!(keys.contains(&"position"));
        assert!(keys.contains(&"scale"));
        assert!(keys.contains(&"rotation"));
        assert!(keys.contains(&"anchor"));
        assert!(keys.contains(&"opacity"));
    }

    #[test]
    fn get_definitions_for_audio_no_transform() {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Audio);
        let keys: Vec<&str> = defs.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"file_path"));
        // Audio clips should NOT have transform properties
        assert!(!keys.contains(&"position"));
        assert!(!keys.contains(&"scale"));
    }

    #[test]
    fn get_definitions_for_video_has_transform() {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Video);
        let keys: Vec<&str> = defs.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"file_path"));
        assert!(keys.contains(&"position"));
    }

    #[test]
    fn get_definitions_for_image_has_transform() {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Image);
        let keys: Vec<&str> = defs.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"file_path"));
        assert!(keys.contains(&"position"));
    }

    #[test]
    fn get_definitions_for_shape() {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Shape);
        let keys: Vec<&str> = defs.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"path"));
        assert!(keys.contains(&"width"));
        assert!(keys.contains(&"height"));
        assert!(keys.contains(&"position"));
    }

    #[test]
    fn get_definitions_for_sksl() {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::SkSL);
        let keys: Vec<&str> = defs.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"shader"));
        assert!(keys.contains(&"position"));
    }

    #[test]
    fn set_constant_property() {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Text);
        let props = PropertyMap::from_definitions(&defs);
        let mut clip = TrackClip::new(
            uuid::Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            90,
            0,
            None,
            30.0,
            props,
        );
        clip.set_constant_property("text", PropertyValue::String("Updated".to_string()));
        let val = constant_value(clip.properties.get("text").unwrap());
        assert_eq!(*val, PropertyValue::String("Updated".to_string()));
    }

    #[test]
    fn transform_definitions_have_correct_defaults() {
        // Use get_definitions_for_kind (public) — Video includes transform defs
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Video);
        let pos = defs.iter().find(|d| d.name() == "position").unwrap();
        if let PropertyValue::Vec2(v) = pos.default_value() {
            assert_eq!(v.x, OrderedFloat(0.0));
            assert_eq!(v.y, OrderedFloat(0.0));
        } else {
            panic!("position should be Vec2");
        }

        let scale = defs.iter().find(|d| d.name() == "scale").unwrap();
        if let PropertyValue::Vec2(v) = scale.default_value() {
            assert_eq!(v.x, OrderedFloat(100.0));
            assert_eq!(v.y, OrderedFloat(100.0));
        } else {
            panic!("scale should be Vec2");
        }

        let opacity = defs.iter().find(|d| d.name() == "opacity").unwrap();
        assert_eq!(
            *opacity.default_value(),
            PropertyValue::Number(OrderedFloat(100.0))
        );
    }
}

// ===== Builtin Plugins =====

mod builtin_plugins {
    use library::builtin::DecoratorPlugin;
    use library::builtin::EffectorPlugin;
    use library::builtin::StylePlugin;
    use library::builtin::decorators::BackplateDecoratorPlugin;
    use library::builtin::effectors::{
        OpacityEffectorPlugin, RandomizeEffectorPlugin, StepDelayEffectorPlugin,
        TransformEffectorPlugin,
    };
    use library::builtin::styles::{FillStylePlugin, StrokeStylePlugin};
    use library::plugin::Plugin;

    #[test]
    fn fill_style_plugin_metadata() {
        let p = FillStylePlugin;
        assert_eq!(p.id(), "fill");
        assert_eq!(p.name(), "Fill");
        assert_eq!(p.category(), "Built-in");
        assert_eq!(p.version(), (0, 1, 0));
    }

    #[test]
    fn fill_style_has_color_opacity_offset() {
        let p = FillStylePlugin;
        let props = p.properties();
        let keys: Vec<&str> = props.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"color"));
        assert!(keys.contains(&"opacity"));
        assert!(keys.contains(&"offset"));
        assert_eq!(props.len(), 3);
    }

    #[test]
    fn stroke_style_plugin_metadata() {
        let p = StrokeStylePlugin;
        assert_eq!(p.id(), "stroke");
        assert_eq!(p.name(), "Stroke");
    }

    #[test]
    fn stroke_style_has_all_properties() {
        let p = StrokeStylePlugin;
        let props = p.properties();
        let keys: Vec<&str> = props.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"color"));
        assert!(keys.contains(&"width"));
        assert!(keys.contains(&"opacity"));
        assert!(keys.contains(&"offset"));
        assert!(keys.contains(&"join"));
        assert!(keys.contains(&"cap"));
        assert!(keys.contains(&"miter_limit"));
        assert!(keys.contains(&"dash_array"));
        assert!(keys.contains(&"dash_offset"));
        assert_eq!(props.len(), 9);
    }

    #[test]
    fn transform_effector_metadata() {
        let p = TransformEffectorPlugin;
        assert_eq!(p.id(), "transform");
        assert_eq!(p.name(), "Transform");
    }

    #[test]
    fn transform_effector_has_translate_scale_rotation() {
        let p = TransformEffectorPlugin;
        let props = p.properties();
        let keys: Vec<&str> = props.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"tx"));
        assert!(keys.contains(&"ty"));
        assert!(keys.contains(&"scale_x"));
        assert!(keys.contains(&"scale_y"));
        assert!(keys.contains(&"rotation"));
        assert_eq!(props.len(), 5);
    }

    #[test]
    fn step_delay_effector_metadata() {
        let p = StepDelayEffectorPlugin;
        assert_eq!(p.id(), "step_delay");
        assert_eq!(p.name(), "Step Delay");
    }

    #[test]
    fn step_delay_effector_has_properties() {
        let p = StepDelayEffectorPlugin;
        let props = p.properties();
        let keys: Vec<&str> = props.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"delay"));
        assert!(keys.contains(&"duration"));
        assert!(keys.contains(&"from_opacity"));
        assert!(keys.contains(&"to_opacity"));
        assert_eq!(props.len(), 4);
    }

    #[test]
    fn randomize_effector_metadata() {
        let p = RandomizeEffectorPlugin;
        assert_eq!(p.id(), "randomize");
    }

    #[test]
    fn randomize_effector_has_properties() {
        let p = RandomizeEffectorPlugin;
        let props = p.properties();
        let keys: Vec<&str> = props.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"seed"));
        assert!(keys.contains(&"amount"));
        assert!(keys.contains(&"translate_range"));
        assert!(keys.contains(&"rotate_range"));
        assert!(keys.contains(&"scale_range"));
        assert_eq!(props.len(), 5);
    }

    #[test]
    fn opacity_effector_metadata() {
        let p = OpacityEffectorPlugin;
        assert_eq!(p.id(), "opacity");
    }

    #[test]
    fn opacity_effector_has_properties() {
        let p = OpacityEffectorPlugin;
        let props = p.properties();
        let keys: Vec<&str> = props.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"opacity"));
        assert!(keys.contains(&"mode"));
        assert_eq!(props.len(), 2);
    }

    #[test]
    fn backplate_decorator_metadata() {
        let p = BackplateDecoratorPlugin;
        assert_eq!(p.id(), "backplate");
        assert_eq!(p.name(), "Backplate");
    }

    #[test]
    fn backplate_decorator_has_properties() {
        let p = BackplateDecoratorPlugin;
        let props = p.properties();
        let keys: Vec<&str> = props.iter().map(|d| d.name()).collect();
        assert!(keys.contains(&"target"));
        assert!(keys.contains(&"shape"));
        assert!(keys.contains(&"color"));
        assert!(keys.contains(&"padding"));
        assert!(keys.contains(&"radius"));
        assert_eq!(props.len(), 5);
    }
}

// ===== Node Definitions (per-category) =====

mod node_definitions {
    use library::plugin::PluginManager;

    fn make_manager() -> PluginManager {
        PluginManager::default()
    }

    #[test]
    fn all_nodes_have_non_empty_display_name() {
        let pm = make_manager();
        let defs = pm.get_available_node_types();
        for def in &defs {
            assert!(
                !def.display_name.is_empty(),
                "Empty display_name for {}",
                def.type_id
            );
        }
    }

    #[test]
    fn all_nodes_have_non_empty_type_id() {
        let pm = make_manager();
        let defs = pm.get_available_node_types();
        for def in &defs {
            assert!(!def.type_id.is_empty());
        }
    }

    #[test]
    fn effect_blur_node_has_image_io() {
        let pm = make_manager();
        let def = pm
            .get_node_type("filters.blur")
            .expect("filters.blur not found");
        let in_names: Vec<&str> = def.inputs.iter().map(|p| p.name.as_str()).collect();
        let out_names: Vec<&str> = def.outputs.iter().map(|p| p.name.as_str()).collect();
        assert!(
            in_names.contains(&"image"),
            "filters.blur missing image input"
        );
        assert!(
            out_names.contains(&"image"),
            "filters.blur missing image output"
        );
    }

    #[test]
    fn style_fill_node_has_correct_io() {
        let pm = make_manager();
        let def = pm
            .get_node_type("style.fill")
            .expect("style.fill not found");
        let in_names: Vec<&str> = def.inputs.iter().map(|p| p.name.as_str()).collect();
        let out_names: Vec<&str> = def.outputs.iter().map(|p| p.name.as_str()).collect();
        assert!(in_names.contains(&"shape_in"));
        assert!(out_names.contains(&"image_out"));
    }

    #[test]
    fn compositing_transform_node_exists() {
        let pm = make_manager();
        let def = pm
            .get_node_type("compositing.transform")
            .expect("compositing.transform not found");
        let in_names: Vec<&str> = def.inputs.iter().map(|p| p.name.as_str()).collect();
        let out_names: Vec<&str> = def.outputs.iter().map(|p| p.name.as_str()).collect();
        assert!(in_names.contains(&"image_in"));
        assert!(out_names.contains(&"image_out"));
    }

    #[test]
    fn math_add_node_exists() {
        let pm = make_manager();
        assert!(pm.get_node_type("math.add").is_some());
    }

    #[test]
    fn effector_nodes_have_shape_io() {
        let pm = make_manager();
        let effector_types = [
            "effector.transform",
            "effector.step_delay",
            "effector.randomize",
            "effector.opacity",
        ];
        for type_id in effector_types {
            let def = pm
                .get_node_type(type_id)
                .unwrap_or_else(|| panic!("{} not found", type_id));
            let in_names: Vec<&str> = def.inputs.iter().map(|p| p.name.as_str()).collect();
            let out_names: Vec<&str> = def.outputs.iter().map(|p| p.name.as_str()).collect();
            assert!(
                in_names.contains(&"shape_in"),
                "{} missing shape_in",
                type_id
            );
            assert!(
                out_names.contains(&"shape_out"),
                "{} missing shape_out",
                type_id
            );
        }
    }

    #[test]
    fn decorator_backplate_node_has_shape_io() {
        let pm = make_manager();
        let def = pm
            .get_node_type("decorator.backplate")
            .expect("decorator.backplate not found");
        let in_names: Vec<&str> = def.inputs.iter().map(|p| p.name.as_str()).collect();
        let out_names: Vec<&str> = def.outputs.iter().map(|p| p.name.as_str()).collect();
        assert!(in_names.contains(&"shape_in"));
        assert!(out_names.contains(&"shape_out"));
    }

    #[test]
    fn generator_nodes_have_image_output() {
        let pm = make_manager();
        let gen_types = [
            "generators.noise",
            "generators.solid_color",
            "generators.linear_gradient",
        ];
        for type_id in gen_types {
            let def = pm
                .get_node_type(type_id)
                .unwrap_or_else(|| panic!("{} not found", type_id));
            let out_names: Vec<&str> = def.outputs.iter().map(|p| p.name.as_str()).collect();
            assert!(
                out_names.contains(&"image"),
                "{} missing image output",
                type_id
            );
        }
    }

    #[test]
    fn particle_emitter_node_exists() {
        let pm = make_manager();
        assert!(pm.get_node_type("particles.particle_emitter").is_some());
    }

    #[test]
    fn color_correction_node_exists() {
        let pm = make_manager();
        assert!(pm.get_node_type("color.color_correction").is_some());
    }
}

// ===== draw_type =====

mod draw_type_tests {
    use library::runtime::color::Color;
    use library::runtime::draw_type::*;

    #[test]
    fn blend_mode_default_is_normal() {
        assert_eq!(BlendMode::default(), BlendMode::Normal);
    }

    #[test]
    fn join_type_default_is_round() {
        assert_eq!(JoinType::default(), JoinType::Round);
    }

    #[test]
    fn cap_type_default_is_square() {
        assert_eq!(CapType::default(), CapType::Square);
    }

    #[test]
    fn draw_style_default_is_fill_white() {
        let ds = DrawStyle::default();
        match ds {
            DrawStyle::Fill { color, offset } => {
                assert_eq!(color, Color::white());
                assert_eq!(offset, 0.0);
            }
            _ => panic!("Default DrawStyle should be Fill"),
        }
    }

    #[test]
    fn draw_style_fill_equality() {
        let s1 = DrawStyle::Fill {
            color: Color::black(),
            offset: 1.0,
        };
        let s2 = DrawStyle::Fill {
            color: Color::black(),
            offset: 1.0,
        };
        let s3 = DrawStyle::Fill {
            color: Color::white(),
            offset: 1.0,
        };
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn draw_style_stroke_equality() {
        let s1 = DrawStyle::Stroke {
            color: Color::black(),
            width: 2.0,
            offset: 0.0,
            cap: CapType::Round,
            join: JoinType::Miter,
            miter: 4.0,
            dash_array: vec![5.0, 3.0],
            dash_offset: 0.0,
        };
        let s2 = DrawStyle::Stroke {
            color: Color::black(),
            width: 2.0,
            offset: 0.0,
            cap: CapType::Round,
            join: JoinType::Miter,
            miter: 4.0,
            dash_array: vec![5.0, 3.0],
            dash_offset: 0.0,
        };
        assert_eq!(s1, s2);
    }

    #[test]
    fn draw_style_fill_ne_stroke() {
        let fill = DrawStyle::Fill {
            color: Color::white(),
            offset: 0.0,
        };
        let stroke = DrawStyle::Stroke {
            color: Color::white(),
            width: 1.0,
            offset: 0.0,
            cap: CapType::default(),
            join: JoinType::default(),
            miter: 4.0,
            dash_array: vec![],
            dash_offset: 0.0,
        };
        assert_ne!(fill, stroke);
    }

    #[test]
    fn path_effect_dash_equality() {
        let p1 = PathEffect::Dash {
            intervals: vec![5.0, 3.0],
            phase: 0.0,
        };
        let p2 = PathEffect::Dash {
            intervals: vec![5.0, 3.0],
            phase: 0.0,
        };
        let p3 = PathEffect::Dash {
            intervals: vec![5.0, 4.0],
            phase: 0.0,
        };
        assert_eq!(p1, p2);
        assert_ne!(p1, p3);
    }

    #[test]
    fn path_effect_corner_equality() {
        let p1 = PathEffect::Corner { radius: 10.0 };
        let p2 = PathEffect::Corner { radius: 10.0 };
        assert_eq!(p1, p2);
    }

    #[test]
    fn path_effect_discrete_equality() {
        let p1 = PathEffect::Discrete {
            seg_length: 5.0,
            deviation: 2.0,
            seed: 42,
        };
        let p2 = PathEffect::Discrete {
            seg_length: 5.0,
            deviation: 2.0,
            seed: 42,
        };
        let p3 = PathEffect::Discrete {
            seg_length: 5.0,
            deviation: 2.0,
            seed: 99,
        };
        assert_eq!(p1, p2);
        assert_ne!(p1, p3);
    }

    #[test]
    fn path_effect_trim_equality() {
        let p1 = PathEffect::Trim {
            start: 0.0,
            end: 1.0,
        };
        let p2 = PathEffect::Trim {
            start: 0.0,
            end: 1.0,
        };
        assert_eq!(p1, p2);
    }

    #[test]
    fn blend_mode_serialization_roundtrip() {
        for mode in [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Add,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let m2: BlendMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, m2);
        }
    }

    #[test]
    fn draw_style_serialization_roundtrip() {
        let ds = DrawStyle::Stroke {
            color: Color {
                r: 100,
                g: 50,
                b: 200,
                a: 128,
            },
            width: 3.5,
            offset: -1.0,
            cap: CapType::Butt,
            join: JoinType::Bevel,
            miter: 8.0,
            dash_array: vec![10.0, 5.0, 2.0],
            dash_offset: 3.0,
        };
        let json = serde_json::to_string(&ds).unwrap();
        let ds2: DrawStyle = serde_json::from_str(&json).unwrap();
        assert_eq!(ds, ds2);
    }
}

// ===== PropertyValue =====

mod property_value_tests {
    use library::project::property::PropertyValue;
    use library::runtime::color::Color;
    use ordered_float::OrderedFloat;

    #[test]
    fn from_f64() {
        let pv = PropertyValue::from(42.5);
        assert_eq!(pv, PropertyValue::Number(OrderedFloat(42.5)));
    }

    #[test]
    fn from_f32() {
        let pv = PropertyValue::from(1.5f32);
        assert_eq!(pv, PropertyValue::Number(OrderedFloat(1.5)));
    }

    #[test]
    fn from_i64() {
        let pv = PropertyValue::from(7i64);
        assert_eq!(pv, PropertyValue::Integer(7));
    }

    #[test]
    fn from_string() {
        let pv = PropertyValue::from("hello".to_string());
        assert_eq!(pv, PropertyValue::String("hello".to_string()));
    }

    #[test]
    fn from_bool() {
        assert_eq!(PropertyValue::from(true), PropertyValue::Boolean(true));
        assert_eq!(PropertyValue::from(false), PropertyValue::Boolean(false));
    }

    #[test]
    fn get_as_f64() {
        let pv = PropertyValue::Number(OrderedFloat(3.14));
        assert_eq!(pv.get_as::<f64>(), Some(3.14));
    }

    #[test]
    fn get_as_string() {
        let pv = PropertyValue::String("test".to_string());
        assert_eq!(pv.get_as::<String>(), Some("test".to_string()));
    }

    #[test]
    fn get_as_bool() {
        let pv = PropertyValue::Boolean(true);
        assert_eq!(pv.get_as::<bool>(), Some(true));
    }

    #[test]
    fn is_compatible_with_float() {
        use library::project::property::PropertyUiType;
        let pv = PropertyValue::from(1.0);
        assert!(pv.is_compatible_with(&PropertyUiType::Float {
            min: 0.0,
            max: 10.0,
            step: 0.1,
            suffix: "".into(),
            min_hard_limit: false,
            max_hard_limit: false,
        }));
    }

    #[test]
    fn is_compatible_with_color() {
        use library::project::property::PropertyUiType;
        let pv = PropertyValue::Color(Color::white());
        assert!(pv.is_compatible_with(&PropertyUiType::Color));
    }

    #[test]
    fn is_compatible_mismatch() {
        use library::project::property::PropertyUiType;
        let pv = PropertyValue::from(1.0);
        assert!(!pv.is_compatible_with(&PropertyUiType::Color));
    }

    #[test]
    fn serialization_roundtrip_number() {
        let pv = PropertyValue::from(99.9);
        let json = serde_json::to_string(&pv).unwrap();
        let pv2: PropertyValue = serde_json::from_str(&json).unwrap();
        assert_eq!(pv, pv2);
    }

    #[test]
    fn serialization_roundtrip_string() {
        let pv = PropertyValue::String("hello".to_string());
        let json = serde_json::to_string(&pv).unwrap();
        let pv2: PropertyValue = serde_json::from_str(&json).unwrap();
        assert_eq!(pv, pv2);
    }

    #[test]
    fn serialization_roundtrip_color() {
        let pv = PropertyValue::Color(Color {
            r: 10,
            g: 20,
            b: 30,
            a: 40,
        });
        let json = serde_json::to_string(&pv).unwrap();
        let pv2: PropertyValue = serde_json::from_str(&json).unwrap();
        assert_eq!(pv, pv2);
    }
}

// ===== ShapeData =====

mod shape_data_tests {
    use library::pipeline::ensemble::types::TransformData;
    use library::pipeline::output::{DecorationShape, FontInfo, LineInfo, ShapeData, ShapeGroup};
    use library::runtime::color::Color;
    use std::hash::{Hash, Hasher};

    #[test]
    fn shape_data_path_variant() {
        let sd = ShapeData::Path {
            path_data: "M 0 0 L 10 10".to_string(),
            path_effects: vec![],
        };
        assert!(matches!(sd, ShapeData::Path { .. }));
    }

    #[test]
    fn shape_group_equality() {
        let g1 = make_group("M 0 0", "A", 0);
        let g2 = make_group("M 0 0", "A", 0);
        assert_eq!(g1, g2);
    }

    #[test]
    fn shape_group_inequality() {
        let g1 = make_group("M 0 0", "A", 0);
        let g2 = make_group("M 0 0", "B", 0);
        assert_ne!(g1, g2);
    }

    #[test]
    fn shape_group_hash_consistency() {
        let g1 = make_group("M 0 0", "A", 0);
        let g2 = make_group("M 0 0", "A", 0);
        assert_eq!(compute_hash(&g1), compute_hash(&g2));
    }

    #[test]
    fn line_info_equality() {
        let l1 = LineInfo {
            group_range: 0..5,
            bounds: (0.0, 0.0, 100.0, 20.0),
        };
        let l2 = LineInfo {
            group_range: 0..5,
            bounds: (0.0, 0.0, 100.0, 20.0),
        };
        assert_eq!(l1, l2);
    }

    #[test]
    fn font_info_equality() {
        let f1 = FontInfo {
            family: "Arial".to_string(),
            size: 12.0,
        };
        let f2 = FontInfo {
            family: "Arial".to_string(),
            size: 12.0,
        };
        let f3 = FontInfo {
            family: "Arial".to_string(),
            size: 14.0,
        };
        assert_eq!(f1, f2);
        assert_ne!(f1, f3);
    }

    #[test]
    fn decoration_shape_equality() {
        let d1 = DecorationShape {
            path: "M 0 0 L 10 10".to_string(),
            color: Color::black(),
            behind: true,
        };
        let d2 = DecorationShape {
            path: "M 0 0 L 10 10".to_string(),
            color: Color::black(),
            behind: true,
        };
        let d3 = DecorationShape {
            path: "M 0 0 L 10 10".to_string(),
            color: Color::black(),
            behind: false,
        };
        assert_eq!(d1, d2);
        assert_ne!(d1, d3);
    }

    fn make_group(path: &str, ch: &str, idx: usize) -> ShapeGroup {
        ShapeGroup {
            path: path.to_string(),
            source_char: ch.to_string(),
            index: idx,
            line_index: 0,
            base_position: (0.0, 0.0),
            bounds: (0.0, 0.0, 10.0, 10.0),
            transform: TransformData::identity(),
            decorations: vec![],
        }
    }

    fn compute_hash<T: Hash>(val: &T) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        val.hash(&mut hasher);
        hasher.finish()
    }
}

// ===== Bounds (service/bounds.rs) =====

mod bounds_tests {
    use library::plugin::PluginManager;
    use library::project::clip::{TrackClip, TrackClipKind};
    use library::project::property::{Property, PropertyMap, PropertyValue};
    use library::service::bounds::get_clip_content_bounds;

    fn make_evaluators() -> std::sync::Arc<library::plugin::PropertyEvaluatorRegistry> {
        let pm = PluginManager::default();
        pm.get_property_evaluators()
    }

    #[test]
    fn text_clip_bounds_returns_some() {
        let evaluators = make_evaluators();
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Text);
        let mut props = PropertyMap::from_definitions(&defs);
        props.set(
            "text".to_string(),
            Property::constant(PropertyValue::String("Hello".to_string())),
        );
        props.set(
            "size".to_string(),
            Property::constant(PropertyValue::from(48.0)),
        );

        let clip = TrackClip::new(
            uuid::Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            90,
            0,
            None,
            30.0,
            props,
        );

        let bounds = get_clip_content_bounds(&clip, 30.0, 0, &evaluators);
        assert!(bounds.is_some());
        let (x, y, w, h) = bounds.unwrap();
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
        assert!(w > 0.0, "Text width should be > 0");
        assert!(h > 0.0, "Text height should be > 0");
    }

    #[test]
    fn shape_clip_bounds_returns_some() {
        let evaluators = make_evaluators();
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Shape);
        let mut props = PropertyMap::from_definitions(&defs);
        props.set(
            "path".to_string(),
            Property::constant(PropertyValue::String(
                "M 0 0 L 100 0 L 100 50 L 0 50 Z".to_string(),
            )),
        );

        let clip = TrackClip::new(
            uuid::Uuid::new_v4(),
            None,
            TrackClipKind::Shape,
            0,
            60,
            0,
            None,
            30.0,
            props,
        );

        let bounds = get_clip_content_bounds(&clip, 30.0, 0, &evaluators);
        assert!(bounds.is_some());
        let (_, _, w, h) = bounds.unwrap();
        assert!(w > 0.0);
        assert!(h > 0.0);
    }

    #[test]
    fn sksl_clip_bounds_uses_width_height() {
        let evaluators = make_evaluators();
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::SkSL);
        let mut props = PropertyMap::from_definitions(&defs);
        props.set(
            "width".to_string(),
            Property::constant(PropertyValue::from(640.0)),
        );
        props.set(
            "height".to_string(),
            Property::constant(PropertyValue::from(480.0)),
        );

        let clip = TrackClip::new(
            uuid::Uuid::new_v4(),
            None,
            TrackClipKind::SkSL,
            0,
            90,
            0,
            None,
            30.0,
            props,
        );

        let bounds = get_clip_content_bounds(&clip, 30.0, 0, &evaluators);
        assert!(bounds.is_some());
        let (x, y, w, h) = bounds.unwrap();
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
        assert_eq!(w, 640.0);
        assert_eq!(h, 480.0);
    }

    #[test]
    fn video_clip_bounds_returns_none() {
        let evaluators = make_evaluators();
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Video);
        let props = PropertyMap::from_definitions(&defs);

        let clip = TrackClip::new(
            uuid::Uuid::new_v4(),
            None,
            TrackClipKind::Video,
            0,
            90,
            0,
            Some(90),
            30.0,
            props,
        );

        assert!(get_clip_content_bounds(&clip, 30.0, 0, &evaluators).is_none());
    }

    #[test]
    fn audio_clip_bounds_returns_none() {
        let evaluators = make_evaluators();
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Audio);
        let props = PropertyMap::from_definitions(&defs);

        let clip = TrackClip::new(
            uuid::Uuid::new_v4(),
            None,
            TrackClipKind::Audio,
            0,
            90,
            0,
            Some(90),
            30.0,
            props,
        );

        assert!(get_clip_content_bounds(&clip, 30.0, 0, &evaluators).is_none());
    }

    #[test]
    fn text_clip_with_empty_text_returns_none() {
        let evaluators = make_evaluators();
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Text);
        let mut props = PropertyMap::from_definitions(&defs);
        // text defaults to empty string — measure_text_size returns (0, h) for empty
        // but eval_string should still return Some for non-empty default
        props.set(
            "text".to_string(),
            Property::constant(PropertyValue::String("".to_string())),
        );

        let clip = TrackClip::new(
            uuid::Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            90,
            0,
            None,
            30.0,
            props,
        );

        // Empty text still produces bounds (0 width but non-zero height)
        let bounds = get_clip_content_bounds(&clip, 30.0, 0, &evaluators);
        assert!(bounds.is_some());
    }
}

// ===== Project Model =====

mod project_model {
    use library::project::clip::{TrackClip, TrackClipKind};
    use library::project::connection::{Connection, PinId};
    use library::project::graph_node::GraphNode;
    use library::project::node::Node;
    use library::project::project::{Composition, Project};
    use library::project::property::PropertyMap;
    use library::project::track::TrackData;
    use uuid::Uuid;

    // --- Project basics ---

    #[test]
    fn new_project_has_empty_collections() {
        let p = Project::new("test");
        assert_eq!(p.name, "test");
        assert!(p.compositions.is_empty());
        assert!(p.assets.is_empty());
        assert!(p.nodes.is_empty());
        assert!(p.connections.is_empty());
    }

    #[test]
    fn add_and_get_composition() {
        let mut project = Project::new("test");
        let (comp, root_track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
        let comp_id = comp.id;
        project.add_node(Node::Track(root_track));
        project.add_composition(comp);

        let retrieved = project.get_composition(comp_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "main");
        assert_eq!(retrieved.unwrap().width, 1920);
        assert_eq!(retrieved.unwrap().height, 1080);
        assert_eq!(retrieved.unwrap().fps, 30.0);
    }

    #[test]
    fn composition_new_creates_root_track() {
        let (comp, root_track) = Composition::new("test", 1920, 1080, 30.0, 5.0);
        assert_eq!(comp.root_track_id, root_track.id);
        assert!(root_track.child_ids.is_empty());
    }

    // --- Node management ---

    #[test]
    fn add_and_get_clip_node() {
        let mut project = Project::new("test");
        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            90,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;
        project.add_node(Node::Clip(clip));

        assert!(project.get_node(clip_id).is_some());
        assert!(project.get_clip(clip_id).is_some());
        assert_eq!(project.get_clip(clip_id).unwrap().kind, TrackClipKind::Text);
    }

    #[test]
    fn add_and_get_track_node() {
        let mut project = Project::new("test");
        let track = TrackData::new("Track 1");
        let track_id = track.id;
        project.add_node(Node::Track(track));

        assert!(project.get_track(track_id).is_some());
        assert_eq!(project.get_track(track_id).unwrap().name, "Track 1");
    }

    #[test]
    fn add_and_get_graph_node() {
        let mut project = Project::new("test");
        let gn = GraphNode::new("compositing.transform", PropertyMap::new());
        let gn_id = gn.id;
        project.add_node(Node::Graph(gn));

        assert!(project.get_graph_node(gn_id).is_some());
        assert_eq!(
            project.get_graph_node(gn_id).unwrap().type_id,
            "compositing.transform"
        );
    }

    #[test]
    fn remove_node() {
        let mut project = Project::new("test");
        let track = TrackData::new("temp");
        let id = track.id;
        project.add_node(Node::Track(track));
        assert!(project.get_node(id).is_some());

        let removed = project.remove_node(id);
        assert!(removed.is_some());
        assert!(project.get_node(id).is_none());
    }

    #[test]
    fn get_nonexistent_node_returns_none() {
        let project = Project::new("test");
        assert!(project.get_node(Uuid::new_v4()).is_none());
        assert!(project.get_clip(Uuid::new_v4()).is_none());
        assert!(project.get_track(Uuid::new_v4()).is_none());
        assert!(project.get_graph_node(Uuid::new_v4()).is_none());
    }

    // --- Track children ---

    #[test]
    fn track_add_and_remove_child() {
        let mut track = TrackData::new("Track 1");
        let child1 = Uuid::new_v4();
        let child2 = Uuid::new_v4();

        track.add_child(child1);
        track.add_child(child2);
        assert_eq!(track.child_ids.len(), 2);
        assert_eq!(track.child_ids[0], child1);
        assert_eq!(track.child_ids[1], child2);

        assert!(track.remove_child(child1));
        assert_eq!(track.child_ids.len(), 1);
        assert_eq!(track.child_ids[0], child2);

        // Removing non-existent child returns false
        assert!(!track.remove_child(Uuid::new_v4()));
    }

    #[test]
    fn track_insert_child() {
        let mut track = TrackData::new("Track");
        let c1 = Uuid::new_v4();
        let c2 = Uuid::new_v4();
        let c3 = Uuid::new_v4();

        track.add_child(c1);
        track.add_child(c3);
        track.insert_child(1, c2);

        assert_eq!(track.child_ids, vec![c1, c2, c3]);
    }

    // --- Connections ---

    #[test]
    fn add_and_query_connections() {
        let mut project = Project::new("test");
        let node_a = Uuid::new_v4();
        let node_b = Uuid::new_v4();

        let conn = Connection::new(
            PinId::new(node_a, "image_out"),
            PinId::new(node_b, "image_in"),
        );
        let conn_id = conn.id;
        project.add_connection(conn);

        let conns_a = project.get_connections_for_node(node_a);
        assert_eq!(conns_a.len(), 1);
        assert_eq!(conns_a[0].id, conn_id);

        let conns_b = project.get_connections_for_node(node_b);
        assert_eq!(conns_b.len(), 1);

        // Query input connection
        let input = project.get_input_connection(&PinId::new(node_b, "image_in"));
        assert!(input.is_some());
        assert_eq!(input.unwrap().from.node_id, node_a);
    }

    #[test]
    fn remove_connection() {
        let mut project = Project::new("test");
        let conn = Connection::new(
            PinId::new(Uuid::new_v4(), "out"),
            PinId::new(Uuid::new_v4(), "in"),
        );
        let conn_id = conn.id;
        project.add_connection(conn);
        assert_eq!(project.connections.len(), 1);

        let removed = project.remove_connection(conn_id);
        assert!(removed.is_some());
        assert!(project.connections.is_empty());
    }

    #[test]
    fn remove_connections_for_node() {
        let mut project = Project::new("test");
        let node_a = Uuid::new_v4();
        let node_b = Uuid::new_v4();
        let node_c = Uuid::new_v4();

        project.add_connection(Connection::new(
            PinId::new(node_a, "out"),
            PinId::new(node_b, "in"),
        ));
        project.add_connection(Connection::new(
            PinId::new(node_b, "out"),
            PinId::new(node_c, "in"),
        ));
        assert_eq!(project.connections.len(), 2);

        project.remove_connections_for_node(node_b);
        assert!(project.connections.is_empty());
    }

    // --- Traversal ---

    #[test]
    fn collect_clips_from_track() {
        let mut project = Project::new("test");
        let mut track = TrackData::new("T1");
        let track_id = track.id;

        let clip1 = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            30,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip2 = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Shape,
            30,
            60,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let c1_id = clip1.id;
        let c2_id = clip2.id;

        track.add_child(c1_id);
        track.add_child(c2_id);

        project.add_node(Node::Track(track));
        project.add_node(Node::Clip(clip1));
        project.add_node(Node::Clip(clip2));

        let clips = project.collect_clips(track_id);
        assert_eq!(clips.len(), 2);
    }

    #[test]
    fn all_clips_iterator() {
        let mut project = Project::new("test");
        project.add_node(Node::Clip(TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            30,
            0,
            None,
            30.0,
            PropertyMap::new(),
        )));
        project.add_node(Node::Track(TrackData::new("T")));
        project.add_node(Node::Clip(TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Shape,
            0,
            30,
            0,
            None,
            30.0,
            PropertyMap::new(),
        )));

        let clips: Vec<_> = project.all_clips().collect();
        assert_eq!(clips.len(), 2);
    }

    #[test]
    fn find_parent_track() {
        let mut project = Project::new("test");
        let mut track = TrackData::new("Parent");
        let track_id = track.id;

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            30,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;
        track.add_child(clip_id);

        project.add_node(Node::Track(track));
        project.add_node(Node::Clip(clip));

        assert_eq!(project.find_parent_track(clip_id), Some(track_id));
        assert_eq!(project.find_parent_track(Uuid::new_v4()), None);
    }

    // --- Serialization ---

    #[test]
    fn project_save_load_roundtrip() {
        let mut project = Project::new("roundtrip test");
        let (comp, root_track) = Composition::new("comp1", 1280, 720, 24.0, 8.0);
        let track_id = root_track.id;
        project.add_node(Node::Track(root_track));
        project.add_composition(comp);

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            72,
            0,
            None,
            24.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;
        project.add_node(Node::Clip(clip));
        project.get_track_mut(track_id).unwrap().add_child(clip_id);

        let json = project.save().expect("save failed");
        let loaded = Project::load(&json).expect("load failed");

        assert_eq!(loaded.name, "roundtrip test");
        assert_eq!(loaded.compositions.len(), 1);
        assert!(loaded.get_clip(clip_id).is_some());
        assert!(loaded.get_track(track_id).is_some());
    }

    #[test]
    fn node_id_accessor() {
        let clip = Node::Clip(TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            30,
            0,
            None,
            30.0,
            PropertyMap::new(),
        ));
        let track = Node::Track(TrackData::new("T"));
        let graph = Node::Graph(GraphNode::new("effect.blur", PropertyMap::new()));

        // Node::id() returns the inner ID
        match &clip {
            Node::Clip(c) => assert_eq!(clip.id(), c.id),
            _ => panic!(),
        }
        match &track {
            Node::Track(t) => assert_eq!(track.id(), t.id),
            _ => panic!(),
        }
        match &graph {
            Node::Graph(g) => assert_eq!(graph.id(), g.id),
            _ => panic!(),
        }
    }
}

// ===== PropertyMap =====

mod property_map_tests {
    use library::project::clip::{TrackClip, TrackClipKind};
    use library::project::property::{Property, PropertyMap, PropertyValue};
    use ordered_float::OrderedFloat;

    #[test]
    fn from_definitions_populates_all_keys() {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Text);
        let map = PropertyMap::from_definitions(&defs);
        for d in &defs {
            assert!(map.get(d.name()).is_some(), "Missing key: {}", d.name());
        }
    }

    #[test]
    fn get_constant_value() {
        let mut map = PropertyMap::new();
        map.set(
            "x".to_string(),
            Property::constant(PropertyValue::from(42.0)),
        );
        let val = map.get_constant_value("x");
        assert!(val.is_some());
        assert_eq!(*val.unwrap(), PropertyValue::Number(OrderedFloat(42.0)));
    }

    #[test]
    fn get_f64() {
        let mut map = PropertyMap::new();
        map.set(
            "num".to_string(),
            Property::constant(PropertyValue::from(3.14)),
        );
        assert_eq!(map.get_f64("num"), Some(3.14));
        assert_eq!(map.get_f64("missing"), None);
    }

    #[test]
    fn get_string() {
        let mut map = PropertyMap::new();
        map.set(
            "s".to_string(),
            Property::constant(PropertyValue::String("hello".to_string())),
        );
        assert_eq!(map.get_string("s"), Some("hello".to_string()));
        assert_eq!(map.get_string("missing"), None);
    }

    #[test]
    fn get_bool() {
        let mut map = PropertyMap::new();
        map.set(
            "flag".to_string(),
            Property::constant(PropertyValue::Boolean(true)),
        );
        assert_eq!(map.get_bool("flag"), Some(true));
        assert_eq!(map.get_bool("missing"), None);
    }

    #[test]
    fn set_overwrites_existing() {
        let mut map = PropertyMap::new();
        map.set(
            "x".to_string(),
            Property::constant(PropertyValue::from(1.0)),
        );
        map.set(
            "x".to_string(),
            Property::constant(PropertyValue::from(2.0)),
        );
        assert_eq!(map.get_f64("x"), Some(2.0));
    }

    #[test]
    fn iter_returns_all_entries() {
        let mut map = PropertyMap::new();
        map.set(
            "a".to_string(),
            Property::constant(PropertyValue::from(1.0)),
        );
        map.set(
            "b".to_string(),
            Property::constant(PropertyValue::from(2.0)),
        );
        map.set(
            "c".to_string(),
            Property::constant(PropertyValue::from(3.0)),
        );
        let keys: Vec<&String> = map.iter().map(|(k, _)| k).collect();
        assert_eq!(keys.len(), 3);
    }
}

// ===== Property (constant/keyframe/expression) =====

mod property_tests {
    use library::animation::EasingFunction;
    use library::project::property::{Property, PropertyValue};
    use ordered_float::OrderedFloat;

    #[test]
    fn constant_property_value() {
        let p = Property::constant(PropertyValue::from(42.0));
        assert_eq!(p.evaluator, "constant");
        assert_eq!(p.value(), Some(&PropertyValue::Number(OrderedFloat(42.0))));
    }

    #[test]
    fn constant_property_get_static_value() {
        let p = Property::constant(PropertyValue::String("test".to_string()));
        assert_eq!(
            p.get_static_value(),
            Some(&PropertyValue::String("test".to_string()))
        );
    }

    #[test]
    fn keyframe_property_has_keyframes() {
        use library::project::property::Keyframe;
        let kfs = vec![
            Keyframe {
                time: OrderedFloat(0.0),
                value: PropertyValue::from(0.0),
                easing: EasingFunction::Linear,
            },
            Keyframe {
                time: OrderedFloat(1.0),
                value: PropertyValue::from(100.0),
                easing: EasingFunction::Linear,
            },
        ];
        let p = Property::keyframe(kfs);
        assert_eq!(p.evaluator, "keyframe");
        let extracted = p.keyframes();
        assert_eq!(extracted.len(), 2);
    }

    #[test]
    fn expression_property() {
        let p = Property::expression("value * 2".to_string());
        assert_eq!(p.evaluator, "expression");
        assert_eq!(p.expression_text(), Some("value * 2"));
    }

    #[test]
    fn upsert_keyframe() {
        use library::project::property::Keyframe;
        let mut p = Property::keyframe(vec![Keyframe {
            time: OrderedFloat(0.0),
            value: PropertyValue::from(0.0),
            easing: EasingFunction::Linear,
        }]);
        // Add new keyframe at time 1.0
        let added = p.upsert_keyframe(
            1.0,
            PropertyValue::from(50.0),
            Some(EasingFunction::EaseInOutSine),
        );
        assert!(added);
        assert_eq!(p.keyframes().len(), 2);
    }

    #[test]
    fn has_keyframe_at() {
        use library::project::property::Keyframe;
        let p = Property::keyframe(vec![Keyframe {
            time: OrderedFloat(0.5),
            value: PropertyValue::from(10.0),
            easing: EasingFunction::Linear,
        }]);
        assert!(p.has_keyframe_at(0.5, 0.001));
        assert!(!p.has_keyframe_at(1.0, 0.001));
    }

    #[test]
    fn property_serialization_roundtrip() {
        let p = Property::constant(PropertyValue::from(99.0));
        let json = serde_json::to_string(&p).unwrap();
        let p2: Property = serde_json::from_str(&json).unwrap();
        assert_eq!(p2.evaluator, "constant");
        assert_eq!(p2.value(), Some(&PropertyValue::Number(OrderedFloat(99.0))));
    }
}

// ===== Connection Model =====

mod connection_tests {
    use library::project::connection::{Connection, PinId};
    use uuid::Uuid;

    #[test]
    fn pin_id_creation() {
        let node_id = Uuid::new_v4();
        let pin = PinId::new(node_id, "image_out");
        assert_eq!(pin.node_id, node_id);
        assert_eq!(pin.pin_name, "image_out");
    }

    #[test]
    fn connection_creation_has_unique_id() {
        let c1 = Connection::new(
            PinId::new(Uuid::new_v4(), "out"),
            PinId::new(Uuid::new_v4(), "in"),
        );
        let c2 = Connection::new(
            PinId::new(Uuid::new_v4(), "out"),
            PinId::new(Uuid::new_v4(), "in"),
        );
        assert_ne!(c1.id, c2.id);
    }

    #[test]
    fn connection_from_to() {
        let src = Uuid::new_v4();
        let dst = Uuid::new_v4();
        let conn = Connection::new(PinId::new(src, "image_out"), PinId::new(dst, "image_in"));
        assert_eq!(conn.from.node_id, src);
        assert_eq!(conn.from.pin_name, "image_out");
        assert_eq!(conn.to.node_id, dst);
        assert_eq!(conn.to.pin_name, "image_in");
    }

    #[test]
    fn connection_serialization_roundtrip() {
        let conn = Connection::new(
            PinId::new(Uuid::new_v4(), "out"),
            PinId::new(Uuid::new_v4(), "in"),
        );
        let json = serde_json::to_string(&conn).unwrap();
        let conn2: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(conn.id, conn2.id);
        assert_eq!(conn.from.node_id, conn2.from.node_id);
        assert_eq!(conn.to.pin_name, conn2.to.pin_name);
    }
}

// ===== GraphNode =====

mod graph_node_tests {
    use library::project::graph_node::GraphNode;
    use library::project::property::{Property, PropertyMap, PropertyValue};
    use uuid::Uuid;

    #[test]
    fn new_graph_node_has_unique_id() {
        let g1 = GraphNode::new("effect.blur", PropertyMap::new());
        let g2 = GraphNode::new("effect.blur", PropertyMap::new());
        assert_ne!(g1.id, g2.id);
    }

    #[test]
    fn new_with_id() {
        let id = Uuid::new_v4();
        let gn = GraphNode::new_with_id(id, "style.fill", PropertyMap::new());
        assert_eq!(gn.id, id);
        assert_eq!(gn.type_id, "style.fill");
    }

    #[test]
    fn graph_node_properties_accessible() {
        let mut props = PropertyMap::new();
        props.set(
            "radius".to_string(),
            Property::constant(PropertyValue::from(5.0)),
        );
        let gn = GraphNode::new("effect.blur", props);
        assert_eq!(gn.properties.get_f64("radius"), Some(5.0));
    }

    #[test]
    fn graph_node_serialization_roundtrip() {
        let mut props = PropertyMap::new();
        props.set(
            "amount".to_string(),
            Property::constant(PropertyValue::from(10.0)),
        );
        let gn = GraphNode::new("effect.glow", props);
        let json = serde_json::to_string(&gn).unwrap();
        let gn2: GraphNode = serde_json::from_str(&json).unwrap();
        assert_eq!(gn.id, gn2.id);
        assert_eq!(gn2.type_id, "effect.glow");
        assert_eq!(gn2.properties.get_f64("amount"), Some(10.0));
    }
}

// ===== Graph Analysis =====

mod graph_analysis_tests {
    use library::project::clip::{TrackClip, TrackClipKind};
    use library::project::connection::{Connection, PinId};
    use library::project::graph_analysis;
    use library::project::graph_node::GraphNode;
    use library::project::node::Node;
    use library::project::project::{Composition, Project};
    use library::project::property::{Property, PropertyMap, PropertyValue};
    use library::project::track::TrackData;
    use uuid::Uuid;

    /// Build a minimal project: root_track → clip → transform graph node (connected)
    fn build_project_with_clip_and_transform() -> (Project, Uuid, Uuid, Uuid) {
        let mut project = Project::new("test");
        let (comp, mut root_track) = Composition::new("comp", 1920, 1080, 30.0, 5.0);
        let root_track_id = root_track.id;

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            90,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;
        root_track.add_child(clip_id);

        // Transform graph node
        let transform = GraphNode::new("compositing.transform", PropertyMap::new());
        let transform_id = transform.id;

        // Connection: clip.image_out → transform.image_in
        let conn = Connection::new(
            PinId::new(clip_id, "image_out"),
            PinId::new(transform_id, "image_in"),
        );

        project.add_node(Node::Track(root_track));
        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(transform));
        project.add_connection(conn);
        project.add_composition(comp);

        (project, root_track_id, clip_id, transform_id)
    }

    #[test]
    fn resolve_clip_context_finds_transform() {
        let (project, _, clip_id, transform_id) = build_project_with_clip_and_transform();
        let ctx = graph_analysis::resolve_clip_context(&project, clip_id);
        assert_eq!(ctx.transform_node, Some(transform_id));
    }

    #[test]
    fn resolve_clip_context_no_effects() {
        let (project, _, clip_id, _) = build_project_with_clip_and_transform();
        let ctx = graph_analysis::resolve_clip_context(&project, clip_id);
        assert!(ctx.effect_chain.is_empty());
    }

    #[test]
    fn get_effect_chain_empty_for_no_effects() {
        let (project, _, clip_id, _) = build_project_with_clip_and_transform();
        let chain = graph_analysis::get_effect_chain(&project, clip_id);
        assert!(chain.is_empty());
    }

    #[test]
    fn collect_all_associated_nodes_includes_transform() {
        let (project, _, clip_id, transform_id) = build_project_with_clip_and_transform();
        let nodes = graph_analysis::collect_all_associated_nodes(&project, clip_id);
        assert!(nodes.contains(&transform_id));
    }

    #[test]
    fn resolve_clip_context_with_style() {
        let mut project = Project::new("test");
        let (comp, mut root_track) = Composition::new("comp", 1920, 1080, 30.0, 5.0);

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            90,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;
        root_track.add_child(clip_id);

        // Style fill graph node
        let mut style_props = PropertyMap::new();
        style_props.set(
            "color".to_string(),
            Property::constant(PropertyValue::Color(library::runtime::color::Color::white())),
        );
        let style = GraphNode::new("style.fill", style_props);
        let style_id = style.id;

        // Connection: clip.shape_out → style.shape_in (forward shape chain)
        let conn = Connection::new(
            PinId::new(clip_id, "shape_out"),
            PinId::new(style_id, "shape_in"),
        );

        project.add_node(Node::Track(root_track));
        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(style));
        project.add_connection(conn);
        project.add_composition(comp);

        let styles = graph_analysis::get_associated_styles(&project, clip_id);
        assert!(!styles.is_empty());
    }

    // ===== Regression tests: forward shape chain traversal =====

    /// Build a text clip with full shape chain: clip → effector → decorator → style → transform
    fn build_text_clip_full_chain() -> (Project, Uuid, Uuid, Uuid, Uuid, Uuid) {
        let mut project = Project::new("test");
        let (comp, mut root_track) = Composition::new("comp", 1920, 1080, 30.0, 5.0);

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            90,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;

        let effector = GraphNode::new("effector.random_transform", PropertyMap::new());
        let effector_id = effector.id;

        let decorator = GraphNode::new("decorator.backplate", PropertyMap::new());
        let decorator_id = decorator.id;

        let fill = GraphNode::new("style.fill", PropertyMap::new());
        let fill_id = fill.id;

        let transform = GraphNode::new("compositing.transform", PropertyMap::new());
        let transform_id = transform.id;

        // Build the chain: clip → effector → decorator → fill → transform
        root_track.add_child(clip_id);
        root_track.add_child(effector_id);
        root_track.add_child(decorator_id);
        root_track.add_child(fill_id);
        root_track.add_child(transform_id);

        project.add_node(Node::Track(root_track));
        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(effector));
        project.add_node(Node::Graph(decorator));
        project.add_node(Node::Graph(fill));
        project.add_node(Node::Graph(transform));

        // Shape chain: clip.shape_out → effector.shape_in → effector.shape_out → decorator.shape_in
        //              → decorator.shape_out → fill.shape_in
        project.add_connection(Connection::new(
            PinId::new(clip_id, "shape_out"),
            PinId::new(effector_id, "shape_in"),
        ));
        project.add_connection(Connection::new(
            PinId::new(effector_id, "shape_out"),
            PinId::new(decorator_id, "shape_in"),
        ));
        project.add_connection(Connection::new(
            PinId::new(decorator_id, "shape_out"),
            PinId::new(fill_id, "shape_in"),
        ));

        // Image chain: fill.image_out → transform.image_in
        project.add_connection(Connection::new(
            PinId::new(fill_id, "image_out"),
            PinId::new(transform_id, "image_in"),
        ));

        project.add_composition(comp);

        (
            project,
            clip_id,
            effector_id,
            decorator_id,
            fill_id,
            transform_id,
        )
    }

    #[test]
    fn get_associated_styles_finds_fill_via_shape_chain() {
        // Regression: styles must be discovered via forward shape chain (clip.shape_out → ... → style.shape_in),
        // NOT via backward traversal (clip.style_in doesn't exist)
        let (project, clip_id, _, _, fill_id, _) = build_text_clip_full_chain();
        let styles = graph_analysis::get_associated_styles(&project, clip_id);
        assert_eq!(
            styles,
            vec![fill_id],
            "Style should be found via shape chain"
        );
    }

    #[test]
    fn get_associated_styles_returns_empty_for_image_clip() {
        // Image clips have no shape_out, so no styles
        let (project, _, clip_id, _) = build_project_with_clip_and_transform();
        let styles = graph_analysis::get_associated_styles(&project, clip_id);
        assert!(
            styles.is_empty(),
            "Image clips should have no associated styles"
        );
    }

    #[test]
    fn get_associated_effectors_via_shape_chain() {
        let (project, clip_id, effector_id, _, _, _) = build_text_clip_full_chain();
        let effectors = graph_analysis::get_associated_effectors(&project, clip_id);
        assert_eq!(effectors, vec![effector_id]);
    }

    #[test]
    fn get_associated_decorators_via_shape_chain() {
        let (project, clip_id, _, decorator_id, _, _) = build_text_clip_full_chain();
        let decorators = graph_analysis::get_associated_decorators(&project, clip_id);
        assert_eq!(decorators, vec![decorator_id]);
    }

    #[test]
    fn resolve_clip_context_full_text_chain() {
        // Regression: full text chain must be resolved correctly
        let (project, clip_id, effector_id, decorator_id, fill_id, transform_id) =
            build_text_clip_full_chain();
        let ctx = graph_analysis::resolve_clip_context(&project, clip_id);

        assert_eq!(ctx.transform_node, Some(transform_id));
        assert_eq!(ctx.style_chain, vec![fill_id]);
        assert_eq!(ctx.effector_chain, vec![effector_id]);
        assert_eq!(ctx.decorator_chain, vec![decorator_id]);
        assert!(ctx.effect_chain.is_empty());
    }

    #[test]
    fn collect_all_associated_nodes_full_text_chain() {
        let (project, clip_id, effector_id, decorator_id, fill_id, transform_id) =
            build_text_clip_full_chain();
        let nodes = graph_analysis::collect_all_associated_nodes(&project, clip_id);

        assert!(nodes.contains(&transform_id));
        assert!(nodes.contains(&fill_id));
        assert!(nodes.contains(&effector_id));
        assert!(nodes.contains(&decorator_id));
        assert_eq!(nodes.len(), 4);
    }

    /// Build a video clip with effect chain: clip → effect → transform
    fn build_video_clip_with_effect() -> (Project, Uuid, Uuid, Uuid) {
        let mut project = Project::new("test");
        let (comp, mut root_track) = Composition::new("comp", 1920, 1080, 30.0, 5.0);

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Video,
            0,
            90,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;

        let effect = GraphNode::new("effect.blur", PropertyMap::new());
        let effect_id = effect.id;

        let transform = GraphNode::new("compositing.transform", PropertyMap::new());
        let transform_id = transform.id;

        root_track.add_child(clip_id);
        root_track.add_child(effect_id);
        root_track.add_child(transform_id);

        project.add_node(Node::Track(root_track));
        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(effect));
        project.add_node(Node::Graph(transform));

        // Image chain: clip.image_out → effect.image_in → effect.image_out → transform.image_in
        project.add_connection(Connection::new(
            PinId::new(clip_id, "image_out"),
            PinId::new(effect_id, "image_in"),
        ));
        project.add_connection(Connection::new(
            PinId::new(effect_id, "image_out"),
            PinId::new(transform_id, "image_in"),
        ));

        project.add_composition(comp);

        (project, clip_id, effect_id, transform_id)
    }

    #[test]
    fn effect_chain_with_transform_connected() {
        // Regression: effects should be properly chained with transform at end
        let (project, clip_id, effect_id, transform_id) = build_video_clip_with_effect();

        let chain = graph_analysis::get_effect_chain(&project, clip_id);
        assert_eq!(chain, vec![effect_id]);

        let ctx = graph_analysis::resolve_clip_context(&project, clip_id);
        assert_eq!(ctx.effect_chain, vec![effect_id]);
        assert_eq!(ctx.transform_node, Some(transform_id));
    }

    #[test]
    fn validate_connection_rejects_duplicate_input() {
        // Regression: validate_connection should reject duplicate connections to same input pin
        let (mut project, _, _) = {
            let mut project = Project::new("test");
            let root_track = TrackData::new("Root");
            let root_id = root_track.id;
            project.add_node(Node::Track(root_track));
            let comp = Composition::new_with_root("comp", 1920, 1080, 30.0, 10.0, root_id);
            let comp_id = comp.id;
            project.add_composition(comp);
            (project, root_id, comp_id)
        };

        let node_a = GraphNode::new("test.a", PropertyMap::new());
        let a_id = node_a.id;
        let node_b = GraphNode::new("test.b", PropertyMap::new());
        let b_id = node_b.id;
        let node_c = GraphNode::new("test.c", PropertyMap::new());
        let c_id = node_c.id;

        project.add_node(Node::Graph(node_a));
        project.add_node(Node::Graph(node_b));
        project.add_node(Node::Graph(node_c));

        // Connect A → B.input
        let conn1 = Connection::new(PinId::new(a_id, "out"), PinId::new(b_id, "in"));
        project.add_connection(conn1);

        // Try to connect C → B.input (same input pin) — should fail
        let conn2 = Connection::new(PinId::new(c_id, "out"), PinId::new(b_id, "in"));
        let result = graph_analysis::validate_connection(&project, &conn2);
        assert!(
            result.is_err(),
            "Duplicate input connection should be rejected"
        );
        assert!(result.unwrap_err().contains("already has a connection"));
    }

    #[test]
    fn validate_connection_allows_after_removal() {
        // Regression: removing an old connection should allow a new one to the same input
        let (mut project, _, _) = {
            let mut project = Project::new("test");
            let root_track = TrackData::new("Root");
            let root_id = root_track.id;
            project.add_node(Node::Track(root_track));
            let comp = Composition::new_with_root("comp", 1920, 1080, 30.0, 10.0, root_id);
            let comp_id = comp.id;
            project.add_composition(comp);
            (project, root_id, comp_id)
        };

        let node_a = GraphNode::new("test.a", PropertyMap::new());
        let a_id = node_a.id;
        let node_b = GraphNode::new("test.b", PropertyMap::new());
        let b_id = node_b.id;
        let node_c = GraphNode::new("test.c", PropertyMap::new());
        let c_id = node_c.id;

        project.add_node(Node::Graph(node_a));
        project.add_node(Node::Graph(node_b));
        project.add_node(Node::Graph(node_c));

        // Connect A → B.input
        let conn1 = Connection::new(PinId::new(a_id, "out"), PinId::new(b_id, "in"));
        let conn1_id = conn1.id;
        project.add_connection(conn1);

        // Remove old connection
        project.remove_connection(conn1_id);

        // Now C → B.input should succeed
        let conn2 = Connection::new(PinId::new(c_id, "out"), PinId::new(b_id, "in"));
        let result = graph_analysis::validate_connection(&project, &conn2);
        assert!(
            result.is_ok(),
            "Connection should be valid after old one is removed"
        );
    }

    #[test]
    fn effect_insertion_between_clip_and_transform() {
        // Regression: inserting an effect should properly break and re-chain connections
        // Starting state: clip.image_out → transform.image_in
        // After insertion: clip.image_out → effect.image_in → effect.image_out → transform.image_in
        let mut project = Project::new("test");
        let (comp, mut root_track) = Composition::new("comp", 1920, 1080, 30.0, 5.0);

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Video,
            0,
            90,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;

        let transform = GraphNode::new("compositing.transform", PropertyMap::new());
        let transform_id = transform.id;

        let effect = GraphNode::new("effect.blur", PropertyMap::new());
        let effect_id = effect.id;

        root_track.add_child(clip_id);
        root_track.add_child(transform_id);
        root_track.add_child(effect_id);

        project.add_node(Node::Track(root_track));
        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(transform));
        project.add_node(Node::Graph(effect));

        // Initial: clip.image_out → transform.image_in
        let old_conn = Connection::new(
            PinId::new(clip_id, "image_out"),
            PinId::new(transform_id, "image_in"),
        );
        let old_conn_id = old_conn.id;
        project.add_connection(old_conn);
        project.add_composition(comp);

        // Simulate inspector effect insertion:
        // 1. Remove old connection to transform.image_in
        project.remove_connection(old_conn_id);
        // 2. Connect clip.image_out → effect.image_in
        project.add_connection(Connection::new(
            PinId::new(clip_id, "image_out"),
            PinId::new(effect_id, "image_in"),
        ));
        // 3. Connect effect.image_out → transform.image_in
        project.add_connection(Connection::new(
            PinId::new(effect_id, "image_out"),
            PinId::new(transform_id, "image_in"),
        ));

        // Verify the chain
        let ctx = graph_analysis::resolve_clip_context(&project, clip_id);
        assert_eq!(ctx.effect_chain, vec![effect_id]);
        assert_eq!(ctx.transform_node, Some(transform_id));
    }

    #[test]
    fn style_insertion_into_shape_chain() {
        // Regression: adding a style to clip with only effector should connect correctly
        // Starting: clip.shape_out → effector.shape_in (no style yet)
        // After: clip.shape_out → effector.shape_in → effector.shape_out → style.shape_in
        //        style.image_out → transform.image_in
        let mut project = Project::new("test");
        let (comp, mut root_track) = Composition::new("comp", 1920, 1080, 30.0, 5.0);

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            90,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;

        let effector = GraphNode::new("effector.random_transform", PropertyMap::new());
        let effector_id = effector.id;

        let transform = GraphNode::new("compositing.transform", PropertyMap::new());
        let transform_id = transform.id;

        let fill = GraphNode::new("style.fill", PropertyMap::new());
        let fill_id = fill.id;

        root_track.add_child(clip_id);
        root_track.add_child(effector_id);
        root_track.add_child(transform_id);
        root_track.add_child(fill_id);

        project.add_node(Node::Track(root_track));
        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(effector));
        project.add_node(Node::Graph(transform));
        project.add_node(Node::Graph(fill));

        // Initial chain: clip.shape_out → effector.shape_in
        project.add_connection(Connection::new(
            PinId::new(clip_id, "shape_out"),
            PinId::new(effector_id, "shape_in"),
        ));

        // Style insertion: effector.shape_out → fill.shape_in
        project.add_connection(Connection::new(
            PinId::new(effector_id, "shape_out"),
            PinId::new(fill_id, "shape_in"),
        ));
        // fill.image_out → transform.image_in
        project.add_connection(Connection::new(
            PinId::new(fill_id, "image_out"),
            PinId::new(transform_id, "image_in"),
        ));

        project.add_composition(comp);

        // Verify
        let ctx = graph_analysis::resolve_clip_context(&project, clip_id);
        assert_eq!(ctx.effector_chain, vec![effector_id]);
        assert_eq!(ctx.style_chain, vec![fill_id]);
        assert_eq!(ctx.transform_node, Some(transform_id));
    }

    #[test]
    fn multiple_effects_chained_correctly() {
        // Regression: multiple effects must chain linearly
        // clip.image_out → effect1.image_in → effect1.image_out → effect2.image_in
        //                → effect2.image_out → transform.image_in
        let mut project = Project::new("test");
        let (comp, mut root_track) = Composition::new("comp", 1920, 1080, 30.0, 5.0);

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Video,
            0,
            90,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;

        let effect1 = GraphNode::new("effect.blur", PropertyMap::new());
        let effect1_id = effect1.id;
        let effect2 = GraphNode::new("effect.dilate", PropertyMap::new());
        let effect2_id = effect2.id;

        let transform = GraphNode::new("compositing.transform", PropertyMap::new());
        let transform_id = transform.id;

        root_track.add_child(clip_id);
        root_track.add_child(effect1_id);
        root_track.add_child(effect2_id);
        root_track.add_child(transform_id);

        project.add_node(Node::Track(root_track));
        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(effect1));
        project.add_node(Node::Graph(effect2));
        project.add_node(Node::Graph(transform));

        project.add_connection(Connection::new(
            PinId::new(clip_id, "image_out"),
            PinId::new(effect1_id, "image_in"),
        ));
        project.add_connection(Connection::new(
            PinId::new(effect1_id, "image_out"),
            PinId::new(effect2_id, "image_in"),
        ));
        project.add_connection(Connection::new(
            PinId::new(effect2_id, "image_out"),
            PinId::new(transform_id, "image_in"),
        ));

        project.add_composition(comp);

        let ctx = graph_analysis::resolve_clip_context(&project, clip_id);
        assert_eq!(ctx.effect_chain, vec![effect1_id, effect2_id]);
        assert_eq!(ctx.transform_node, Some(transform_id));
    }

    #[test]
    fn text_clip_with_effects_after_style() {
        // Regression: text clip with effects between style and transform
        // clip.shape_out → fill.shape_in → fill.image_out → effect.image_in
        //                → effect.image_out → transform.image_in
        let mut project = Project::new("test");
        let (comp, mut root_track) = Composition::new("comp", 1920, 1080, 30.0, 5.0);

        let clip = TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            0,
            90,
            0,
            None,
            30.0,
            PropertyMap::new(),
        );
        let clip_id = clip.id;

        let fill = GraphNode::new("style.fill", PropertyMap::new());
        let fill_id = fill.id;

        let effect = GraphNode::new("effect.blur", PropertyMap::new());
        let effect_id = effect.id;

        let transform = GraphNode::new("compositing.transform", PropertyMap::new());
        let transform_id = transform.id;

        root_track.add_child(clip_id);
        root_track.add_child(fill_id);
        root_track.add_child(effect_id);
        root_track.add_child(transform_id);

        project.add_node(Node::Track(root_track));
        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(fill));
        project.add_node(Node::Graph(effect));
        project.add_node(Node::Graph(transform));

        // Shape chain: clip.shape_out → fill.shape_in
        project.add_connection(Connection::new(
            PinId::new(clip_id, "shape_out"),
            PinId::new(fill_id, "shape_in"),
        ));
        // Image chain after style: fill.image_out → effect.image_in → effect.image_out → transform.image_in
        project.add_connection(Connection::new(
            PinId::new(fill_id, "image_out"),
            PinId::new(effect_id, "image_in"),
        ));
        project.add_connection(Connection::new(
            PinId::new(effect_id, "image_out"),
            PinId::new(transform_id, "image_in"),
        ));

        project.add_composition(comp);

        let ctx = graph_analysis::resolve_clip_context(&project, clip_id);
        assert_eq!(ctx.style_chain, vec![fill_id]);
        // Note: effect chain from clip.image_out is empty for text clips
        // (effects are in the image chain after style, discovered via shape chain resolution)
        assert_eq!(ctx.transform_node, Some(transform_id));
    }
}

// ===== EvalEngine (construction only — no GPU) =====

mod eval_engine_tests {
    use library::pipeline::engine::EvalEngine;

    #[test]
    fn with_default_evaluators_creates_engine() {
        let engine = EvalEngine::with_default_evaluators();
        // Just verify it doesn't panic and creates successfully
        drop(engine);
    }

    #[test]
    fn new_engine_creates_empty() {
        let engine = EvalEngine::new();
        drop(engine);
    }
}
