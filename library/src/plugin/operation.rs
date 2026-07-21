//! Runtime descriptions and construction for plugin-backed graph operations.
//!
//! A descriptor is the single source of truth for an operation's label,
//! persisted port contract, and authored property defaults. Projects persist
//! the resulting operation identity and ports, but never require the plugin to
//! be installed merely to load or validate that data.

use crate::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, FPS_PORT, FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT,
    PortDataType, PortDefinition, PortDirection, PortExposure, PortSide, SHAPE_INPUT_PORT,
    SHAPE_OUTPUT_PORT, TIME_PORT,
};
use crate::model::property::{PropertyDefinition, PropertyUiType};
use crate::model::{Node, PluginOperationContent};
use std::collections::HashSet;
use thiserror::Error;

pub const STYLE_CATEGORY: &str = "style";
pub const STYLE_APPLY_OPERATION: &str = "style.apply.v1";
pub const EFFECT_CATEGORY: &str = "effect";
pub const EFFECT_APPLY_OPERATION: &str = "effect.apply.v1";
pub const EFFECTOR_CATEGORY: &str = "effector";
pub const EFFECTOR_APPLY_OPERATION: &str = "effector.apply.v1";
pub const PATH_EFFECT_CATEGORY: &str = "path_effect";
pub const PATH_EFFECT_APPLY_OPERATION: &str = "path_effect.apply.v1";
pub const DECORATOR_CATEGORY: &str = "decorator";
pub const DECORATOR_APPLY_OPERATION: &str = "decorator.apply.v1";
/// Native whole-Shape spatial placement. This is intentionally distinct from
/// `EFFECTOR_CATEGORY`: Effectors modulate grouped elements, while this
/// operation owns the absolute root transform edited by Preview/Inspector.
pub const TRANSFORM_CATEGORY: &str = "transform";
/// Native whole-Shape placement. The persisted value remains `transform`
/// because it already identifies the pre-v1 Shape operation; the Rust name
/// makes its type boundary explicit next to Image Transform.
pub const SHAPE_TRANSFORM_COMPONENT_ID: &str = "transform";
/// Native whole-Image placement. Unlike Shape Transform, this operation wraps
/// and transforms the complete raster subtree connected to its Image input.
pub const IMAGE_TRANSFORM_COMPONENT_ID: &str = "image";
pub const TRANSFORM_APPLY_OPERATION: &str = "transform.apply.v1";
pub const PROPERTY_PORT_PREFIX: &str = "property:";

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum OperationDescriptorError {
    #[error("operation descriptor {field} must not be empty")]
    EmptyIdentity { field: &'static str },
    #[error("operation descriptor has duplicate property {name:?}")]
    DuplicateProperty { name: String },
    #[error("operation property {name:?} is invalid: {reason}")]
    InvalidProperty { name: String, reason: String },
    #[error("operation descriptor has duplicate or colliding port {key:?}")]
    PortCollision { key: String },
    #[error("operation descriptor operation port {key:?} has an invalid key")]
    InvalidOperationPortKey { key: String },
    #[error("operation descriptor port {key:?} label must not be empty")]
    EmptyPortLabel { key: String },
    #[error("operation descriptor derived timing port {key:?} is read-only and cannot be an input")]
    ReadOnlyDerivedTimingInput { key: String },
    #[error(
        "operation descriptor derived timing output {key:?} must use {expected:?}, got {actual:?}"
    )]
    InvalidDerivedTimingOutputType {
        key: String,
        expected: PortDataType,
        actual: PortDataType,
    },
}

#[derive(Clone, Debug)]
pub struct OperationDescriptor {
    category: String,
    component_id: String,
    operation: String,
    label: String,
    declared_ports: Vec<PortDefinition>,
    properties: Vec<PropertyDefinition>,
}

/// Opaque construction capability produced only from a validated operation
/// descriptor. Keeping its fields and constructor private prevents other
/// library modules from pairing arbitrary operation identity with a detached
/// property map while still letting the model own final Node construction.
pub(crate) struct OperationNodeParts {
    label: String,
    content: PluginOperationContent,
    properties: Vec<PropertyDefinition>,
}

impl OperationNodeParts {
    fn from_descriptor(descriptor: &OperationDescriptor) -> Self {
        Self {
            label: descriptor.label.clone(),
            content: PluginOperationContent {
                category: descriptor.category.clone(),
                component_id: descriptor.component_id.clone(),
                operation: descriptor.operation.clone(),
                declared_ports: descriptor.declared_ports.clone(),
            },
            properties: descriptor.properties.clone(),
        }
    }

    pub(crate) fn into_node_data(
        self,
    ) -> (String, PluginOperationContent, Vec<PropertyDefinition>) {
        (self.label, self.content, self.properties)
    }
}

impl OperationDescriptor {
    /// Builds a descriptor from one authoritative list of property
    /// definitions. Every property becomes a typed graph input, followed by
    /// operation-specific input and output ports.
    pub fn new(
        category: impl Into<String>,
        component_id: impl Into<String>,
        operation: impl Into<String>,
        label: impl Into<String>,
        properties: Vec<PropertyDefinition>,
        operation_ports: impl IntoIterator<Item = PortDefinition>,
    ) -> Result<Self, OperationDescriptorError> {
        let mut declared_ports = properties
            .iter()
            .map(|definition| {
                PortDefinition::input(
                    &property_port_key(definition.name()),
                    definition.label(),
                    property_ui_type_to_port_data_type(definition.ui_type()),
                )
            })
            .collect::<Vec<_>>();
        declared_ports.extend(operation_ports);
        let descriptor = Self {
            category: category.into(),
            component_id: component_id.into(),
            operation: operation.into(),
            label: label.into(),
            declared_ports,
            properties,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn style(
        component_id: impl Into<String>,
        label: impl Into<String>,
        properties: Vec<PropertyDefinition>,
    ) -> Result<Self, OperationDescriptorError> {
        Self::new(
            STYLE_CATEGORY,
            component_id,
            STYLE_APPLY_OPERATION,
            label,
            properties,
            [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::input(SHAPE_INPUT_PORT, "Shape", PortDataType::Shape),
                PortDefinition::output(
                    IMAGE_OUTPUT_PORT,
                    "Image",
                    PortDataType::Image,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
        )
    }

    pub fn effect(
        component_id: impl Into<String>,
        label: impl Into<String>,
        properties: Vec<PropertyDefinition>,
    ) -> Result<Self, OperationDescriptorError> {
        Self::new(
            EFFECT_CATEGORY,
            component_id,
            EFFECT_APPLY_OPERATION,
            label,
            properties,
            [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::input(IMAGE_INPUT_PORT, "Image", PortDataType::Image),
                PortDefinition::output(
                    IMAGE_OUTPUT_PORT,
                    "Image",
                    PortDataType::Image,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
        )
    }

    pub fn effector(
        component_id: impl Into<String>,
        label: impl Into<String>,
        properties: Vec<PropertyDefinition>,
    ) -> Result<Self, OperationDescriptorError> {
        Self::new(
            EFFECTOR_CATEGORY,
            component_id,
            EFFECTOR_APPLY_OPERATION,
            label,
            properties,
            [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::input(SHAPE_INPUT_PORT, "Shape", PortDataType::Shape),
                PortDefinition::output(
                    SHAPE_OUTPUT_PORT,
                    "Shape",
                    PortDataType::Shape,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
        )
    }

    pub fn path_effect(
        component_id: impl Into<String>,
        label: impl Into<String>,
        properties: Vec<PropertyDefinition>,
    ) -> Result<Self, OperationDescriptorError> {
        Self::new(
            PATH_EFFECT_CATEGORY,
            component_id,
            PATH_EFFECT_APPLY_OPERATION,
            label,
            properties,
            [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::input(SHAPE_INPUT_PORT, "Shape", PortDataType::Shape),
                PortDefinition::output(
                    SHAPE_OUTPUT_PORT,
                    "Shape",
                    PortDataType::Shape,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
        )
    }

    pub fn decorator(
        component_id: impl Into<String>,
        label: impl Into<String>,
        properties: Vec<PropertyDefinition>,
    ) -> Result<Self, OperationDescriptorError> {
        Self::new(
            DECORATOR_CATEGORY,
            component_id,
            DECORATOR_APPLY_OPERATION,
            label,
            properties,
            [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::input(SHAPE_INPUT_PORT, "Shape", PortDataType::Shape),
                PortDefinition::output(
                    SHAPE_OUTPUT_PORT,
                    "Shape",
                    PortDataType::Shape,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
        )
    }

    /// Built-in geometry-only Backplate contract. The first Shape supplies the
    /// layout/group metadata; the second is arbitrary authored geometry to fit
    /// into each selected target. Appearance remains entirely downstream.
    pub fn backplate(
        component_id: impl Into<String>,
        label: impl Into<String>,
        properties: Vec<PropertyDefinition>,
    ) -> Result<Self, OperationDescriptorError> {
        Self::new(
            DECORATOR_CATEGORY,
            component_id,
            DECORATOR_APPLY_OPERATION,
            label,
            properties,
            [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::input(SHAPE_INPUT_PORT, "Target", PortDataType::Shape),
                PortDefinition::input(
                    BACKGROUND_SHAPE_INPUT_PORT,
                    "Background",
                    PortDataType::Shape,
                ),
                PortDefinition::output(
                    SHAPE_OUTPUT_PORT,
                    "Background",
                    PortDataType::Shape,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
        )
    }

    pub fn shape_transform(
        properties: Vec<PropertyDefinition>,
    ) -> Result<Self, OperationDescriptorError> {
        Self::new(
            TRANSFORM_CATEGORY,
            SHAPE_TRANSFORM_COMPONENT_ID,
            TRANSFORM_APPLY_OPERATION,
            "Shape Transform",
            properties,
            [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::input(SHAPE_INPUT_PORT, "Shape", PortDataType::Shape),
                PortDefinition::output(
                    SHAPE_OUTPUT_PORT,
                    "Shape",
                    PortDataType::Shape,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
        )
    }

    pub fn image_transform(
        properties: Vec<PropertyDefinition>,
    ) -> Result<Self, OperationDescriptorError> {
        Self::new(
            TRANSFORM_CATEGORY,
            IMAGE_TRANSFORM_COMPONENT_ID,
            TRANSFORM_APPLY_OPERATION,
            "Image Transform",
            properties,
            [
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::input(IMAGE_INPUT_PORT, "Image", PortDataType::Image),
                PortDefinition::output(
                    IMAGE_OUTPUT_PORT,
                    "Image",
                    PortDataType::Image,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
        )
    }

    pub fn category(&self) -> &str {
        &self.category
    }

    pub fn component_id(&self) -> &str {
        &self.component_id
    }

    pub fn operation(&self) -> &str {
        &self.operation
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn declared_ports(&self) -> &[PortDefinition] {
        &self.declared_ports
    }

    pub fn properties(&self) -> &[PropertyDefinition] {
        &self.properties
    }

    /// Checks the persisted execution contract while deliberately ignoring
    /// display-only labels, port ordering, and layout hints. A plugin may
    /// improve those without turning an existing Project into NoOutput.
    pub fn is_execution_compatible_with_ports(&self, persisted: &[PortDefinition]) -> bool {
        self.declared_ports.len() == persisted.len()
            && self.declared_ports.iter().all(|expected| {
                persisted.iter().any(|actual| {
                    actual.key == expected.key
                        && actual.direction == expected.direction
                        && actual.data_type == expected.data_type
                        && actual.multiplicity == expected.multiplicity
                })
            })
    }

    /// Creates a fully initialized graph node. Defaults are always
    /// materialized from the same definitions that produced the input ports.
    pub(super) fn create_node(&self) -> Result<Node, OperationDescriptorError> {
        self.validate()?;
        Node::from_operation_parts(OperationNodeParts::from_descriptor(self)).map_err(|reason| {
            OperationDescriptorError::InvalidProperty {
                name: "<factory>".to_string(),
                reason,
            }
        })
    }

    fn validate(&self) -> Result<(), OperationDescriptorError> {
        for (field, value) in [
            ("category", self.category.as_str()),
            ("component_id", self.component_id.as_str()),
            ("operation", self.operation.as_str()),
            ("label", self.label.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(OperationDescriptorError::EmptyIdentity { field });
            }
        }

        let mut property_names = HashSet::new();
        for definition in &self.properties {
            let name = definition.name();
            if name.is_empty() {
                return Err(OperationDescriptorError::InvalidProperty {
                    name: name.to_string(),
                    reason: "name must not be empty".to_string(),
                });
            }
            if !property_names.insert(name) {
                return Err(OperationDescriptorError::DuplicateProperty {
                    name: name.to_string(),
                });
            }
            definition.validate_definition().map_err(|reason| {
                OperationDescriptorError::InvalidProperty {
                    name: name.to_string(),
                    reason,
                }
            })?;
        }

        // PortAddress intentionally has no direction field. Connection source
        // and target roles imply direction, but address identity is also used
        // by graph mutation and validation. Therefore one owner may not reuse
        // a key for an input and output even though their directions differ.
        let mut port_keys = HashSet::new();
        for port in &self.declared_ports {
            if !port_keys.insert(port.key.as_str()) {
                return Err(OperationDescriptorError::PortCollision {
                    key: port.key.clone(),
                });
            }
        }
        let property_count = self.properties.len();
        for port in self.declared_ports.iter().skip(property_count) {
            if !valid_operation_port_key(&port.key) {
                return Err(OperationDescriptorError::InvalidOperationPortKey {
                    key: port.key.clone(),
                });
            }
            if port.label.trim().is_empty() {
                return Err(OperationDescriptorError::EmptyPortLabel {
                    key: port.key.clone(),
                });
            }
            let derived_type = match port.key.as_str() {
                FRAME_PORT => Some(PortDataType::Integer),
                FPS_PORT => Some(PortDataType::Number),
                _ => None,
            };
            if let Some(expected) = derived_type {
                if port.direction == PortDirection::Input {
                    return Err(OperationDescriptorError::ReadOnlyDerivedTimingInput {
                        key: port.key.clone(),
                    });
                }
                if port.data_type != expected {
                    return Err(OperationDescriptorError::InvalidDerivedTimingOutputType {
                        key: port.key.clone(),
                        expected,
                        actual: port.data_type,
                    });
                }
            }
        }
        Ok(())
    }
}

pub fn property_port_key(property_name: &str) -> String {
    format!("{PROPERTY_PORT_PREFIX}{property_name}")
}

pub fn property_name_from_port(port_key: &str) -> Option<&str> {
    port_key.strip_prefix(PROPERTY_PORT_PREFIX)
}

fn valid_operation_port_key(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with(PROPERTY_PORT_PREFIX)
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub fn property_ui_type_to_port_data_type(ui_type: &PropertyUiType) -> PortDataType {
    match ui_type {
        PropertyUiType::Float { .. } => PortDataType::Number,
        PropertyUiType::Integer { .. } => PortDataType::Integer,
        PropertyUiType::Color => PortDataType::Color,
        PropertyUiType::Text
        | PropertyUiType::MultilineText
        | PropertyUiType::Dropdown { .. }
        | PropertyUiType::Font => PortDataType::String,
        PropertyUiType::Bool => PortDataType::Boolean,
        PropertyUiType::Vec2 { .. } => PortDataType::Vec2,
        PropertyUiType::Vec3 { .. } => PortDataType::Vec3,
        PropertyUiType::Vec4 { .. } => PortDataType::Vec4,
    }
}
