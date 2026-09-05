//! Project-independent factories used by authoring UI and services.
//!
//! These constructors deliberately need no legacy `ProjectManager`: the
//! plugin registry supplies canonical operation/converter contracts and the
//! returned values can be inserted through [`TimelineEditorService`].

use std::collections::HashMap;

use crate::error::LibraryError;
use crate::model::authoring::{
    AppearanceOperation, AutomatableParameter, BuiltinEffectInstance, EffectContractSnapshot,
    OperationRef, ProcessorParameterContract, TextEnsembleOperation,
    appearance_direct_contract_is_compatible, text_ensemble_direct_contract_is_compatible,
};
use crate::model::frame::color::Color;
use crate::model::node::{GeneratorContent, MediaContent, NativeNodeFactory, Node};
use crate::model::project::{PortDataType, PortDirection};
use crate::model::property::{ColorValue, Property, PropertyMap, PropertyValue};
use crate::plugin::entity_converter::measure_text_size;
use crate::plugin::{EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, PROPERTY_PORT_PREFIX, PluginManager};

/// Text Ensemble operation family accepted by the source-level stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEnsembleOperationKind {
    Effector,
    Decorator,
}

/// Builds a Text Ensemble entry through the same descriptor-backed Node
/// construction boundary used by the production Node Editor.
pub struct TextEnsembleOperationFactory;

impl TextEnsembleOperationFactory {
    pub fn create(
        plugins: &PluginManager,
        kind: TextEnsembleOperationKind,
        component_id: &str,
    ) -> Result<TextEnsembleOperation, LibraryError> {
        let (node, version) = Self::create_node_and_version(plugins, kind, component_id)?;
        let crate::model::node::NodeContent::PluginOperation(operation) = node.content() else {
            return Err(LibraryError::Validation(
                "Text Ensemble factory did not create a plugin operation".to_string(),
            ));
        };
        Ok(TextEnsembleOperation {
            id: node.id,
            operation: OperationRef {
                category: operation.category.clone(),
                component_id: operation.component_id.clone(),
                operation: operation.operation.clone(),
                version: format!("{}.{}.{}", version.0, version.1, version.2),
            },
            declared_ports: operation.declared_ports.clone(),
            properties: node.properties().clone(),
        })
    }

    pub(crate) fn create_node(
        plugins: &PluginManager,
        kind: TextEnsembleOperationKind,
        component_id: &str,
    ) -> Result<Node, LibraryError> {
        Self::create_node_and_version(plugins, kind, component_id).map(|(node, _)| node)
    }

    fn create_node_and_version(
        plugins: &PluginManager,
        kind: TextEnsembleOperationKind,
        component_id: &str,
    ) -> Result<(Node, (u32, u32, u32)), LibraryError> {
        let (category, version) = match kind {
            TextEnsembleOperationKind::Effector => {
                let plugin = plugins.get_effector_plugin(component_id).ok_or_else(|| {
                    LibraryError::Plugin(format!("Effector plugin '{component_id}' is unavailable"))
                })?;
                (crate::plugin::EFFECTOR_CATEGORY, plugin.version())
            }
            TextEnsembleOperationKind::Decorator => {
                let plugin = plugins.get_decorator_plugin(component_id).ok_or_else(|| {
                    LibraryError::Plugin(format!(
                        "Decorator plugin '{component_id}' is unavailable"
                    ))
                })?;
                (crate::plugin::DECORATOR_CATEGORY, plugin.version())
            }
        };
        let node = plugins.create_text_ensemble_operation_node(category, component_id)?;
        let crate::model::node::NodeContent::PluginOperation(operation) = node.content() else {
            return Err(LibraryError::Validation(
                "Text Ensemble factory did not create a plugin operation".to_string(),
            ));
        };
        if !text_ensemble_direct_contract_is_compatible(&operation.declared_ports) {
            return Err(LibraryError::Validation(format!(
                "Operation {}/{}/{} requires Node Editor media inputs and cannot run inline on Text",
                operation.category, operation.component_id, operation.operation
            )));
        }
        Ok((node, version))
    }
}

/// Builds one direct-source appearance entry through the same descriptor and
/// Node factory used by the production Node Editor.
pub struct AppearanceOperationFactory;

impl AppearanceOperationFactory {
    pub fn create(
        plugins: &PluginManager,
        component_id: &str,
    ) -> Result<AppearanceOperation, LibraryError> {
        let plugin = plugins.get_style_plugin(component_id).ok_or_else(|| {
            LibraryError::Plugin(format!("Style plugin '{component_id}' is unavailable"))
        })?;
        let version = plugin.version();
        let node = plugins.create_style_operation_node(component_id)?;
        let crate::model::node::NodeContent::PluginOperation(operation) = node.content() else {
            return Err(LibraryError::Validation(
                "Appearance factory did not create a plugin operation".to_string(),
            ));
        };
        if !appearance_direct_contract_is_compatible(&operation.declared_ports) {
            return Err(LibraryError::Validation(format!(
                "Style '{component_id}' requires Node Editor media inputs and cannot run inline"
            )));
        }
        Ok(AppearanceOperation {
            id: node.id,
            operation: OperationRef {
                category: operation.category.clone(),
                component_id: operation.component_id.clone(),
                operation: operation.operation.clone(),
                version: format!("{}.{}.{}", version.0, version.1, version.2),
            },
            declared_ports: operation.declared_ports.clone(),
            properties: node.properties().clone(),
        })
    }
}

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
    /// Builds the Media Node represented by one imported Asset. Asset kind,
    /// stream identity, source path, and media dimensions are resolved here
    /// so editor surfaces never reconstruct persisted media semantics.
    pub fn create_asset_media(
        plugins: &PluginManager,
        asset: &crate::model::asset::Asset,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        use super::project_service::MediaNodeRequest;
        use crate::model::asset::AssetKind;

        let request = match asset.kind {
            AssetKind::Audio => MediaNodeRequest::Audio {
                asset_id: asset.id,
                file_path: asset.path.clone(),
                audio_stream_index: asset.stream_index,
            },
            AssetKind::Video => MediaNodeRequest::Video {
                asset_id: asset.id,
                file_path: asset.path.clone(),
                stream_index: asset.stream_index,
                // Imports expose each media stream as its own Asset. A Video
                // Asset therefore must not guess a sibling audio stream.
                audio_stream_index: None,
                outputs: crate::model::MediaOutputSelection::Image,
            },
            AssetKind::Image => MediaNodeRequest::Image {
                asset_id: asset.id,
                file_path: asset.path.clone(),
            },
            AssetKind::Model3D | AssetKind::Other => {
                return Err(LibraryError::Validation(format!(
                    "Asset kind {:?} cannot be used as a 2D Media Node",
                    asset.kind
                )));
            }
        };
        Self::create_media(
            plugins,
            &asset.name,
            request,
            canvas_width,
            canvas_height,
            asset.width.map(u64::from).unwrap_or(canvas_width),
            asset.height.map(u64::from).unwrap_or(canvas_height),
        )
    }

    /// Builds the authoritative detached Media Node used by both authoring
    /// models. Source identity and converter defaults are materialized in one
    /// step, so callers cannot construct a half-initialized Media Node.
    pub fn create_media(
        plugins: &PluginManager,
        name: &str,
        request: super::project_service::MediaNodeRequest,
        canvas_width: u64,
        canvas_height: u64,
        media_width: u64,
        media_height: u64,
    ) -> Result<Node, LibraryError> {
        use super::project_service::MediaNodeRequest;

        let (converter_kind, converter_required, content, file_path) = match request {
            MediaNodeRequest::Audio {
                asset_id,
                file_path,
                audio_stream_index,
            } => (
                "audio",
                false,
                MediaContent::new(
                    asset_id,
                    crate::model::MediaOutputSelection::Audio,
                    None,
                    audio_stream_index,
                )
                .map_err(LibraryError::Validation)?,
                file_path,
            ),
            MediaNodeRequest::Video {
                asset_id,
                file_path,
                stream_index,
                audio_stream_index,
                outputs,
            } => (
                "video",
                true,
                MediaContent::new(asset_id, outputs, stream_index, audio_stream_index)
                    .map_err(LibraryError::Validation)?,
                file_path,
            ),
            MediaNodeRequest::Image {
                asset_id,
                file_path,
            } => (
                "image",
                true,
                MediaContent::new(
                    asset_id,
                    crate::model::MediaOutputSelection::Image,
                    None,
                    None,
                )
                .map_err(LibraryError::Validation)?,
                file_path,
            ),
        };
        let definitions = match plugins.get_entity_converter(converter_kind) {
            Some(converter) => converter.get_property_definitions(
                canvas_width,
                canvas_height,
                media_width,
                media_height,
            ),
            None if converter_required => {
                return Err(LibraryError::Plugin(format!(
                    "{converter_kind} converter plugin not found"
                )));
            }
            None => Vec::new(),
        };
        Node::from_media_converter(name, content, &definitions, file_path)
            .map_err(LibraryError::Validation)
    }

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
            parameter_contracts.push(ProcessorParameterContract {
                key: definition.name().to_string(),
                data_type: port.data_type,
                default_value: default_value.clone(),
            });
            parameters.insert(
                definition.name().to_string(),
                AutomatableParameter {
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
