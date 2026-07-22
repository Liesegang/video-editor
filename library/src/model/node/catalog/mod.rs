//! Central first-party Node catalog.
//!
//! Menu presentation, detached factories, QA identity, persisted placeholder
//! identity, and graph ports all consume these descriptors. Runtime-specific
//! implementations may remain elsewhere, but must keep this contract.

use std::sync::LazyLock;

use super::{GeneratorContent, Node, NodeContent};

mod builtins;
mod descriptor;
mod particle;
mod three_d;

pub use descriptor::{NativeNodeCatalogDescriptor, NativeNodeFactory, NativeNodeRuntimeStatus};

use descriptor::DescriptorSpec;

static NATIVE_NODE_CATALOG: LazyLock<Vec<NativeNodeCatalogDescriptor>> = LazyLock::new(|| {
    builtins::specs()
        .chain(particle::specs().iter())
        .chain(three_d::specs().iter())
        .copied()
        .map(DescriptorSpec::build)
        .collect()
});

pub fn native_node_catalog() -> &'static [NativeNodeCatalogDescriptor] {
    NATIVE_NODE_CATALOG.as_slice()
}

pub fn native_node_descriptor(catalog_id: &str) -> Option<&'static NativeNodeCatalogDescriptor> {
    native_node_catalog()
        .iter()
        .find(|descriptor| descriptor.catalog_id() == catalog_id)
}

pub fn native_node_descriptor_for_node(
    node: &Node,
) -> Option<&'static NativeNodeCatalogDescriptor> {
    match node.content() {
        NodeContent::Generator(generator) => {
            let catalog_id = match generator {
                GeneratorContent::Text => "native.text",
                GeneratorContent::Solid => "native.solid-color",
                GeneratorContent::Shape => "native.shape",
                GeneratorContent::SkSL => "native.sksl-shader",
            };
            native_node_descriptor(catalog_id)
        }
        NodeContent::Value(value) => native_node_catalog().iter().find(|descriptor| {
            matches!(descriptor.factory(), NativeNodeFactory::Value(candidate) if candidate == *value)
        }),
        NodeContent::Data(data) => native_node_catalog().iter().find(|descriptor| {
            matches!(descriptor.factory(), NativeNodeFactory::Data(candidate) if candidate == *data)
        }),
        NodeContent::Color(operation) => native_node_catalog().iter().find(|descriptor| {
            matches!(descriptor.factory(), NativeNodeFactory::Color(candidate) if candidate == *operation)
        }),
        NodeContent::List(operation) => native_node_catalog().iter().find(|descriptor| {
            matches!(descriptor.factory(), NativeNodeFactory::List(candidate) if candidate == *operation)
        }),
        NodeContent::Path(operation) => native_node_catalog().iter().find(|descriptor| {
            matches!(descriptor.factory(), NativeNodeFactory::Path(candidate) if candidate == *operation)
        }),
        NodeContent::Merge => native_node_descriptor("native.merge"),
        NodeContent::SoundMerge => native_node_descriptor("native.sound.merge"),
        NodeContent::SoundAnalysis(analysis) => native_node_catalog().iter().find(|descriptor| {
            matches!(
                descriptor.factory(),
                NativeNodeFactory::SoundAnalysis(candidate) if candidate == *analysis
            )
        }),
        NodeContent::NativeOperation(operation) => native_node_descriptor(&operation.catalog_id),
        NodeContent::Media(_)
        | NodeContent::CompositionInstance(_)
        | NodeContent::PluginOperation(_) => None,
    }
}
