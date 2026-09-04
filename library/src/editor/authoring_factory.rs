//! Project-independent factories used by Timeline-first authoring UI.
//!
//! These constructors deliberately need no legacy `ProjectManager`: the
//! plugin registry supplies canonical operation/converter contracts and the
//! returned values can be inserted through [`TimelineEditorService`].

use std::collections::HashMap;

use crate::error::LibraryError;
use crate::model::authoring::{
    BuiltinEffectInstance, BuiltinEffectParameter, EffectContractSnapshot, EffectParameterContract,
    OperationRef,
};
use crate::model::frame::color::Color;
use crate::model::node::{GeneratorContent, NativeNodeFactory, Node};
use crate::model::project::{PortDataType, PortDirection};
use crate::model::property::{ColorValue, Property, PropertyMap, PropertyValue};
use crate::plugin::entity_converter::measure_text_size;
use crate::plugin::{EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, PROPERTY_PORT_PREFIX, PluginManager};

/// Authoring payload for a detached Module Node. Canvas dimensions are a
/// separate argument because they are context, not persisted Node identity.
#[derive(Clone, Debug, PartialEq)]
pub enum ModuleNodeRequest {
    NativeCatalog {
        catalog_id: String,
    },
    PluginOperation {
        category: String,
        component_id: String,
        operation: String,
    },
    Text {
        text: String,
        font: String,
    },
    Shape {
        path: String,
        width: u64,
        height: u64,
    },
    Solid {
        color: Color,
    },
    SkSL {
        shader: String,
    },
}

/// Constructs Nodes without creating a legacy Project or graph container.
pub struct AuthoringNodeFactory;

impl AuthoringNodeFactory {
    pub fn create(
        plugins: &PluginManager,
        request: ModuleNodeRequest,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        if canvas_width == 0 || canvas_height == 0 {
            return Err(LibraryError::Validation(
                "Module Node canvas dimensions must be positive".to_string(),
            ));
        }
        match request {
            ModuleNodeRequest::NativeCatalog { catalog_id } => {
                let descriptor = crate::model::node::native_node_descriptor(&catalog_id)
                    .ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Unknown native Node catalog id '{catalog_id}'"
                        ))
                    })?;
                if matches!(descriptor.factory(), NativeNodeFactory::Generator(_)) {
                    return Err(LibraryError::Validation(format!(
                        "Native Generator '{catalog_id}' requires its typed authoring request"
                    )));
                }
                descriptor
                    .create_detached_node()
                    .map_err(LibraryError::Validation)
            }
            ModuleNodeRequest::PluginOperation {
                category,
                component_id,
                operation,
            } => plugins.create_operation_node(&category, &component_id, &operation),
            ModuleNodeRequest::Text { text, font } => {
                let (width, height) = measure_text_size(&text, &font, 100.0);
                create_generator(
                    plugins,
                    GeneratorSpec {
                        name: "Text",
                        converter_kind: "text",
                        content: GeneratorContent::Text,
                        canvas_width,
                        canvas_height,
                        content_width: positive_extent(width),
                        content_height: positive_extent(height),
                    },
                    [
                        ("text", PropertyValue::String(text)),
                        ("font_family", PropertyValue::String(font)),
                    ],
                )
            }
            ModuleNodeRequest::Shape {
                path,
                width,
                height,
            } => {
                if width == 0 || height == 0 {
                    return Err(LibraryError::Validation(
                        "Shape dimensions must be positive".to_string(),
                    ));
                }
                let path =
                    crate::model::path::parse_legacy_svg_path_data(&path).map_err(|error| {
                        LibraryError::Validation(format!("Invalid Shape SVG path: {error}"))
                    })?;
                create_generator(
                    plugins,
                    GeneratorSpec {
                        name: "Shape",
                        converter_kind: "shape",
                        content: GeneratorContent::Shape,
                        canvas_width,
                        canvas_height,
                        content_width: width,
                        content_height: height,
                    },
                    [("path", PropertyValue::Path(path))],
                )
            }
            ModuleNodeRequest::Solid { color } => create_generator(
                plugins,
                GeneratorSpec {
                    name: "Solid",
                    converter_kind: "solid",
                    content: GeneratorContent::Solid,
                    canvas_width,
                    canvas_height,
                    content_width: canvas_width,
                    content_height: canvas_height,
                },
                [(
                    "color",
                    PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color)),
                )],
            ),
            ModuleNodeRequest::SkSL { shader } => create_generator(
                plugins,
                GeneratorSpec {
                    name: "SkSL",
                    converter_kind: "sksl",
                    content: GeneratorContent::SkSL,
                    canvas_width,
                    canvas_height,
                    content_width: canvas_width,
                    content_height: canvas_height,
                },
                [("shader", PropertyValue::String(shader))],
            ),
        }
    }
}

/// Builds a lightweight Effect Stack entry from the authoritative operation
/// descriptor. The caller never assembles a persisted contract by hand.
pub struct BuiltinEffectFactory;

impl BuiltinEffectFactory {
    pub fn create(
        plugins: &PluginManager,
        effect_id: &str,
    ) -> Result<BuiltinEffectInstance, LibraryError> {
        let plugin = plugins.get_effect_plugin(effect_id).ok_or_else(|| {
            LibraryError::Plugin(format!("Effect plugin '{effect_id}' is not available"))
        })?;
        let (major, minor, patch) = plugin.version();
        let descriptor =
            plugins.operation_descriptor(EFFECT_CATEGORY, effect_id, EFFECT_APPLY_OPERATION)?;
        let media_inputs = descriptor
            .declared_ports()
            .iter()
            .filter(|port| {
                port.direction == PortDirection::Input && is_attachment_media(port.data_type)
            })
            .collect::<Vec<_>>();
        let media_outputs = descriptor
            .declared_ports()
            .iter()
            .filter(|port| {
                port.direction == PortDirection::Output && is_attachment_media(port.data_type)
            })
            .collect::<Vec<_>>();
        let [input] = media_inputs.as_slice() else {
            return Err(LibraryError::Validation(format!(
                "Effect '{effect_id}' must declare exactly one media input"
            )));
        };
        let [output] = media_outputs.as_slice() else {
            return Err(LibraryError::Validation(format!(
                "Effect '{effect_id}' must declare exactly one media output"
            )));
        };
        if input.data_type != output.data_type {
            return Err(LibraryError::Validation(format!(
                "Effect '{effect_id}' must preserve its media type"
            )));
        }

        let mut parameters = HashMap::with_capacity(descriptor.properties().len());
        let mut parameter_contracts = Vec::with_capacity(descriptor.properties().len());
        for definition in descriptor.properties() {
            let port_key = format!("{PROPERTY_PORT_PREFIX}{}", definition.name());
            let port = descriptor
                .declared_ports()
                .iter()
                .find(|port| port.key == port_key && port.direction == PortDirection::Input)
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Effect '{effect_id}' has no typed port for property '{}'",
                        definition.name()
                    ))
                })?;
            let default_value = definition.default_value().clone();
            parameter_contracts.push(EffectParameterContract {
                key: definition.name().to_string(),
                data_type: port.data_type,
                default_value: default_value.clone(),
            });
            parameters.insert(
                definition.name().to_string(),
                BuiltinEffectParameter {
                    value: default_value,
                    automation: None,
                },
            );
        }

        Ok(BuiltinEffectInstance {
            operation: OperationRef {
                category: descriptor.category().to_string(),
                component_id: descriptor.component_id().to_string(),
                operation: descriptor.operation().to_string(),
                version: format!("{major}.{minor}.{patch}"),
            },
            contract: EffectContractSnapshot {
                input_type: input.data_type,
                output_type: output.data_type,
                parameters: parameter_contracts,
            },
            parameters,
            blend_mode: crate::model::BlendMode::Normal,
        })
    }
}

struct GeneratorSpec<'a> {
    name: &'a str,
    converter_kind: &'a str,
    content: GeneratorContent,
    canvas_width: u64,
    canvas_height: u64,
    content_width: u64,
    content_height: u64,
}

fn create_generator<const N: usize>(
    plugins: &PluginManager,
    spec: GeneratorSpec<'_>,
    authored_values: [(&str, PropertyValue); N],
) -> Result<Node, LibraryError> {
    let converter = plugins
        .get_entity_converter(spec.converter_kind)
        .ok_or_else(|| LibraryError::Plugin(format!("{} converter plugin not found", spec.name)))?;
    let definitions = converter.get_property_definitions(
        spec.canvas_width,
        spec.canvas_height,
        spec.content_width,
        spec.content_height,
    );
    let mut properties = PropertyMap::from_definitions(&definitions);
    for (key, value) in authored_values {
        properties.set(key.to_string(), Property::constant(value));
    }
    Node::new_generator(spec.name, spec.content, &definitions, properties)
        .map_err(LibraryError::Validation)
}

fn positive_extent(value: f32) -> u64 {
    value.ceil().max(1.0) as u64
}

fn is_attachment_media(data_type: PortDataType) -> bool {
    matches!(data_type, PortDataType::Image | PortDataType::Audio)
}
