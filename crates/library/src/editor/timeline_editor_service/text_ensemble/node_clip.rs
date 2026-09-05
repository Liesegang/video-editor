use std::collections::HashSet;

use super::super::module_structure::{
    module_item_ids, operation_parameter_ids, reorder_published_operation_groups,
    require_module_item_ids,
};
use super::*;
use crate::model::authoring::{ModuleConnection, ModulePortAddress, PublishedParameter};
use crate::model::node::{GeneratorContent, NodeContent};
use crate::model::project::{PortDirection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT};
use crate::plugin::{DECORATOR_CATEGORY, EFFECTOR_CATEGORY, PROPERTY_PORT_PREFIX};

/// One descriptor-backed Text Ensemble Node on the selected Node Clip's
/// contiguous Text-to-Fill Shape path. Published parameter IDs let Inspector
/// reuse the ordinary Timeline-owned Module automation controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeClipTextEnsembleEntry {
    pub node_id: uuid::Uuid,
    pub kind: TextEnsembleOperationKind,
    pub category: String,
    pub component_id: String,
    pub operation: String,
    pub parameter_ids: Vec<PublishedParameterId>,
}

/// Derived structured facade over a real Module graph. It is never persisted
/// and disappears when arbitrary Node edits make the path ambiguous.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeClipTextEnsembleStack {
    pub item_id: TimelineItemId,
    pub instance_id: ModuleInstanceId,
    pub definition_id: ModuleDefinitionId,
    pub text_node_id: uuid::Uuid,
    pub appearance_anchor_node_id: uuid::Uuid,
    pub operations: Vec<NodeClipTextEnsembleEntry>,
}

struct RecognizedStack {
    text_node_id: uuid::Uuid,
    appearance_anchor_node_id: uuid::Uuid,
    appearance_shape_links: Vec<ModuleConnectionId>,
    operations: Vec<NodeClipTextEnsembleEntry>,
    /// Shape connections in path order: Text->first and between operations.
    links: Vec<ModuleConnectionId>,
}

impl TimelineEditorService {
    pub fn node_clip_text_ensemble_stack(
        &self,
        item_id: TimelineItemId,
    ) -> Result<Option<NodeClipTextEnsembleStack>, LibraryError> {
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
            recognize(definition, output_id)?.map(|recognized| NodeClipTextEnsembleStack {
                item_id,
                instance_id,
                definition_id: definition.id,
                text_node_id: recognized.text_node_id,
                appearance_anchor_node_id: recognized.appearance_anchor_node_id,
                operations: recognized.operations,
            }),
        )
    }

    pub fn add_node_clip_text_ensemble_operation_by_id(
        &self,
        plugins: &PluginManager,
        item_id: TimelineItemId,
        kind: TextEnsembleOperationKind,
        component_id: &str,
    ) -> Result<(uuid::Uuid, ChangeSet), LibraryError> {
        let mut node = TextEnsembleOperationFactory::create_node(plugins, kind, component_id)?;
        let descriptor =
            plugins.text_ensemble_operation_descriptor(category_for_kind(kind), component_id)?;
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
                            "Text Ensemble Node {} has no default for '{}'",
                            node.id,
                            definition.name()
                        ))
                    })?;
                Ok((
                    definition.name().to_string(),
                    definition.label().to_string(),
                    value,
                ))
            })
            .collect::<Result<Vec<_>, LibraryError>>()?;
        let operation_id = node.id;
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        let instance_id = module_item_ids(session.project(), item_id)?
            .ok_or_else(|| {
                LibraryError::Validation(format!("Timeline item {item_id} is not a Node Clip"))
            })?
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
                    let (instance_id, output_id) = require_module_item_ids(project, item_id)?;
                    let definition_id = super::super::module::private_definition_for_instance(
                        project,
                        instance_id,
                    )?;
                    let definition = project
                        .module_definitions
                        .get_mut(&definition_id)
                        .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
                    let stack = require_recognized(definition, output_id, item_id)?;
                    let insert_at = match kind {
                        TextEnsembleOperationKind::Effector => stack
                            .operations
                            .iter()
                            .position(|entry| entry.kind == TextEnsembleOperationKind::Decorator)
                            .unwrap_or(stack.operations.len()),
                        TextEnsembleOperationKind::Decorator => stack.operations.len(),
                    };
                    let predecessor = if insert_at == 0 {
                        ModulePortAddress {
                            node_id: stack.text_node_id,
                            port: SHAPE_OUTPUT_PORT.to_string(),
                        }
                    } else {
                        ModulePortAddress {
                            node_id: stack.operations[insert_at - 1].node_id,
                            port: SHAPE_OUTPUT_PORT.to_string(),
                        }
                    };
                    let successor_id = stack
                        .operations
                        .get(insert_at)
                        .map_or(stack.appearance_anchor_node_id, |entry| entry.node_id);
                    let left = definition.graph.nodes[&predecessor.node_id].ui_position;
                    let right = definition.graph.nodes[&successor_id].ui_position;
                    node.ui_position = [(left[0] + right[0]) * 0.5, (left[1] + right[1]) * 0.5];
                    if insert_at < stack.operations.len() {
                        let link_id = stack.links[insert_at];
                        let link = definition
                            .graph
                            .connections
                            .iter_mut()
                            .find(|connection| connection.id == link_id)
                            .ok_or_else(|| format!("Missing Module connection {link_id}"))?;
                        let successor = link.to.clone();
                        link.to = ModulePortAddress {
                            node_id: operation_id,
                            port: SHAPE_INPUT_PORT.to_string(),
                        };
                        definition.graph.connections.push(ModuleConnection {
                            id: ModuleConnectionId::new(),
                            from: ModulePortAddress {
                                node_id: operation_id,
                                port: SHAPE_OUTPUT_PORT.to_string(),
                            },
                            to: successor,
                            order: 0,
                            blend_mode: BlendMode::Normal,
                        });
                    } else {
                        for link_id in &stack.appearance_shape_links {
                            let link = definition
                                .graph
                                .connections
                                .iter_mut()
                                .find(|connection| connection.id == *link_id)
                                .ok_or_else(|| {
                                    format!("Missing Appearance Shape link {link_id}")
                                })?;
                            link.from = ModulePortAddress {
                                node_id: operation_id,
                                port: SHAPE_OUTPUT_PORT.to_string(),
                            };
                        }
                        definition.graph.connections.push(ModuleConnection {
                            id: ModuleConnectionId::new(),
                            from: predecessor,
                            to: ModulePortAddress {
                                node_id: operation_id,
                                port: SHAPE_INPUT_PORT.to_string(),
                            },
                            order: 0,
                            blend_mode: BlendMode::Normal,
                        });
                    }
                    if definition.graph.nodes.insert(operation_id, node).is_some() {
                        return Err(format!("Module Node {operation_id} already exists"));
                    }
                    let fill_parameter_index = definition
                        .interface
                        .parameters
                        .iter()
                        .position(|parameter| {
                            parameter.target.node_id == stack.appearance_anchor_node_id
                        })
                        .unwrap_or(definition.interface.parameters.len());
                    let mut published = Vec::with_capacity(parameter_specs.len());
                    for (key, label, default_value) in parameter_specs {
                        let target = ModulePortAddress {
                            node_id: operation_id,
                            port: format!("{PROPERTY_PORT_PREFIX}{key}"),
                        };
                        let port = definition
                            .graph
                            .port_definition(&target, PortDirection::Input)?;
                        published.push(PublishedParameter {
                            id: PublishedParameterId::new(),
                            name: label,
                            data_type: port.data_type,
                            default_value,
                            target,
                        });
                    }
                    definition
                        .interface
                        .parameters
                        .splice(fill_parameter_index..fill_parameter_index, published);
                    super::super::module::bump_topology_revision(definition)?;
                    super::super::module::bump_interface_version(definition)?;
                    definition.validate()?;
                    Ok(())
                },
            )
            .map_err(LibraryError::Validation)?;
        Ok((operation_id, changes))
    }

    pub fn remove_node_clip_text_ensemble_operation(
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
                    let removed_parameter_ids = {
                        let definition = project
                            .module_definitions
                            .get_mut(&definition_id)
                            .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
                        let stack = require_recognized(definition, output_id, item_id)?;
                        let index = stack
                            .operations
                            .iter()
                            .position(|entry| entry.node_id == operation_id)
                            .ok_or_else(|| {
                                format!("Missing Text Ensemble operation {operation_id}")
                            })?;
                        let incoming_id = stack.links[index];
                        if index + 1 < stack.operations.len() {
                            let outgoing_id = stack.links[index + 1];
                            let outgoing_target = definition
                                .graph
                                .connections
                                .iter()
                                .find(|connection| connection.id == outgoing_id)
                                .map(|connection| connection.to.clone())
                                .ok_or_else(|| {
                                    format!("Missing Module connection {outgoing_id}")
                                })?;
                            let incoming = definition
                                .graph
                                .connections
                                .iter_mut()
                                .find(|connection| connection.id == incoming_id)
                                .ok_or_else(|| {
                                    format!("Missing Module connection {incoming_id}")
                                })?;
                            incoming.to = outgoing_target;
                            definition
                                .graph
                                .connections
                                .retain(|connection| connection.id != outgoing_id);
                        } else {
                            let predecessor = definition
                                .graph
                                .connections
                                .iter()
                                .find(|connection| connection.id == incoming_id)
                                .map(|connection| connection.from.clone())
                                .ok_or_else(|| {
                                    format!("Missing Module connection {incoming_id}")
                                })?;
                            for link_id in &stack.appearance_shape_links {
                                let link = definition
                                    .graph
                                    .connections
                                    .iter_mut()
                                    .find(|connection| connection.id == *link_id)
                                    .ok_or_else(|| {
                                        format!("Missing Appearance Shape link {link_id}")
                                    })?;
                                link.from = predecessor.clone();
                            }
                            definition
                                .graph
                                .connections
                                .retain(|connection| connection.id != incoming_id);
                        }
                        definition
                            .graph
                            .nodes
                            .remove(&operation_id)
                            .ok_or_else(|| format!("Missing Text Ensemble Node {operation_id}"))?;
                        let removed = stack.operations[index].parameter_ids.clone();
                        let removed_set = removed.iter().copied().collect::<HashSet<_>>();
                        definition
                            .interface
                            .parameters
                            .retain(|parameter| !removed_set.contains(&parameter.id));
                        super::super::module::bump_topology_revision(definition)?;
                        super::super::module::bump_interface_version(definition)?;
                        definition.validate()?;
                        removed
                    };
                    let instance = project
                        .module_instances
                        .get_mut(&instance_id)
                        .ok_or_else(|| format!("Missing Module instance {instance_id}"))?;
                    for parameter_id in &removed_parameter_ids {
                        instance.parameter_overrides.remove(parameter_id);
                    }
                    let SourceRef::Module(invocation) = &mut project
                        .items
                        .get_mut(&item_id)
                        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
                        .source
                    else {
                        return Err(format!("Timeline item {item_id} is not a Node Clip"));
                    };
                    for parameter_id in removed_parameter_ids {
                        invocation.automation_tracks.remove(&parameter_id);
                    }
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn reorder_node_clip_text_ensemble_operation(
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
                    let definition_id =
                        super::super::module::private_definition_for_instance(project, instance_id)?;
                    let definition = project
                        .module_definitions
                        .get_mut(&definition_id)
                        .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
                    let stack = require_recognized(definition, output_id, item_id)?;
                    if new_index >= stack.operations.len() {
                        return Err(format!(
                            "Text Ensemble index {new_index} is outside item {item_id}"
                        ));
                    }
                    let old_index = stack
                        .operations
                        .iter()
                        .position(|entry| entry.node_id == operation_id)
                        .ok_or_else(|| format!("Missing Text Ensemble operation {operation_id}"))?;
                    if stack.operations[old_index].kind != stack.operations[new_index].kind {
                        return Err(
                            "Text Ensemble operations can only be reordered within their execution phase"
                                .to_string(),
                        );
                    }
                    if old_index == new_index {
                        return Ok(());
                    }
                    let mut order = stack
                        .operations
                        .iter()
                        .map(|entry| entry.node_id)
                        .collect::<Vec<_>>();
                    let presentation_slots = stack
                        .operations
                        .iter()
                        .map(|entry| definition.graph.nodes[&entry.node_id].ui_position)
                        .collect::<Vec<_>>();
                    let moved = order.remove(old_index);
                    order.insert(new_index, moved);
                    for (node_id, position) in order.iter().zip(presentation_slots) {
                        definition
                            .graph
                            .nodes
                            .get_mut(node_id)
                            .ok_or_else(|| format!("Missing Text Ensemble Node {node_id}"))?
                            .ui_position = position;
                    }
                    let sources = std::iter::once(stack.text_node_id)
                        .chain(order.iter().copied())
                        .collect::<Vec<_>>();
                    for ((link_id, source), target) in stack
                        .links
                        .iter()
                        .zip(sources.iter().copied())
                        .zip(order.iter().copied())
                    {
                        let connection = definition
                            .graph
                            .connections
                            .iter_mut()
                            .find(|connection| connection.id == *link_id)
                            .ok_or_else(|| format!("Missing Module connection {link_id}"))?;
                        connection.from = ModulePortAddress {
                            node_id: source,
                            port: SHAPE_OUTPUT_PORT.to_string(),
                        };
                        connection.to = ModulePortAddress {
                            node_id: target,
                            port: SHAPE_INPUT_PORT.to_string(),
                        };
                    }
                    let appearance_source = sources
                        .get(order.len())
                        .copied()
                        .ok_or_else(|| "Missing Text Ensemble Appearance source".to_string())?;
                    for link_id in &stack.appearance_shape_links {
                        let connection = definition
                            .graph
                            .connections
                            .iter_mut()
                            .find(|connection| connection.id == *link_id)
                            .ok_or_else(|| format!("Missing Appearance Shape link {link_id}"))?;
                        connection.from = ModulePortAddress {
                            node_id: appearance_source,
                            port: SHAPE_OUTPUT_PORT.to_string(),
                        };
                    }
                    reorder_published_operation_groups(definition, &order);
                    super::super::module::bump_topology_revision(definition)?;
                    super::super::module::bump_interface_version(definition)?;
                    definition.validate()
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}

fn recognize(
    definition: &ModuleDefinition,
    output_id: ModuleOutputId,
) -> Result<Option<RecognizedStack>, LibraryError> {
    let Some(appearance) = super::super::appearance::node_clip::recognize(definition, output_id)?
    else {
        return Ok(None);
    };
    let appearance_anchor_node_id = appearance.entries[0].node_id;
    let Some(appearance_link) = appearance.shape_links.first().and_then(|link_id| {
        definition
            .graph
            .connections
            .iter()
            .find(|connection| connection.id == *link_id)
    }) else {
        return Ok(None);
    };
    let mut target = appearance_link.to.clone();
    let mut reverse_operations = Vec::new();
    let mut reverse_links = Vec::new();
    let mut visited = HashSet::new();
    let text_node_id = loop {
        let Some(connection) = unique_incoming(definition, &target) else {
            return Ok(None);
        };
        reverse_links.push(connection.id);
        let source_node = definition
            .graph
            .nodes
            .get(&connection.from.node_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Module Node {}", connection.from.node_id))
            })?;
        if !visited.insert(source_node.id) || connection.from.port != SHAPE_OUTPUT_PORT {
            return Ok(None);
        }
        match source_node.content() {
            NodeContent::Generator(GeneratorContent::Text) => break source_node.id,
            NodeContent::PluginOperation(content)
                if source_node.enabled
                    && !source_node.bypassed
                    && matches!(
                        content.category.as_str(),
                        EFFECTOR_CATEGORY | DECORATOR_CATEGORY
                    )
                    && crate::model::authoring::text_ensemble_direct_contract_is_compatible(
                        &content.declared_ports,
                    ) =>
            {
                let kind = if content.category == EFFECTOR_CATEGORY {
                    TextEnsembleOperationKind::Effector
                } else {
                    TextEnsembleOperationKind::Decorator
                };
                let Some(parameter_ids) =
                    operation_parameter_ids(definition, source_node.id, content)
                else {
                    return Ok(None);
                };
                reverse_operations.push(NodeClipTextEnsembleEntry {
                    node_id: source_node.id,
                    kind,
                    category: content.category.clone(),
                    component_id: content.component_id.clone(),
                    operation: content.operation.clone(),
                    parameter_ids,
                });
                target = ModulePortAddress {
                    node_id: source_node.id,
                    port: SHAPE_INPUT_PORT.to_string(),
                };
            }
            _ => return Ok(None),
        }
    };
    reverse_operations.reverse();
    reverse_links.reverse();
    reverse_links.pop();
    if reverse_operations.windows(2).any(|pair| {
        pair[0].kind == TextEnsembleOperationKind::Decorator
            && pair[1].kind == TextEnsembleOperationKind::Effector
    }) {
        return Ok(None);
    }
    Ok(Some(RecognizedStack {
        text_node_id,
        appearance_anchor_node_id,
        appearance_shape_links: appearance.shape_links,
        operations: reverse_operations,
        links: reverse_links,
    }))
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
) -> Result<RecognizedStack, String> {
    recognize(definition, output_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "Node Clip {item_id} is no longer a structured Text Ensemble; edit its custom topology in the Node Editor"
            )
        })
}

const fn category_for_kind(kind: TextEnsembleOperationKind) -> &'static str {
    match kind {
        TextEnsembleOperationKind::Effector => EFFECTOR_CATEGORY,
        TextEnsembleOperationKind::Decorator => DECORATOR_CATEGORY,
    }
}

#[cfg(test)]
#[path = "node_clip/tests.rs"]
mod tests;
