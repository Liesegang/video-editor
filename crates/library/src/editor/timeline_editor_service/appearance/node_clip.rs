//! Structured Appearance facade over the production Module graph.

use std::collections::HashSet;

use super::super::module_structure::{
    module_item_ids, operation_parameter_ids, reorder_published_operation_groups,
    require_module_item_ids,
};
use super::*;
use crate::editor::AppearanceOperationFactory;
use crate::model::authoring::{
    ModuleConnection, ModuleNodePortContract, ModulePortAddress, PublishedParameter,
};
use crate::model::node::NodeContent;
use crate::model::project::{
    APPEARANCE_STYLES_PORT, IMAGE_OUTPUT_PORT, PortDataType, PortDirection, PortMultiplicity,
    SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, STYLE_OUTPUT_PORT,
};
use crate::plugin::{PROPERTY_PORT_PREFIX, STYLE_APPLY_OPERATION, STYLE_CATEGORY};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeClipAppearanceEntry {
    pub node_id: uuid::Uuid,
    pub component_id: String,
    pub parameter_ids: Vec<PublishedParameterId>,
}

/// Derived facade over one unambiguous Shape-to-Appearance Stack. It is never
/// persisted and disappears as soon as arbitrary Node edits make the chain
/// ambiguous.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeClipAppearanceStack {
    pub item_id: TimelineItemId,
    pub instance_id: ModuleInstanceId,
    pub definition_id: ModuleDefinitionId,
    pub operations: Vec<NodeClipAppearanceEntry>,
}

#[derive(Clone)]
pub(in crate::editor::timeline_editor_service) struct RecognizedAppearance {
    pub(in crate::editor::timeline_editor_service) shape_links: Vec<ModuleConnectionId>,
    pub(in crate::editor::timeline_editor_service) entries: Vec<NodeClipAppearanceEntry>,
    stack_node_id: uuid::Uuid,
}

impl TimelineEditorService {
    pub fn node_clip_appearance_stack(
        &self,
        item_id: TimelineItemId,
    ) -> Result<Option<NodeClipAppearanceStack>, LibraryError> {
        let project = self.snapshot()?;
        let Some((instance_id, output_id)) = module_item_ids(&project, item_id)? else {
            return Ok(None);
        };
        let instance = project.module_instances.get(&instance_id).ok_or_else(|| {
            LibraryError::Validation(format!("Missing Module instance {instance_id}"))
        })?;
        let definition = project
            .module_definitions
            .get(&instance.definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Missing Module definition {}",
                    instance.definition_id
                ))
            })?;
        Ok(
            recognize(definition, output_id)?.map(|recognized| NodeClipAppearanceStack {
                item_id,
                instance_id,
                definition_id: definition.id,
                operations: recognized.entries,
            }),
        )
    }

    pub fn add_node_clip_appearance_operation(
        &self,
        plugins: &PluginManager,
        item_id: TimelineItemId,
        component_id: &str,
        index: usize,
    ) -> Result<(uuid::Uuid, ChangeSet), LibraryError> {
        let authored = AppearanceOperationFactory::create(plugins, component_id)?;
        let operation_id = authored.id;
        let mut node = plugins.create_style_operation_node(component_id)?;
        node.id = operation_id;
        let descriptor =
            plugins.operation_descriptor(STYLE_CATEGORY, component_id, STYLE_APPLY_OPERATION)?;
        let parameter_specs = descriptor
            .properties()
            .iter()
            .map(|definition| {
                let value = node
                    .properties()
                    .get(definition.name())
                    .and_then(Property::value)
                    .cloned()
                    .ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Appearance Node {} has no default for '{}'",
                            node.id,
                            definition.name()
                        ))
                    })?;
                Ok((
                    definition.name().to_string(),
                    format!("{} {}", descriptor.label(), definition.label()),
                    value,
                ))
            })
            .collect::<Result<Vec<_>, LibraryError>>()?;

        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        let instance_id = require_module_item_ids(session.project(), item_id)
            .map_err(LibraryError::Validation)?
            .0;
        let (_, changes) = session
            .transact(
                vec![
                    ProjectInvalidation::Item {
                        timeline_id,
                        item_id,
                    },
                    ProjectInvalidation::ModuleInstance { instance_id },
                ],
                move |project| {
                    let (_, output_id) = require_module_item_ids(project, item_id)?;
                    let definition_id = super::super::module::private_definition_for_instance(
                        project,
                        instance_id,
                    )?;
                    let definition = project
                        .module_definitions
                        .get_mut(&definition_id)
                        .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
                    let stack = require_recognized(definition, output_id, item_id)?;
                    if index > stack.entries.len() {
                        return Err(format!(
                            "Appearance index {index} is outside Node Clip {item_id}"
                        ));
                    }
                    let first = definition.graph.nodes[&stack.entries[0].node_id].ui_position;
                    node.ui_position = [first[0], first[1] + index as f32 * 80.0 + 80.0];
                    if definition.graph.nodes.insert(operation_id, node).is_some() {
                        return Err(format!("Module Node {operation_id} already exists"));
                    }
                    insert_style_output(definition, &stack, operation_id, index)?;

                    let appearance_nodes = stack
                        .entries
                        .iter()
                        .map(|entry| entry.node_id)
                        .collect::<HashSet<_>>();
                    let parameter_index = definition
                        .interface
                        .parameters
                        .iter()
                        .position(|parameter| appearance_nodes.contains(&parameter.target.node_id))
                        .unwrap_or(definition.interface.parameters.len());
                    let mut published = Vec::with_capacity(parameter_specs.len());
                    for (key, name, default_value) in parameter_specs {
                        let target = ModulePortAddress {
                            node_id: operation_id,
                            port: format!("{PROPERTY_PORT_PREFIX}{key}"),
                        };
                        let port = definition
                            .graph
                            .port_definition(&target, PortDirection::Input)?;
                        published.push(PublishedParameter {
                            id: PublishedParameterId::new(),
                            name,
                            data_type: port.data_type,
                            default_value,
                            target,
                        });
                    }
                    definition
                        .interface
                        .parameters
                        .splice(parameter_index..parameter_index, published);
                    let mut order = stack
                        .entries
                        .iter()
                        .map(|entry| entry.node_id)
                        .collect::<Vec<_>>();
                    order.insert(index, operation_id);
                    reorder_published_operation_groups(definition, &order);
                    super::super::module::bump_topology_revision(definition)?;
                    super::super::module::bump_interface_version(definition)?;
                    definition.validate()
                },
            )
            .map_err(LibraryError::Validation)?;
        Ok((operation_id, changes))
    }

    pub fn reorder_node_clip_appearance_operation(
        &self,
        item_id: TimelineItemId,
        operation_id: uuid::Uuid,
        new_index: usize,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let instance_id = require_module_item_ids(session.project(), item_id)
            .map_err(LibraryError::Validation)?
            .0;
        session
            .transact(
                vec![ProjectInvalidation::ModuleInstance { instance_id }],
                |project| {
                    let (_, output_id) = require_module_item_ids(project, item_id)?;
                    let definition_id = super::super::module::private_definition_for_instance(
                        project,
                        instance_id,
                    )?;
                    let definition = project
                        .module_definitions
                        .get_mut(&definition_id)
                        .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
                    let stack = require_recognized(definition, output_id, item_id)?;
                    if new_index >= stack.entries.len() {
                        return Err(format!(
                            "Appearance index {new_index} is outside Node Clip {item_id}"
                        ));
                    }
                    let old_index = stack
                        .entries
                        .iter()
                        .position(|entry| entry.node_id == operation_id)
                        .ok_or_else(|| format!("Missing Appearance Node {operation_id}"))?;
                    if old_index == new_index {
                        return Ok(());
                    }
                    let mut order = stack
                        .entries
                        .iter()
                        .map(|entry| entry.node_id)
                        .collect::<Vec<_>>();
                    let moved = order.remove(old_index);
                    order.insert(new_index, moved);
                    reorder_style_outputs(definition, stack.stack_node_id, &order)?;
                    reorder_published_operation_groups(definition, &order);
                    super::super::module::bump_topology_revision(definition)?;
                    super::super::module::bump_interface_version(definition)?;
                    definition.validate()
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn remove_node_clip_appearance_operation(
        &self,
        item_id: TimelineItemId,
        operation_id: uuid::Uuid,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        let instance_id = require_module_item_ids(session.project(), item_id)
            .map_err(LibraryError::Validation)?
            .0;
        session
            .transact(
                vec![
                    ProjectInvalidation::Item {
                        timeline_id,
                        item_id,
                    },
                    ProjectInvalidation::ModuleInstance { instance_id },
                ],
                |project| {
                    let (_, output_id) = require_module_item_ids(project, item_id)?;
                    let definition_id = super::super::module::private_definition_for_instance(
                        project,
                        instance_id,
                    )?;
                    let removed = {
                        let definition = project
                            .module_definitions
                            .get_mut(&definition_id)
                            .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
                        let stack = require_recognized(definition, output_id, item_id)?;
                        if stack.entries.len() == 1 {
                            return Err(
                                "A Node Clip needs at least one Appearance to produce Image output"
                                    .to_string(),
                            );
                        }
                        let index = stack
                            .entries
                            .iter()
                            .position(|entry| entry.node_id == operation_id)
                            .ok_or_else(|| format!("Missing Appearance Node {operation_id}"))?;
                        collapse_removed_style(definition, &stack, index)?;
                        let removed = super::super::module::removal::remove_nodes_from_definition(
                            definition,
                            &[operation_id],
                        )?;
                        definition.validate()?;
                        removed
                    };
                    super::super::interface::cleanup_removed_interface_dependents(
                        project,
                        &[instance_id],
                        removed.parameter_ids,
                        removed.media_input_ids,
                    )?;
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}

fn insert_style_output(
    definition: &mut ModuleDefinition,
    stack: &RecognizedAppearance,
    node_id: uuid::Uuid,
    index: usize,
) -> Result<(), String> {
    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id,
            port: STYLE_OUTPUT_PORT.to_string(),
        },
        to: ModulePortAddress {
            node_id: stack.stack_node_id,
            port: APPEARANCE_STYLES_PORT.to_string(),
        },
        order: index as i64,
        blend_mode: BlendMode::Normal,
    });
    let mut order = stack
        .entries
        .iter()
        .map(|entry| entry.node_id)
        .collect::<Vec<_>>();
    order.insert(index, node_id);
    reorder_style_outputs(definition, stack.stack_node_id, &order)?;
    Ok(())
}

fn collapse_removed_style(
    definition: &mut ModuleDefinition,
    stack: &RecognizedAppearance,
    removed_index: usize,
) -> Result<(), String> {
    let mut order = stack
        .entries
        .iter()
        .map(|entry| entry.node_id)
        .collect::<Vec<_>>();
    order.remove(removed_index);
    reorder_style_outputs(definition, stack.stack_node_id, &order)
}

fn reorder_style_outputs(
    definition: &mut ModuleDefinition,
    stack_node_id: uuid::Uuid,
    order: &[uuid::Uuid],
) -> Result<(), String> {
    for (index, source_id) in order.iter().enumerate() {
        let connection = definition
            .graph
            .connections
            .iter_mut()
            .find(|connection| {
                connection.from.node_id == *source_id
                    && connection.from.port == STYLE_OUTPUT_PORT
                    && connection.to.node_id == stack_node_id
                    && connection.to.port == APPEARANCE_STYLES_PORT
            })
            .ok_or_else(|| format!("Missing Appearance Stack input for {source_id}"))?;
        connection.order = index as i64;
    }
    Ok(())
}

pub(in crate::editor::timeline_editor_service) fn recognize(
    definition: &ModuleDefinition,
    output_id: ModuleOutputId,
) -> Result<Option<RecognizedAppearance>, LibraryError> {
    let output = definition.output(output_id).ok_or_else(|| {
        LibraryError::Validation(format!(
            "Module definition {} has no Output {output_id}",
            definition.id
        ))
    })?;
    let mut target = output.target(PortDataType::Image).ok_or_else(|| {
        LibraryError::Validation(format!("Module Output {output_id} has no Image input"))
    })?;
    let mut visited = HashSet::new();
    loop {
        let Some(downstream) = unique_incoming(definition, &target) else {
            return Ok(None);
        };
        let Some(node) = definition.graph.nodes.get(&downstream.from.node_id) else {
            return Err(LibraryError::Validation(format!(
                "Missing Module Node {}",
                downstream.from.node_id
            )));
        };
        if !visited.insert(node.id) || downstream.from.port != IMAGE_OUTPUT_PORT {
            return Ok(None);
        }
        if matches!(
            node.content(),
            NodeContent::NativeOperation(operation)
                if operation.catalog_id == crate::model::node::APPEARANCE_STACK_CATALOG_ID
        ) {
            return recognize_stack(definition, node.id, downstream.id);
        }
        let contract = ModuleNodePortContract::resolve(node).map_err(LibraryError::Validation)?;
        let image_inputs = contract
            .ports
            .iter()
            .filter(|port| {
                port.direction == PortDirection::Input
                    && port.data_type == PortDataType::Image
                    && port.multiplicity == PortMultiplicity::Single
            })
            .collect::<Vec<_>>();
        if image_inputs.len() != 1 {
            return Ok(None);
        }
        target = ModulePortAddress {
            node_id: node.id,
            port: image_inputs[0].key.clone(),
        };
    }
}

fn recognize_stack(
    definition: &ModuleDefinition,
    stack_node_id: uuid::Uuid,
    downstream_link: ModuleConnectionId,
) -> Result<Option<RecognizedAppearance>, LibraryError> {
    let mut stack_consumers = definition
        .graph
        .connections
        .iter()
        .filter(|connection| connection.from.node_id == stack_node_id);
    if stack_consumers.next().map(|connection| connection.id) != Some(downstream_link)
        || stack_consumers.next().is_some()
    {
        return Ok(None);
    }
    let shape_target = ModulePortAddress {
        node_id: stack_node_id,
        port: SHAPE_INPUT_PORT.to_string(),
    };
    let Some(shape_connection) = unique_incoming(definition, &shape_target) else {
        return Ok(None);
    };
    if shape_connection.from.port != SHAPE_OUTPUT_PORT {
        return Ok(None);
    }
    let mut inputs = definition
        .graph
        .connections
        .iter()
        .filter(|connection| {
            connection.to.node_id == stack_node_id && connection.to.port == APPEARANCE_STYLES_PORT
        })
        .collect::<Vec<_>>();
    inputs.sort_by_key(|connection| (connection.order, connection.id));
    if inputs.is_empty() {
        return Ok(None);
    }
    let mut entries = Vec::with_capacity(inputs.len());
    for connection in &inputs {
        if connection.from.port != STYLE_OUTPUT_PORT {
            return Ok(None);
        }
        let Some(node) = definition.graph.nodes.get(&connection.from.node_id) else {
            return Ok(None);
        };
        let Some(entry) = style_entry(definition, node) else {
            return Ok(None);
        };
        let mut consumers = definition
            .graph
            .connections
            .iter()
            .filter(|candidate| candidate.from.node_id == node.id);
        if consumers.next().map(|candidate| candidate.id) != Some(connection.id)
            || consumers.next().is_some()
        {
            return Ok(None);
        }
        entries.push(entry);
    }
    Ok(Some(RecognizedAppearance {
        shape_links: vec![shape_connection.id],
        entries,
        stack_node_id,
    }))
}

fn style_entry(definition: &ModuleDefinition, node: &Node) -> Option<NodeClipAppearanceEntry> {
    let NodeContent::PluginOperation(content) = node.content() else {
        return None;
    };
    if !node.enabled
        || node.bypassed
        || content.category != STYLE_CATEGORY
        || content.operation != STYLE_APPLY_OPERATION
        || !crate::model::authoring::appearance_direct_contract_is_compatible(
            &content.declared_ports,
        )
    {
        return None;
    }
    Some(NodeClipAppearanceEntry {
        node_id: node.id,
        component_id: content.component_id.clone(),
        parameter_ids: operation_parameter_ids(definition, node.id, content)?,
    })
}

fn unique_incoming<'a>(
    definition: &'a ModuleDefinition,
    target: &ModulePortAddress,
) -> Option<&'a ModuleConnection> {
    let mut incoming = definition
        .graph
        .connections
        .iter()
        .filter(|connection| connection.to == *target);
    let result = incoming.next()?;
    incoming.next().is_none().then_some(result)
}

fn require_recognized(
    definition: &ModuleDefinition,
    output_id: ModuleOutputId,
    item_id: TimelineItemId,
) -> Result<RecognizedAppearance, String> {
    recognize(definition, output_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "Node Clip {item_id} is no longer a structured Appearance; edit its custom topology in the Node Editor"
            )
        })
}

#[cfg(test)]
#[path = "node_clip/tests.rs"]
mod tests;
