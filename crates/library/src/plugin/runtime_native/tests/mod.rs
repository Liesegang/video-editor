use std::collections::HashMap;
use std::mem::{align_of, size_of};
use std::path::PathBuf;
use std::sync::Arc;

use libloading::Library;
use ordered_float::OrderedFloat;
use ruvie_plugin_api::{
    ALPHA_MODE_STRAIGHT_V1, ASSET_KIND_VIDEO_V1, ASSET_METADATA_FPS_V1,
    ASSET_METADATA_TIME_BASE_V1, COLOR_PROFILE_SRGB_V1, ColorV1, ComponentDescriptorV1,
    DECORATOR_CATEGORY, DECORATOR_EVALUATE_V1, DECORATOR_EVALUATE_V2, DecoratorOutputV2,
    DecoratorTargetV2, EFFECT_CATEGORY, EFFECT_CPU_RGBA8_EXTENSION_V1, EFFECT_PROCESS_CPU_RGBA8_V1,
    EFFECTOR_CATEGORY, EFFECTOR_EVALUATE_V1, LOADER_CATEGORY, LOADER_LOAD_CPU_RGBA8_V1,
    LOADER_OPEN_V1, MAX_CPU_RGBA8_DIMENSION_V1, MAX_CPU_RGBA8_FRAME_BYTES_V1,
    MAX_STYLE_DASH_INTERVALS_V1, PROPERTY_CATEGORY, PROPERTY_EVALUATE_V1, PluginDescriptorV1,
    PropertyDefinitionV1, PropertyUiV1, PropertyValueV1, RUVIE_PLUGIN_ABI_V1, RuvieAssetMetadataV1,
    RuvieBuffer, RuvieBytesView, RuvieCallResult, RuvieEffectCpuRgba8ApiV1, RuvieExtensionResultV1,
    RuvieOwnedRgba8FrameV1, RuviePluginApiV1, RuviePropertyMapViewV1, STATUS_OK, STYLE_CATEGORY,
    STYLE_EVALUATE_V1, StrokeCapV1, StrokeJoinV1, StyleOutputV1,
};

use super::RUNTIME_EFFECT_TIME_PROPERTY;
use super::abi::{RuntimeComponent, RuntimeLibrary, copy_abi_table};
use super::adapters::decorator::{
    RuntimeDecoratorPlugin, RuntimeDecoratorProtocol, decorator_config_from_response,
    decorator_config_from_response_v2, decorator_config_from_wire_v2,
    safe_decorator_config_from_response_v2,
};
use super::adapters::effect::{EffectConfigKey, RuntimeEffectPlugin};
use super::adapters::loader::{metadata_from_wire, runtime_loader_cache_key, source_time_bits};
use super::adapters::property::RuntimePropertyEvaluator;
use super::adapters::style::{
    safe_style_config_from_response, style_config_from_response, style_config_from_wire,
    valid_stroke_dash_pattern, valid_stroke_render_geometry,
};
use super::bundle::{PendingBundle, ResolvedBundle};
use super::descriptor::{property_definitions, validate_descriptor};
use super::registry::{RuntimeBundleClaim, RuntimePluginRegistry, RuntimeRegistrationTargets};
use super::rgba8::{copy_owned_frame, validate_rgba8_layout};
use crate::model::property::{Property, PropertyValue};
use crate::plugin::evaluator::{EvaluationContext, PropertyEvaluator, PropertyEvaluatorRegistry};
use crate::plugin::repository::PluginRepository;
use crate::plugin::{
    DecoratorPlugin, EffectPlugin, EffectorPlugin, LoadRepository, LoadRequest, StylePlugin,
};

fn component(ui: PropertyUiV1, default: serde_json::Value) -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: "example.strict".to_string(),
        name: "Strict Defaults".to_string(),
        category: EFFECTOR_CATEGORY.to_string(),
        group: "Tests".to_string(),
        version: "1.0.0".to_string(),
        operations: vec![EFFECTOR_EVALUATE_V1.to_string()],
        properties: vec![PropertyDefinitionV1 {
            name: "value".to_string(),
            label: "Value".to_string(),
            ui,
            default,
        }],
        output_default: None,
    }
}

fn default_error(ui: PropertyUiV1, default: serde_json::Value) -> String {
    property_definitions(&component(ui, default))
        .expect_err("invalid descriptor default must be rejected")
        .to_string()
}

fn current_process_library() -> Library {
    #[cfg(unix)]
    {
        libloading::os::unix::Library::this().into()
    }
    #[cfg(windows)]
    {
        libloading::os::windows::Library::this()
            .expect("open current process for an inert registry test handle")
            .into()
    }
    #[cfg(not(any(unix, windows)))]
    {
        panic!("runtime native plugins support only Unix and Windows hosts")
    }
}

fn pending_bundle(descriptor: PluginDescriptorV1) -> PendingBundle {
    PendingBundle {
        manifest_path: PathBuf::from("/runtime-plugin-test/ruvie-plugin.toml"),
        library_path: PathBuf::from("/runtime-plugin-test/plugin.test"),
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
        effect_api: None,
        loader_api: None,
    }
}

fn two_component_descriptor(second_default: serde_json::Value) -> PluginDescriptorV1 {
    let bounded = PropertyUiV1::Float {
        min: 0.0,
        max: 100.0,
        step: 1.0,
        suffix: String::new(),
        min_hard_limit: true,
        max_hard_limit: true,
    };
    let mut first = component(bounded.clone(), serde_json::json!(25.0));
    first.id = "example.first".to_string();
    let mut second = component(bounded, second_default);
    second.id = "example.second".to_string();
    PluginDescriptorV1 {
        name: "Atomic bundle".to_string(),
        vendor: "Tests".to_string(),
        version: "1.0.0".to_string(),
        components: vec![first, second],
    }
}

fn property_component(output_default: Option<PropertyValueV1>) -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: "example.runtime_property".to_string(),
        name: "Runtime Property".to_string(),
        category: PROPERTY_CATEGORY.to_string(),
        group: "Tests".to_string(),
        version: "1.0.0".to_string(),
        operations: vec![PROPERTY_EVALUATE_V1.to_string()],
        properties: vec![PropertyDefinitionV1 {
            name: "amplitude".to_string(),
            label: "Amplitude".to_string(),
            ui: PropertyUiV1::Float {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix: String::new(),
                min_hard_limit: false,
                max_hard_limit: false,
            },
            default: serde_json::json!(1.0),
        }],
        output_default,
    }
}

fn style_component() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: "example.runtime_fill".to_string(),
        name: "Runtime Fill".to_string(),
        category: STYLE_CATEGORY.to_string(),
        group: "Tests".to_string(),
        version: "1.2.3".to_string(),
        operations: vec![STYLE_EVALUATE_V1.to_string()],
        properties: vec![
            PropertyDefinitionV1 {
                name: "color".to_string(),
                label: "Color".to_string(),
                ui: PropertyUiV1::Color,
                default: serde_json::json!({"r": 10, "g": 20, "b": 30, "a": 255}),
            },
            PropertyDefinitionV1 {
                name: "offset".to_string(),
                label: "Offset".to_string(),
                ui: PropertyUiV1::Float {
                    min: -100.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                default: serde_json::json!(2.0),
            },
        ],
        output_default: None,
    }
}

fn decorator_component() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: "example.runtime_backplate".to_string(),
        name: "Runtime Backplate".to_string(),
        category: DECORATOR_CATEGORY.to_string(),
        group: "Tests".to_string(),
        version: "2.3.4".to_string(),
        operations: vec![DECORATOR_EVALUATE_V2.to_string()],
        properties: vec![
            PropertyDefinitionV1 {
                name: "target".to_string(),
                label: "Target".to_string(),
                ui: PropertyUiV1::Dropdown {
                    options: vec!["Block".to_string(), "Line".to_string(), "Char".to_string()],
                },
                default: serde_json::json!("Block"),
            },
            PropertyDefinitionV1 {
                name: "padding".to_string(),
                label: "Padding".to_string(),
                ui: PropertyUiV1::Vec4 {
                    min: -1_000_000.0,
                    max: 1_000_000.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                default: serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0, "w": 4.0}),
            },
            PropertyDefinitionV1 {
                name: "offset".to_string(),
                label: "Offset".to_string(),
                ui: PropertyUiV1::Vec2 {
                    min: -1_000_000.0,
                    max: 1_000_000.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                default: serde_json::json!({"x": 0.0, "y": 0.0}),
            },
            PropertyDefinitionV1 {
                name: "fit".to_string(),
                label: "Fit".to_string(),
                ui: PropertyUiV1::Dropdown {
                    options: vec![
                        "Stretch".to_string(),
                        "Contain".to_string(),
                        "Cover".to_string(),
                    ],
                },
                default: serde_json::json!("Stretch"),
            },
        ],
        output_default: None,
    }
}

fn legacy_decorator_component() -> ComponentDescriptorV1 {
    let mut component = decorator_component();
    component.id = "example.legacy_backplate".to_string();
    component.version = "1.0.0".to_string();
    component.operations = vec![DECORATOR_EVALUATE_V1.to_string()];
    component.properties = vec![
        PropertyDefinitionV1 {
            name: "target".to_string(),
            label: "Target".to_string(),
            ui: PropertyUiV1::Dropdown {
                options: vec!["Block".to_string(), "Line".to_string(), "Char".to_string()],
            },
            default: serde_json::json!("Block"),
        },
        PropertyDefinitionV1 {
            name: "shape".to_string(),
            label: "Shape".to_string(),
            ui: PropertyUiV1::Dropdown {
                options: vec![
                    "Rect".to_string(),
                    "RoundedRect".to_string(),
                    "Circle".to_string(),
                ],
            },
            default: serde_json::json!("RoundedRect"),
        },
        PropertyDefinitionV1 {
            name: "color".to_string(),
            label: "Color".to_string(),
            ui: PropertyUiV1::Color,
            default: serde_json::json!({"r": 1, "g": 2, "b": 3, "a": 4}),
        },
        PropertyDefinitionV1 {
            name: "padding".to_string(),
            label: "Padding".to_string(),
            ui: PropertyUiV1::Vec4 {
                min: -100.0,
                max: 100.0,
                step: 1.0,
                suffix: "px".to_string(),
                min_hard_limit: false,
                max_hard_limit: false,
            },
            default: serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0, "w": 4.0}),
        },
        PropertyDefinitionV1 {
            name: "corner_radius".to_string(),
            label: "Corner Radius".to_string(),
            ui: PropertyUiV1::Float {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix: "px".to_string(),
                min_hard_limit: true,
                max_hard_limit: false,
            },
            default: serde_json::json!(5.0),
        },
    ];
    component
}

fn effect_component() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: "example.runtime_effect".to_string(),
        name: "Runtime Effect".to_string(),
        category: EFFECT_CATEGORY.to_string(),
        group: "Tests".to_string(),
        version: "1.0.0".to_string(),
        operations: vec![EFFECT_PROCESS_CPU_RGBA8_V1.to_string()],
        properties: vec![PropertyDefinitionV1 {
            name: "amount".to_string(),
            label: "Amount".to_string(),
            ui: PropertyUiV1::Float {
                min: 0.0,
                max: 1.0,
                step: 0.1,
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            default: serde_json::json!(0.5),
        }],
        output_default: None,
    }
}

fn loader_component() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: "example.runtime_loader".to_string(),
        name: "Runtime Loader".to_string(),
        category: LOADER_CATEGORY.to_string(),
        group: "Tests".to_string(),
        version: "1.0.0".to_string(),
        operations: vec![
            LOADER_OPEN_V1.to_string(),
            LOADER_LOAD_CPU_RGBA8_V1.to_string(),
        ],
        properties: Vec::new(),
        output_default: None,
    }
}

fn config_descriptor() -> PluginDescriptorV1 {
    PluginDescriptorV1 {
        name: "Runtime config test".to_string(),
        vendor: "Tests".to_string(),
        version: "1.0.0".to_string(),
        components: vec![style_component(), decorator_component()],
    }
}

fn descriptor_with(component: ComponentDescriptorV1) -> PluginDescriptorV1 {
    PluginDescriptorV1 {
        name: "Runtime property test".to_string(),
        vendor: "Tests".to_string(),
        version: "1.0.0".to_string(),
        components: vec![component],
    }
}

unsafe extern "C" fn invalid_property_response(
    _context: *mut std::ffi::c_void,
    _request: RuvieBytesView,
) -> RuvieCallResult {
    RuvieCallResult::ok_json(&serde_json::json!({
        "value": {"type": "future_unknown_value", "value": 99}
    }))
}

unsafe extern "C" fn failing_property_response(
    _context: *mut std::ffi::c_void,
    _request: RuvieBytesView,
) -> RuvieCallResult {
    RuvieCallResult::error(
        ruvie_plugin_api::STATUS_PLUGIN_ERROR,
        "intentional evaluator failure",
    )
}

unsafe extern "C" fn test_free_buffer(_context: *mut std::ffi::c_void, buffer: RuvieBuffer) {
    // SAFETY: `invalid_property_response` allocated this buffer with the
    // SDK helper, and the host returns it to this callback exactly once.
    unsafe { ruvie_plugin_api::free_owned_buffer(buffer) };
}

mod abi_frames;
mod appearance;
mod cache;
mod descriptor;
mod registration;
