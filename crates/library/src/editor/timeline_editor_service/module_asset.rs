//! Asset-backed Module nodes and private generator Node Clip creation.

use super::module::{
    add_node_to_definition, module_definition_mut, private_definition_for_instance,
};
use super::*;

impl TimelineEditorService {
    /// Creates one private image-generator Module and places one Timeline
    /// item that invokes it. This is the bounded source island used by direct
    /// generator entries such as the built-in SkSL Shader Clip.
    pub fn create_generator_node_clip(
        &self,
        plugins: &PluginManager,
        request: crate::editor::ModuleNodeRequest,
        placement: GeneratorNodeClipPlacement,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<
        (
            TimelineItemId,
            ModuleInstanceId,
            ModuleDefinitionId,
            ChangeSet,
        ),
        LibraryError,
    > {
        let mut source = crate::editor::AuthoringNodeFactory::create(
            plugins,
            request,
            canvas_width,
            canvas_height,
        )?;
        if !matches!(source.content(), crate::model::NodeContent::Generator(_)) {
            return Err(LibraryError::Validation(
                "A generator Node Clip source must be a Generator Node".to_string(),
            ));
        }
        source.ui_position = [40.0, 120.0];
        let source_id = source.id;
        let (mut definition, output_id) = ModuleDefinition::new_image(
            placement.name.clone(),
            crate::model::authoring::ModuleDefinitionSharing::Private,
        );
        let output_node_id = definition
            .output(output_id)
            .ok_or_else(|| {
                LibraryError::Validation("Generator Module has no Output terminal".to_string())
            })?
            .node_id;
        definition.graph.nodes.insert(source_id, source);
        definition
            .graph
            .connections
            .push(crate::model::authoring::ModuleConnection {
                id: ModuleConnectionId::new(),
                from: crate::model::authoring::ModulePortAddress {
                    node_id: source_id,
                    port: crate::model::project::IMAGE_OUTPUT_PORT.to_string(),
                },
                to: crate::model::authoring::ModulePortAddress {
                    node_id: output_node_id,
                    port: crate::model::project::IMAGE_INPUT_PORT.to_string(),
                },
                order: 0,
                blend_mode: BlendMode::Normal,
            });
        definition.validate().map_err(LibraryError::Validation)?;
        let definition_id = definition.id;
        let (item_id, instance_id, changes) = self.create_private_module_item(
            definition,
            ModuleItemPlacement {
                track_id: placement.track_id,
                name: placement.name,
                output_id,
                interval: placement.interval,
                layer: placement.layer,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )?;
        Ok((item_id, instance_id, definition_id, changes))
    }

    /// Resolves one imported Asset and inserts its authoritative Media Node
    /// into an instance-owned Module definition in a single Project edit.
    /// UI code supplies presentation coordinates only; it never reconstructs
    /// source paths, stream identities, converter defaults, or media ports.
    pub fn add_asset_to_instance_module(
        &self,
        instance_id: ModuleInstanceId,
        asset_id: uuid::Uuid,
        graph_position: [f32; 2],
        plugins: &PluginManager,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<(uuid::Uuid, ModuleDefinitionId, ChangeSet), LibraryError> {
        if !graph_position.into_iter().all(f32::is_finite) {
            return Err(LibraryError::Validation(
                "Media Node graph position must be finite".to_string(),
            ));
        }
        let mut session = self.write_session()?;
        let asset = session
            .project()
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .cloned()
            .ok_or_else(|| LibraryError::Validation(format!("Missing Asset {asset_id}")))?;
        let mut node = crate::editor::AuthoringNodeFactory::create_asset_media(
            plugins,
            &asset,
            canvas_width,
            canvas_height,
        )?;
        node.ui_position = graph_position;
        session
            .transact(
                vec![ProjectInvalidation::ModuleInstance { instance_id }],
                |project| {
                    let definition_id = private_definition_for_instance(project, instance_id)?;
                    let node_id = add_node_to_definition(
                        module_definition_mut(project, definition_id)?,
                        node,
                    )?;
                    Ok((node_id, definition_id))
                },
            )
            .map(|((node_id, definition_id), changes)| (node_id, definition_id, changes))
            .map_err(LibraryError::Validation)
    }
}
