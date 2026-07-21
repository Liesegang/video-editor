    #[test]
    fn malformed_late_decorator_does_not_partially_register_an_earlier_style() {
        let mut malformed_decorator = decorator_component();
        malformed_decorator.properties[1].default =
            serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0, "w": 4.0, "extra": 5.0});
        let descriptor = PluginDescriptorV1 {
            name: "Atomic mixed config".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![style_component(), malformed_decorator],
        };
        let mut registry = RuntimePluginRegistry::new();
        let mut effects: PluginRepository<dyn EffectPlugin> = PluginRepository::new();
        let mut loaders = LoadRepository::new();
        let mut effectors: PluginRepository<dyn EffectorPlugin> = PluginRepository::new();
        let mut decorators: PluginRepository<dyn DecoratorPlugin> = PluginRepository::new();
        let mut styles: PluginRepository<dyn StylePlugin> = PluginRepository::new();
        let mut property_evaluators = PropertyEvaluatorRegistry::new();

        let error = registry
            .register_bundle(
                pending_bundle(descriptor),
                RuntimeRegistrationTargets {
                    effect_plugins: &mut effects,
                    load_plugins: &mut loaders,
                    effector_plugins: &mut effectors,
                    decorator_plugins: &mut decorators,
                    style_plugins: &mut styles,
                    property_evaluators: &mut property_evaluators,
                },
            )
            .expect_err("a malformed later Decorator must reject the whole bundle")
            .to_string();
        assert!(error.contains("expected exactly finite number fields"));
        assert!(registry.components.is_empty());
        assert!(registry.descriptors.is_empty());
        assert!(registry.libraries.is_empty());
        assert!(effectors.plugins.is_empty());
        assert!(decorators.plugins.is_empty());
        assert!(styles.plugins.is_empty());
    }

    #[test]
    fn config_categories_require_their_versioned_operation_and_no_default_output() {
        for (mut component, operation) in [
            (style_component(), STYLE_EVALUATE_V1),
            (decorator_component(), DECORATOR_EVALUATE_V2),
        ] {
            component.operations.clear();
            let error = validate_descriptor(&descriptor_with(component.clone()))
                .expect_err("config component without its versioned evaluator is invalid")
                .to_string();
            assert!(error.contains(operation));

            component.operations.push(operation.to_string());
            component.output_default = Some(PropertyValueV1::Boolean { value: false });
            let error = validate_descriptor(&descriptor_with(component))
                .expect_err("NoOutput categories cannot declare a fabricated default")
                .to_string();
            assert!(error.contains("must not declare output_default"));
        }
    }

    #[test]
    fn high_bandwidth_categories_require_exact_extensions_and_loader_has_no_fake_properties() {
        let effect = effect_component();
        let loader = loader_component();
        validate_descriptor(&PluginDescriptorV1 {
            name: "Typed hot paths".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![effect.clone(), loader.clone()],
        })
        .expect("the exact typed Effect/Loader contracts are accepted");

        let mut missing_effect_operation = effect;
        missing_effect_operation.operations = vec!["effect.apply.v1".to_string()];
        assert!(
            validate_descriptor(&descriptor_with(missing_effect_operation))
                .expect_err("Effect must declare its typed CPU RGBA8 operation")
                .to_string()
                .contains(EFFECT_PROCESS_CPU_RGBA8_V1)
        );

        let mut fake_loader_property = loader;
        fake_loader_property.properties = style_component().properties;
        assert!(
            validate_descriptor(&descriptor_with(fake_loader_property))
                .expect_err("Loader config cannot be advertised without an execution contract")
                .to_string()
                .contains("must not declare properties")
        );
    }

    #[test]
    fn effect_time_transport_cannot_collide_with_instance_config()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut reserved = effect_component();
        let property = reserved
            .properties
            .first_mut()
            .ok_or_else(|| std::io::Error::other("effect fixture has no property"))?;
        property.name = RUNTIME_EFFECT_TIME_PROPERTY.to_string();
        let Err(error) = validate_descriptor(&descriptor_with(reserved)) else {
            return Err(std::io::Error::other(
                "u_time must be rejected during bundle descriptor preflight",
            )
            .into());
        };
        assert!(error.to_string().contains("per-frame render time"));

        let descriptor = effect_component();
        let definitions = property_definitions(&descriptor)?;
        let plugin = RuntimeEffectPlugin::new(
            RuntimeComponent {
                descriptor,
                library: Arc::new(RuntimeLibrary {
                    api: RuviePluginApiV1 {
                        abi_version: RUVIE_PLUGIN_ABI_V1,
                        struct_size: size_of::<RuviePluginApiV1>(),
                        context: std::ptr::null_mut(),
                        descriptor_json: None,
                        invoke_json: None,
                        free_buffer: None,
                        query_extension: None,
                    },
                    _library: current_process_library(),
                }),
            },
            definitions,
            RuvieEffectCpuRgba8ApiV1 {
                abi_version: RUVIE_PLUGIN_ABI_V1,
                struct_size: size_of::<RuvieEffectCpuRgba8ApiV1>(),
                context: std::ptr::null_mut(),
                create_instance: None,
                process: None,
                release_instance: None,
                free_frame: None,
            },
        )?;
        let mut first = HashMap::from([(
            "amount".to_string(),
            PropertyValue::Number(OrderedFloat(0.5)),
        )]);
        first.insert(
            RUNTIME_EFFECT_TIME_PROPERTY.to_string(),
            PropertyValue::Number(OrderedFloat(1.0)),
        );
        let mut second = first.clone();
        second.insert(
            RUNTIME_EFFECT_TIME_PROPERTY.to_string(),
            PropertyValue::Number(OrderedFloat(9.0)),
        );
        let first_key = plugin.config_key(&first)?;
        let second_key = plugin.config_key(&second)?;
        assert_eq!(first_key, second_key);
        assert_eq!(
            first_key
                .0
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["amount"]
        );
        Ok(())
    }

    #[test]
    fn missing_effect_extension_rejects_the_entire_mixed_bundle_before_registration() {
        let descriptor = PluginDescriptorV1 {
            name: "Atomic Style and Effect".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![style_component(), effect_component()],
        };
        let mut registry = RuntimePluginRegistry::new();
        let mut effects: PluginRepository<dyn EffectPlugin> = PluginRepository::new();
        let mut loaders = LoadRepository::new();
        let mut effectors: PluginRepository<dyn EffectorPlugin> = PluginRepository::new();
        let mut decorators: PluginRepository<dyn DecoratorPlugin> = PluginRepository::new();
        let mut styles: PluginRepository<dyn StylePlugin> = PluginRepository::new();
        let mut property_evaluators = PropertyEvaluatorRegistry::new();
        let error = registry
            .register_bundle(
                pending_bundle(descriptor),
                RuntimeRegistrationTargets {
                    effect_plugins: &mut effects,
                    load_plugins: &mut loaders,
                    effector_plugins: &mut effectors,
                    decorator_plugins: &mut decorators,
                    style_plugins: &mut styles,
                    property_evaluators: &mut property_evaluators,
                },
            )
            .expect_err("a missing typed Effect extension rejects the mixed bundle")
            .to_string();
        assert!(error.contains(EFFECT_CPU_RGBA8_EXTENSION_V1));
        assert!(registry.components.is_empty());
        assert!(effects.plugins.is_empty());
        assert!(styles.plugins.is_empty());
    }

    #[test]
    fn property_category_requires_a_valid_explicit_output_default() {
        let valid = property_component(Some(PropertyValueV1::Number { value: 0.0 }));
        validate_descriptor(&descriptor_with(valid))
            .expect("property category and typed fail-safe are integrated in ABI v1");

        let missing = property_component(None);
        let error = validate_descriptor(&descriptor_with(missing))
            .expect_err("property evaluator without a fail-safe must be rejected")
            .to_string();
        assert!(error.contains("must declare output_default"));

        let non_finite = property_component(Some(PropertyValueV1::Number { value: f64::NAN }));
        let error = validate_descriptor(&descriptor_with(non_finite))
            .expect_err("non-finite fail-safe cannot cross JSON ABI v1")
            .to_string();
        assert!(error.contains("non-finite"));
    }

    #[test]
    fn invalid_property_response_logs_and_uses_descriptor_fail_safe() {
        let descriptor = property_component(Some(PropertyValueV1::Number { value: 7.0 }));
        let component = RuntimeComponent {
            descriptor: descriptor.clone(),
            library: Arc::new(RuntimeLibrary {
                api: RuviePluginApiV1 {
                    abi_version: RUVIE_PLUGIN_ABI_V1,
                    struct_size: size_of::<RuviePluginApiV1>(),
                    context: std::ptr::null_mut(),
                    descriptor_json: None,
                    invoke_json: Some(invalid_property_response),
                    free_buffer: Some(test_free_buffer),
                    query_extension: None,
                },
                _library: current_process_library(),
            }),
        };
        let evaluator = RuntimePropertyEvaluator {
            component,
            definitions: property_definitions(&descriptor).expect("test definition is valid"),
            output_default: PropertyValue::Number(OrderedFloat(7.0)),
        };
        let property = Property {
            evaluator: descriptor.id,
            properties: HashMap::new(),
        };
        let siblings = crate::model::property::PropertyMap::new();
        let context = EvaluationContext::new(&siblings, 30.0, (1920, 1080));
        assert_eq!(
            evaluator.evaluate(&property, 0.0, &context),
            Ok(PropertyValue::Number(OrderedFloat(7.0))),
            "invalid plugin output must use the descriptor-declared fail-safe"
        );
    }

    #[test]
    fn property_invocation_failure_uses_only_the_declared_fail_safe() {
        let descriptor = property_component(Some(PropertyValueV1::Number { value: 11.0 }));
        let evaluator = RuntimePropertyEvaluator {
            component: RuntimeComponent {
                descriptor: descriptor.clone(),
                library: Arc::new(RuntimeLibrary {
                    api: RuviePluginApiV1 {
                        abi_version: RUVIE_PLUGIN_ABI_V1,
                        struct_size: size_of::<RuviePluginApiV1>(),
                        context: std::ptr::null_mut(),
                        descriptor_json: None,
                        invoke_json: Some(failing_property_response),
                        free_buffer: Some(test_free_buffer),
                        query_extension: None,
                    },
                    _library: current_process_library(),
                }),
            },
            definitions: property_definitions(&descriptor).expect("test definition is valid"),
            output_default: PropertyValue::Number(OrderedFloat(11.0)),
        };
        let property = Property {
            evaluator: descriptor.id,
            properties: HashMap::new(),
        };
        let siblings = crate::model::property::PropertyMap::new();
        let context = EvaluationContext::new(&siblings, 30.0, (1920, 1080));
        assert_eq!(
            evaluator.evaluate(&property, 0.0, &context),
            Ok(PropertyValue::Number(OrderedFloat(11.0))),
            "plugin errors must not be disguised as an invented zero/default"
        );
    }

    #[test]
    fn late_definition_failure_does_not_partially_commit_a_bundle() {
        let resolved = ResolvedBundle {
            manifest_path: PathBuf::from("/runtime-plugin-test/ruvie-plugin.toml"),
            library_path: PathBuf::from("/runtime-plugin-test/plugin.test"),
        };
        let mut registry = RuntimePluginRegistry::new();
        let mut effects: PluginRepository<dyn EffectPlugin> = PluginRepository::new();
        let mut loaders = LoadRepository::new();
        let mut effectors: PluginRepository<dyn EffectorPlugin> = PluginRepository::new();
        let mut decorators: PluginRepository<dyn DecoratorPlugin> = PluginRepository::new();
        let mut styles: PluginRepository<dyn StylePlugin> = PluginRepository::new();
        let mut property_evaluators = PropertyEvaluatorRegistry::new();
        assert_eq!(
            registry.claim_bundle(&resolved),
            RuntimeBundleClaim::Claimed
        );

        let error = registry
            .register_bundle(
                pending_bundle(two_component_descriptor(serde_json::json!(101.0))),
                RuntimeRegistrationTargets {
                    effect_plugins: &mut effects,
                    load_plugins: &mut loaders,
                    effector_plugins: &mut effectors,
                    decorator_plugins: &mut decorators,
                    style_plugins: &mut styles,
                    property_evaluators: &mut property_evaluators,
                },
            )
            .expect_err("the second component exceeds its hard maximum")
            .to_string();
        assert!(error.contains("cannot be greater"));
        registry.cancel_bundle_load(&resolved);

        assert!(registry.components.is_empty());
        assert!(registry.descriptors.is_empty());
        assert!(registry.libraries.is_empty());
        assert!(registry.loaded_manifests.is_empty());
        assert!(effectors.plugins.is_empty());
        assert!(decorators.plugins.is_empty());
        assert!(styles.plugins.is_empty());
        assert!(!property_evaluators.contains("example.first"));

        assert_eq!(
            registry.claim_bundle(&resolved),
            RuntimeBundleClaim::Claimed
        );
        let registered = registry
            .register_bundle(
                pending_bundle(two_component_descriptor(serde_json::json!(50.0))),
                RuntimeRegistrationTargets {
                    effect_plugins: &mut effects,
                    load_plugins: &mut loaders,
                    effector_plugins: &mut effectors,
                    decorator_plugins: &mut decorators,
                    style_plugins: &mut styles,
                    property_evaluators: &mut property_evaluators,
                },
            )
            .expect("a corrected rescan must not hit a stale partial-ID collision");
        assert_eq!(registered.len(), 2);
        assert!(effectors.get("example.first").is_some());
        assert!(effectors.get("example.second").is_some());
    }
