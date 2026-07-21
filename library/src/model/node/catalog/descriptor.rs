use super::super::{
    GeneratorContent, ListContent, NativeOperationContent, Node, NodeContent, SoundAnalysisContent,
    ValueContent,
};
use crate::model::project::{
    PortDataType, PortDefinition, PortExposure, PortMultiplicity, PortSide,
};
use crate::model::property::PropertyMap;

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
    List(ListContent),
    Merge,
    SoundMerge,
    SoundAnalysis(SoundAnalysisContent),
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
            NativeNodeFactory::List(operation) => Ok(Node::new_list(self.label, operation)),
            NativeNodeFactory::Merge => Ok(Node::new_merge(self.label)),
            NativeNodeFactory::SoundMerge => Ok(Node::new_sound_merge(self.label)),
            NativeNodeFactory::SoundAnalysis(analysis) => {
                Ok(Node::new_sound_analysis(self.label, analysis))
            }
            NativeNodeFactory::TypedPlaceholder => Ok(Node::with_properties(
                self.label,
                NodeContent::NativeOperation(NativeOperationContent {
                    catalog_id: self.catalog_id.to_string(),
                }),
                PropertyMap::new(),
            )),
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
        }
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
        }
    }
}
