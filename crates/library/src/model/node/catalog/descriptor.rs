use super::super::{
    ColorContent, DataContent, GeneratorContent, ListContent, Node, PathOperationContent,
    SoundAnalysisContent, ValueContent,
};
use std::collections::HashSet;

use crate::model::authoring::PublishedParameterAutomationCapability;
use crate::model::project::{
    PortDataType, PortDefinition, PortExposure, PortMultiplicity, PortSide,
};
use crate::model::property::{PropertyDefinition, PropertyMap};

type PropertyDefinitions = fn() -> Vec<PropertyDefinition>;
type PropertySetValidator = fn(&PropertyMap) -> Result<(), String>;

fn no_property_definitions() -> Vec<PropertyDefinition> {
    Vec::new()
}

fn accept_any_property_set(_: &PropertyMap) -> Result<(), String> {
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeNodeRuntimeStatus {
    Implemented,
    DesignNeeded,
}

impl NativeNodeRuntimeStatus {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::DesignNeeded => "design-needed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeNodeFactory {
    Generator(GeneratorContent),
    Value(ValueContent),
    Data(DataContent),
    Color(ColorContent),
    List(ListContent),
    Path(PathOperationContent),
    Merge,
    SoundMerge,
    SoundAnalysis(SoundAnalysisContent),
    /// Executable first-party operation that users may add to a general
    /// Module graph through its canonical catalog descriptor.
    NativeOperation,
    /// Executable first-party operation whose availability must additionally
    /// be validated by a specialized Module host such as Transition.
    HostOperation,
    TypedPlaceholder,
}

#[derive(Clone, Debug)]
pub struct NativeNodeCatalogDescriptor {
    catalog_id: &'static str,
    label: &'static str,
    category: &'static str,
    qa_id: &'static str,
    keywords: &'static [&'static str],
    runtime_status: NativeNodeRuntimeStatus,
    factory: NativeNodeFactory,
    ports: Vec<PortDefinition>,
    property_definitions: PropertyDefinitions,
    property_set_validator: PropertySetValidator,
    constant_only_inputs: &'static [&'static str],
    constant_only_reason: Option<&'static str>,
}

impl NativeNodeCatalogDescriptor {
    pub fn catalog_id(&self) -> &'static str {
        self.catalog_id
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn category(&self) -> &'static str {
        self.category
    }

    pub fn qa_id(&self) -> String {
        if self.runtime_status == NativeNodeRuntimeStatus::DesignNeeded {
            format!("node_editor.menu.create.catalog:{}", self.catalog_id)
        } else {
            self.qa_id.to_string()
        }
    }

    pub fn keywords(&self) -> &'static [&'static str] {
        self.keywords
    }

    pub fn runtime_status(&self) -> NativeNodeRuntimeStatus {
        self.runtime_status
    }

    pub fn factory(&self) -> NativeNodeFactory {
        self.factory
    }

    pub fn ports(&self) -> &[PortDefinition] {
        &self.ports
    }

    pub fn property_definitions(&self) -> Vec<PropertyDefinition> {
        (self.property_definitions)()
    }

    /// Resolve the canonical authored Property behind one native input port.
    /// Native operations may expose either the property name directly or the
    /// shared `property:` graph-port form used by operation descriptors.
    pub fn property_definition_for_input(&self, input_key: &str) -> Option<PropertyDefinition> {
        let property_key = crate::plugin::property_name_from_port(input_key).unwrap_or(input_key);
        self.property_definitions()
            .into_iter()
            .find(|definition| definition.name() == property_key)
    }

    /// Validate the complete persisted property payload for a descriptor-
    /// backed native operation. The catalog is the only schema authority:
    /// imported and service-authored Nodes must neither omit nor invent keys.
    pub fn validate_native_properties(&self, properties: &PropertyMap) -> Result<(), String> {
        let definitions = self.property_definitions();
        let mut declared = HashSet::with_capacity(definitions.len());
        for definition in &definitions {
            definition.validate_definition().map_err(|error| {
                format!(
                    "Native Node '{}' has invalid property metadata: {error}",
                    self.catalog_id
                )
            })?;
            if !declared.insert(definition.name()) {
                return Err(format!(
                    "Native Node '{}' repeats property definition '{}'",
                    self.catalog_id,
                    definition.name()
                ));
            }
        }
        let authored = properties
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<HashSet<_>>();
        if let Some(missing) = declared.difference(&authored).next() {
            return Err(format!(
                "Native Node '{}' is missing required Property '{}'",
                self.catalog_id, missing
            ));
        }
        if let Some(unknown) = authored.difference(&declared).next() {
            return Err(format!(
                "Native Node '{}' has unknown Property '{}'",
                self.catalog_id, unknown
            ));
        }
        for definition in definitions {
            let property = properties.get(definition.name()).ok_or_else(|| {
                format!(
                    "Native Node '{}' is missing required Property '{}'",
                    self.catalog_id,
                    definition.name()
                )
            })?;
            definition
                .validate_authored_property(property)
                .map_err(|error| format!("Native Node '{}': {error}", self.catalog_id))?;
            if property.evaluator != "constant"
                && let Some(reason) = self.dynamic_input_disabled_reason(definition.name())
            {
                return Err(format!(
                    "Native Node '{}' input '{}' must remain constant and cannot use evaluator '{}': {reason}",
                    self.catalog_id,
                    definition.name(),
                    property.evaluator
                ));
            }
        }
        (self.property_set_validator)(properties)
            .map_err(|error| format!("Native Node '{}': {error}", self.catalog_id))
    }

    /// Whether the production Module runtime and factory can create this Node
    /// in a general-purpose Module. UI menus consume this semantic predicate
    /// instead of reconstructing runtime support from catalog ID strings.
    pub fn supports_general_module_creation(&self) -> bool {
        self.runtime_status == NativeNodeRuntimeStatus::Implemented
            && matches!(
                self.factory,
                NativeNodeFactory::Generator(_)
                    | NativeNodeFactory::Value(_)
                    | NativeNodeFactory::Data(_)
                    | NativeNodeFactory::Merge
                    | NativeNodeFactory::SoundMerge
                    | NativeNodeFactory::NativeOperation
            )
    }

    /// Whether a specialized Module host may offer this Node after applying
    /// its own media and protected-boundary validation.
    pub fn supports_host_module_creation(&self) -> bool {
        self.supports_general_module_creation()
            || self.runtime_status == NativeNodeRuntimeStatus::Implemented
                && matches!(self.factory, NativeNodeFactory::HostOperation)
    }

    pub fn input_automation_capability(
        &self,
        input_key: &str,
    ) -> PublishedParameterAutomationCapability {
        if self.constant_only_inputs.contains(&input_key) {
            PublishedParameterAutomationCapability::ConstantOnly {
                reason: self.constant_only_reason.unwrap_or(
                    "the native runtime does not support frame-sampled values for this input",
                ),
            }
        } else {
            PublishedParameterAutomationCapability::FrameSampled
        }
    }

    /// Explains why an input must remain a direct constant Property/Instance
    /// value instead of accepting an Expression, automation track, or graph
    /// connection. All authoring frontends consume this same runtime contract.
    pub fn dynamic_input_disabled_reason(&self, input_key: &str) -> Option<&'static str> {
        match self.input_automation_capability(input_key) {
            PublishedParameterAutomationCapability::FrameSampled => None,
            PublishedParameterAutomationCapability::ConstantOnly { reason } => Some(reason),
        }
    }

    pub fn runtime_diagnostic(&self) -> Option<String> {
        (self.runtime_status == NativeNodeRuntimeStatus::DesignNeeded).then(|| {
            format!(
                "{} runtime/renderer is design-needed; evaluation produces No Output",
                self.label
            )
        })
    }

    pub fn create_detached_node(&self) -> Result<Node, String> {
        match self.factory {
            NativeNodeFactory::Generator(_) => Err(format!(
                "Native Generator '{}' requires its canvas-backed ProjectManager factory",
                self.catalog_id
            )),
            NativeNodeFactory::Value(value) => Ok(Node::new_value(self.label, value)),
            NativeNodeFactory::Data(data) => Ok(Node::new_data(self.label, data)),
            NativeNodeFactory::Color(operation) => Ok(Node::new_color(self.label, operation)),
            NativeNodeFactory::List(operation) => Ok(Node::new_list(self.label, operation)),
            NativeNodeFactory::Path(operation) => {
                Ok(Node::new_path_operation(self.label, operation))
            }
            NativeNodeFactory::Merge => Ok(Node::new_merge(self.label)),
            NativeNodeFactory::SoundMerge => Ok(Node::new_sound_merge(self.label)),
            NativeNodeFactory::SoundAnalysis(analysis) => {
                Ok(Node::new_sound_analysis(self.label, analysis))
            }
            NativeNodeFactory::NativeOperation
            | NativeNodeFactory::HostOperation
            | NativeNodeFactory::TypedPlaceholder => Node::new_native_operation(
                self.label,
                self.catalog_id,
                &(self.property_definitions)(),
            ),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct PortSpec {
    key: &'static str,
    label: &'static str,
    data_type: PortDataType,
    multiplicity: PortMultiplicity,
}

impl PortSpec {
    pub(super) const fn single(
        key: &'static str,
        label: &'static str,
        data_type: PortDataType,
    ) -> Self {
        Self {
            key,
            label,
            data_type,
            multiplicity: PortMultiplicity::Single,
        }
    }

    pub(super) const fn variadic(
        key: &'static str,
        label: &'static str,
        data_type: PortDataType,
    ) -> Self {
        Self {
            key,
            label,
            data_type,
            multiplicity: PortMultiplicity::Variadic,
        }
    }

    fn input(self) -> PortDefinition {
        let mut definition = PortDefinition::input(self.key, self.label, self.data_type);
        definition.multiplicity = self.multiplicity;
        definition
    }

    fn output(self) -> PortDefinition {
        let mut definition = PortDefinition::output(
            self.key,
            self.label,
            self.data_type,
            PortSide::Right,
            PortExposure::Graph,
        );
        definition.multiplicity = self.multiplicity;
        definition
    }
}

#[derive(Clone, Copy)]
pub(super) struct DescriptorIdentity {
    catalog_id: &'static str,
    label: &'static str,
    category: &'static str,
    qa_id: &'static str,
    keywords: &'static [&'static str],
}

impl DescriptorIdentity {
    pub(super) const fn new(
        catalog_id: &'static str,
        label: &'static str,
        category: &'static str,
        qa_id: &'static str,
        keywords: &'static [&'static str],
    ) -> Self {
        Self {
            catalog_id,
            label,
            category,
            qa_id,
            keywords,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct DescriptorSpec {
    catalog_id: &'static str,
    label: &'static str,
    category: &'static str,
    qa_id: &'static str,
    keywords: &'static [&'static str],
    runtime_status: NativeNodeRuntimeStatus,
    factory: NativeNodeFactory,
    inputs: &'static [PortSpec],
    outputs: &'static [PortSpec],
    property_definitions: PropertyDefinitions,
    property_set_validator: PropertySetValidator,
    constant_only_inputs: &'static [&'static str],
    constant_only_reason: Option<&'static str>,
}

impl DescriptorSpec {
    pub(super) const fn implemented(
        identity: DescriptorIdentity,
        factory: NativeNodeFactory,
        inputs: &'static [PortSpec],
        outputs: &'static [PortSpec],
    ) -> Self {
        Self {
            catalog_id: identity.catalog_id,
            label: identity.label,
            category: identity.category,
            qa_id: identity.qa_id,
            keywords: identity.keywords,
            runtime_status: NativeNodeRuntimeStatus::Implemented,
            factory,
            inputs,
            outputs,
            property_definitions: no_property_definitions,
            property_set_validator: accept_any_property_set,
            constant_only_inputs: &[],
            constant_only_reason: None,
        }
    }

    pub(super) const fn implemented_native(
        identity: DescriptorIdentity,
        inputs: &'static [PortSpec],
        outputs: &'static [PortSpec],
        property_definitions: PropertyDefinitions,
    ) -> Self {
        Self {
            catalog_id: identity.catalog_id,
            label: identity.label,
            category: identity.category,
            qa_id: identity.qa_id,
            keywords: identity.keywords,
            runtime_status: NativeNodeRuntimeStatus::Implemented,
            factory: NativeNodeFactory::NativeOperation,
            inputs,
            outputs,
            property_definitions,
            property_set_validator: accept_any_property_set,
            constant_only_inputs: &[],
            constant_only_reason: None,
        }
    }

    pub(super) const fn placeholder(
        catalog_id: &'static str,
        label: &'static str,
        category: &'static str,
        inputs: &'static [PortSpec],
        outputs: &'static [PortSpec],
    ) -> Self {
        Self {
            catalog_id,
            label,
            category,
            qa_id: "node_editor.menu.create.catalog_placeholder",
            keywords: &["typed", "placeholder", "design-needed"],
            runtime_status: NativeNodeRuntimeStatus::DesignNeeded,
            factory: NativeNodeFactory::TypedPlaceholder,
            inputs,
            outputs,
            property_definitions: no_property_definitions,
            property_set_validator: accept_any_property_set,
            constant_only_inputs: &[],
            constant_only_reason: None,
        }
    }

    pub(super) const fn implemented_host_native(
        identity: DescriptorIdentity,
        inputs: &'static [PortSpec],
        outputs: &'static [PortSpec],
        property_definitions: PropertyDefinitions,
    ) -> Self {
        Self {
            catalog_id: identity.catalog_id,
            label: identity.label,
            category: identity.category,
            qa_id: identity.qa_id,
            keywords: identity.keywords,
            runtime_status: NativeNodeRuntimeStatus::Implemented,
            factory: NativeNodeFactory::HostOperation,
            inputs,
            outputs,
            property_definitions,
            property_set_validator: accept_any_property_set,
            constant_only_inputs: &[],
            constant_only_reason: None,
        }
    }

    pub(super) const fn constant_only_inputs(
        mut self,
        inputs: &'static [&'static str],
        reason: &'static str,
    ) -> Self {
        self.constant_only_inputs = inputs;
        self.constant_only_reason = Some(reason);
        self
    }

    /// Attach an invariant which depends on more than one native Property.
    /// Canonical types, finite values, and hard bounds are checked first.
    pub(super) const fn validate_property_set(mut self, validator: PropertySetValidator) -> Self {
        self.property_set_validator = validator;
        self
    }

    pub(super) fn build(self) -> NativeNodeCatalogDescriptor {
        let ports = match self.factory {
            NativeNodeFactory::SoundAnalysis(analysis) => analysis.port_definitions().to_vec(),
            _ => self
                .inputs
                .iter()
                .copied()
                .map(PortSpec::input)
                .chain(self.outputs.iter().copied().map(PortSpec::output))
                .collect(),
        };
        NativeNodeCatalogDescriptor {
            catalog_id: self.catalog_id,
            label: self.label,
            category: self.category,
            qa_id: self.qa_id,
            keywords: self.keywords,
            runtime_status: self.runtime_status,
            factory: self.factory,
            ports,
            property_definitions: self.property_definitions,
            property_set_validator: self.property_set_validator,
            constant_only_inputs: self.constant_only_inputs,
            constant_only_reason: self.constant_only_reason,
        }
    }
}
