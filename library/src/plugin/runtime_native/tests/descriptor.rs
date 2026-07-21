    #[test]
    fn strict_defaults_reject_lossy_json_conversions() {
        assert!(default_error(PropertyUiV1::Text, serde_json::Value::Null).contains("JSON string"));
        assert!(
            default_error(
                PropertyUiV1::Integer {
                    min: i64::MIN,
                    max: i64::MAX,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                serde_json::json!(u64::MAX)
            )
            .contains("representable as i64")
        );
        assert!(
            default_error(
                PropertyUiV1::Color,
                serde_json::json!({"r": 256, "g": 0, "b": 0, "a": 255}),
            )
            .contains("0..=255")
        );
        assert!(
            default_error(
                PropertyUiV1::Color,
                serde_json::json!({"r": 1, "g": 2, "b": 3, "a": 4, "extra": 5}),
            )
            .contains("expected exactly")
        );
    }

    #[test]
    fn strict_defaults_enforce_hard_bounds_and_dropdown_membership() {
        assert!(
            default_error(
                PropertyUiV1::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: String::new(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                serde_json::json!(101.0),
            )
            .contains("cannot be greater")
        );
        assert!(
            default_error(
                PropertyUiV1::Dropdown {
                    options: vec!["Block".to_string(), "Char".to_string()],
                },
                serde_json::json!("Parts"),
            )
            .contains("not a dropdown option")
        );
    }

    #[test]
    fn abi_v1_rejects_unintegrated_categories_instead_of_registering_descriptors() {
        let supported = component(PropertyUiV1::Bool, serde_json::json!(true));
        let mut unsupported = supported.clone();
        unsupported.id = "example.unsupported_exporter".to_string();
        unsupported.category = "exporter".to_string();
        let descriptor = PluginDescriptorV1 {
            name: "Mixed".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![supported, unsupported],
        };
        let error = validate_descriptor(&descriptor)
            .expect_err("an unintegrated category must reject the bundle")
            .to_string();
        assert!(error.contains("uses category 'exporter'"));
        assert!(error.contains("'style'"));
        assert!(error.contains("'decorator'"));
        assert!(error.contains("entire bundle was rejected"));
    }

    #[test]
    fn config_categories_register_typed_descriptor_backed_nodes_atomically() {
        use crate::model::NodeContent;
        use crate::model::project::{
            IMAGE_OUTPUT_PORT, PortDataType, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
        };

        let mut registry = RuntimePluginRegistry::new();
        let mut effects: PluginRepository<dyn EffectPlugin> = PluginRepository::new();
        let mut loaders = LoadRepository::new();
        let mut effectors: PluginRepository<dyn EffectorPlugin> = PluginRepository::new();
        let mut decorators: PluginRepository<dyn DecoratorPlugin> = PluginRepository::new();
        let mut styles: PluginRepository<dyn StylePlugin> = PluginRepository::new();
        let mut property_evaluators = PropertyEvaluatorRegistry::new();
        let registered = registry
            .register_bundle(
                pending_bundle(config_descriptor()),
                RuntimeRegistrationTargets {
                    effect_plugins: &mut effects,
                    load_plugins: &mut loaders,
                    effector_plugins: &mut effectors,
                    decorator_plugins: &mut decorators,
                    style_plugins: &mut styles,
                    property_evaluators: &mut property_evaluators,
                },
            )
            .expect("the complete low-bandwidth config bundle registers");
        assert_eq!(
            registered,
            vec![
                (
                    STYLE_CATEGORY.to_string(),
                    "example.runtime_fill".to_string()
                ),
                (
                    DECORATOR_CATEGORY.to_string(),
                    "example.runtime_backplate".to_string()
                ),
            ]
        );

        let style_descriptor = styles
            .get("example.runtime_fill")
            .expect("runtime Style adapter is in the Style repository")
            .descriptor()
            .expect("runtime Style descriptor is valid");
        let style_node = style_descriptor
            .create_node()
            .expect("runtime Style descriptor creates a Node");
        assert_eq!(style_node.properties().iter().count(), 2);
        let NodeContent::PluginOperation(style_operation) = style_node.content() else {
            panic!("Style descriptor must create PluginOperation content")
        };
        assert_eq!(style_operation.category, crate::plugin::STYLE_CATEGORY);
        assert_eq!(
            style_operation.operation,
            crate::plugin::STYLE_APPLY_OPERATION
        );
        for (key, data_type) in [
            (TIME_PORT, PortDataType::Number),
            (SHAPE_INPUT_PORT, PortDataType::Shape),
            (IMAGE_OUTPUT_PORT, PortDataType::Image),
        ] {
            assert!(
                style_operation
                    .declared_ports
                    .iter()
                    .any(|port| port.key == key && port.data_type == data_type),
                "Style operation is missing typed port {key}"
            );
        }

        let decorator_descriptor = decorators
            .get("example.runtime_backplate")
            .expect("runtime Decorator adapter is in the Decorator repository")
            .descriptor()
            .expect("runtime Decorator descriptor is valid");
        let decorator_node = decorator_descriptor
            .create_node()
            .expect("runtime Decorator descriptor creates a Node");
        assert_eq!(decorator_node.properties().iter().count(), 4);
        let NodeContent::PluginOperation(decorator_operation) = decorator_node.content() else {
            panic!("Decorator descriptor must create PluginOperation content")
        };
        assert_eq!(
            decorator_operation.category,
            crate::plugin::DECORATOR_CATEGORY
        );
        assert_eq!(
            decorator_operation.operation,
            crate::plugin::DECORATOR_APPLY_OPERATION
        );
        for (key, data_type) in [
            (TIME_PORT, PortDataType::Number),
            (SHAPE_INPUT_PORT, PortDataType::Shape),
            (
                crate::model::project::BACKGROUND_SHAPE_INPUT_PORT,
                PortDataType::Shape,
            ),
            (SHAPE_OUTPUT_PORT, PortDataType::Shape),
        ] {
            assert!(
                decorator_operation
                    .declared_ports
                    .iter()
                    .any(|port| port.key == key && port.data_type == data_type),
                "Decorator operation is missing typed port {key}"
            );
        }
    }
