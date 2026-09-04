mod assets;
mod curve;
mod inspector;
mod preview;
mod timeline;

pub use assets::assets_panel;
pub use curve::curve_panel;
pub use inspector::inspector_panel;
pub use preview::{preview_panel, AuthoringPreviewRuntime};
pub use timeline::timeline_panel;

use library::model::authoring::{
    ModuleDefinition, ModuleDefinitionId, ModuleDefinitionSharing, ModuleGraph, ModuleInterface,
    ModulePortAddress, ModuleTemplateOrigin, PublishedMediaOutput, PublishedMediaOutputId,
};
use library::model::project::connection::{PortDataType, IMAGE_OUTPUT_PORT};
use library::model::Node;

/// A useful bounded starter graph. The output Merge remains the stable public
/// boundary while users add generators/effects behind it in the Node Editor.
pub(crate) fn image_module_definition(
    name: impl Into<String>,
    sharing: ModuleDefinitionSharing,
) -> (ModuleDefinition, PublishedMediaOutputId) {
    let mut output = Node::new_merge("Output");
    output.ui_position = [360.0, 120.0];
    let output_node_id = output.id;
    let output_id = PublishedMediaOutputId::new();
    let definition = ModuleDefinition {
        id: ModuleDefinitionId::new(),
        name: name.into(),
        sharing,
        graph: ModuleGraph {
            nodes: std::collections::HashMap::from([(output_node_id, output)]),
            connections: Vec::new(),
        },
        interface: ModuleInterface {
            media_outputs: vec![PublishedMediaOutput {
                id: output_id,
                name: "Image".to_string(),
                data_type: PortDataType::Image,
                source: ModulePortAddress {
                    node_id: output_node_id,
                    port: IMAGE_OUTPUT_PORT.to_string(),
                },
            }],
            ..ModuleInterface::default()
        },
        topology_revision: 1,
        interface_version: 1,
    };
    (definition, output_id)
}

pub(crate) fn project_module_definition(
    name: impl Into<String>,
) -> (ModuleDefinition, PublishedMediaOutputId) {
    image_module_definition(
        name,
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    )
}
