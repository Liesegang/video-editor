//! Structured Appearance facade over the production Module graph.

use std::collections::HashSet;

use super::super::module_structure::{
    module_item_ids, operation_parameter_ids, reorder_published_operation_groups,
    require_module_item_ids,
};
use super::*;
use crate::editor::AppearanceOperationFactory;
use crate::model::authoring::{
    AppearanceInputKind, ModuleConnection, ModuleNodePortContract, ModulePortAddress,
    PublishedParameter, appearance_input_kind,
};
use crate::model::node::NodeContent;
use crate::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortDataType, PortDirection,
    PortMultiplicity, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use crate::plugin::{PROPERTY_PORT_PREFIX, STYLE_APPLY_OPERATION, STYLE_CATEGORY};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeClipAppearanceEntry {
    pub node_id: uuid::Uuid,
    pub component_id: String,
    pub parameter_ids: Vec<PublishedParameterId>,
}

/// Derived facade over one unambiguous Shape/Image appearance chain. It is never
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
    shape_source: ModulePortAddress,
    merge_node_ids: Vec<uuid::Uuid>,
    topology_link_ids: Vec<ModuleConnectionId>,
    downstream_link: ModuleConnection,
}

/// Materializes the one canonical graph representation used by conversion and
/// structured Appearance edits. Raster operations always read the original
/// Shape. Image operations consume the preceding accumulated Image, while an
/// Image operation at the beginning deliberately has no input and therefore
/// evaluates as transparent.
pub(in crate::editor::timeline_editor_service) fn build_appearance_chain(
    definition: &mut ModuleDefinition,
    shape_source: &ModulePortAddress,
    operation_ids: &[uuid::Uuid],
    first_column: f32,
) -> Result<(ModulePortAddress, f32), String> {
    let mut accumulated = None;
    let mut column = first_column;
    for node_id in operation_ids.iter().copied() {
        let kind = appearance_node_kind(definition, node_id)?;
        let node = definition
            .graph
            .nodes
            .get_mut(&node_id)
            .ok_or_else(|| format!("Missing Appearance Node {node_id}"))?;
        node.ui_size[0] = node.ui_size[0].max(crate::model::node::PROPERTY_NODE_UI_WIDTH);
        node.ui_position = [column, 40.0];
        column += node.ui_size[0] + crate::model::node::NODE_LAYOUT_COLUMN_GAP;
        let output = ModulePortAddress {
            node_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        };
        match kind {
            AppearanceInputKind::Shape => {
                definition.graph.connections.push(ModuleConnection {
                    id: ModuleConnectionId::new(),
                    from: shape_source.clone(),
                    to: ModulePortAddress {
                        node_id,
                        port: SHAPE_INPUT_PORT.to_string(),
                    },
                    order: 0,
                    blend_mode: BlendMode::Normal,
                });
                accumulated = Some(if let Some(previous) = accumulated {
                    let (output, next_column) =
                        append_appearance_merge(definition, previous, output, column)?;
                    column = next_column;
                    output
                } else {
                    output
                });
            }
            AppearanceInputKind::Image => {
                if let Some(previous) = accumulated {
                    definition.graph.connections.push(ModuleConnection {
                        id: ModuleConnectionId::new(),
                        from: previous,
                        to: ModulePortAddress {
                            node_id,
                            port: IMAGE_INPUT_PORT.to_string(),
                        },
                        order: 0,
                        blend_mode: BlendMode::Normal,
                    });
                }
                accumulated = Some(output);
            }
        }
    }
    accumulated
        .map(|output| (output, column))
        .ok_or_else(|| "An Appearance needs at least one operation".to_string())
}

fn append_appearance_merge(
    definition: &mut ModuleDefinition,
    previous: ModulePortAddress,
    raster: ModulePortAddress,
    column: f32,
) -> Result<(ModulePortAddress, f32), String> {
    let mut merge = Node::new_merge("Appearance Merge");
    merge.ui_position = [column, 220.0];
    let merge_id = merge.id;
    let merge_width = merge.ui_size[0];
    if definition.graph.nodes.insert(merge_id, merge).is_some() {
        return Err(format!("Appearance Merge {merge_id} already exists"));
    }
    for (order, from) in [previous, raster].into_iter().enumerate() {
        definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from,
            to: ModulePortAddress {
                node_id: merge_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            order: order as i64,
            blend_mode: BlendMode::Normal,
        });
    }
    Ok((
        ModulePortAddress {
            node_id: merge_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        column + merge_width + crate::model::node::NODE_LAYOUT_COLUMN_GAP,
    ))
}

fn appearance_node_kind(
    definition: &ModuleDefinition,
    node_id: uuid::Uuid,
) -> Result<AppearanceInputKind, String> {
    let node = definition
        .graph
        .nodes
        .get(&node_id)
        .ok_or_else(|| format!("Missing Appearance Node {node_id}"))?;
    let NodeContent::PluginOperation(content) = node.content() else {
        return Err(format!(
            "Module Node {node_id} is not an Appearance operation"
        ));
    };
    if content.category != STYLE_CATEGORY || content.operation != STYLE_APPLY_OPERATION {
        return Err(format!(
            "Module Node {node_id} is not an Appearance operation"
        ));
    }
    appearance_input_kind(&content.declared_ports)
        .ok_or_else(|| format!("Appearance Node {node_id} has an incompatible Image contract"))
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
                    if definition.graph.nodes.insert(operation_id, node).is_some() {
                        return Err(format!("Module Node {operation_id} already exists"));
                    }

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
                    rebuild_appearance_chain(definition, &stack, &order)?;
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
                    rebuild_appearance_chain(definition, &stack, &order)?;
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
                        let mut order = stack
                            .entries
                            .iter()
                            .map(|entry| entry.node_id)
                            .collect::<Vec<_>>();
                        order.remove(index);
                        if !order.iter().any(|node_id| {
                            appearance_node_kind(definition, *node_id)
                                == Ok(AppearanceInputKind::Shape)
                        }) {
                            return Err(
                                "A structured Appearance needs at least one Fill or Stroke; edit an all-Image graph in the Node Editor"
                                    .to_string(),
                            );
                        }
                        let removed = super::super::module::removal::remove_nodes_from_definition(
                            definition,
                            &[operation_id],
                        )?;
                        rebuild_appearance_chain(definition, &stack, &order)?;
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

fn rebuild_appearance_chain(
    definition: &mut ModuleDefinition,
    stack: &RecognizedAppearance,
    order: &[uuid::Uuid],
) -> Result<(), String> {
    let topology_links = stack
        .topology_link_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    definition.graph.connections.retain(|connection| {
        connection.id == stack.downstream_link.id || !topology_links.contains(&connection.id)
    });
    for merge_id in &stack.merge_node_ids {
        definition.graph.nodes.remove(merge_id);
    }
    let first_column = order
        .iter()
        .filter_map(|node_id| definition.graph.nodes.get(node_id))
        .map(|node| node.ui_position[0])
        .reduce(f32::min)
        .unwrap_or(40.0);
    let (image, _) = build_appearance_chain(definition, &stack.shape_source, order, first_column)?;
    if let Some(downstream) = definition
        .graph
        .connections
        .iter_mut()
        .find(|connection| connection.id == stack.downstream_link.id)
    {
        downstream.from = image;
    } else {
        let mut downstream = stack.downstream_link.clone();
        downstream.from = image;
        definition.graph.connections.push(downstream);
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
        if is_appearance_node(node) || matches!(node.content(), NodeContent::Merge) {
            let mut chain_visited = HashSet::new();
            let Some(parsed) = parse_appearance_source(
                definition,
                &downstream.from,
                downstream.id,
                &mut chain_visited,
            )?
            else {
                return Ok(None);
            };
            let Some(shape_source) = parsed.shape_source else {
                return Ok(None);
            };
            return Ok(Some(RecognizedAppearance {
                shape_links: parsed.shape_links,
                entries: parsed.entries,
                shape_source,
                merge_node_ids: parsed.merge_node_ids,
                topology_link_ids: parsed.topology_link_ids,
                downstream_link: downstream.clone(),
            }));
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

#[derive(Default)]
struct ParsedAppearance {
    shape_source: Option<ModulePortAddress>,
    shape_links: Vec<ModuleConnectionId>,
    entries: Vec<NodeClipAppearanceEntry>,
    merge_node_ids: Vec<uuid::Uuid>,
    topology_link_ids: Vec<ModuleConnectionId>,
}

fn parse_appearance_source(
    definition: &ModuleDefinition,
    source: &ModulePortAddress,
    consumer_link: ModuleConnectionId,
    visited: &mut HashSet<uuid::Uuid>,
) -> Result<Option<ParsedAppearance>, LibraryError> {
    if source.port != IMAGE_OUTPUT_PORT || !visited.insert(source.node_id) {
        return Ok(None);
    }
    let Some(node) = definition.graph.nodes.get(&source.node_id) else {
        return Ok(None);
    };
    if !has_only_image_consumer(definition, node.id, consumer_link) {
        return Ok(None);
    }

    if let Some((entry, kind)) = style_entry(definition, node) {
        let mut parsed = match kind {
            AppearanceInputKind::Shape => {
                let target = ModulePortAddress {
                    node_id: node.id,
                    port: SHAPE_INPUT_PORT.to_string(),
                };
                let Some(shape_link) = unique_incoming(definition, &target) else {
                    return Ok(None);
                };
                if shape_link.from.port != SHAPE_OUTPUT_PORT {
                    return Ok(None);
                }
                ParsedAppearance {
                    shape_source: Some(shape_link.from.clone()),
                    shape_links: vec![shape_link.id],
                    topology_link_ids: vec![shape_link.id],
                    ..ParsedAppearance::default()
                }
            }
            AppearanceInputKind::Image => {
                let target = ModulePortAddress {
                    node_id: node.id,
                    port: IMAGE_INPUT_PORT.to_string(),
                };
                let inputs = definition
                    .graph
                    .connections
                    .iter()
                    .filter(|connection| connection.to == target)
                    .collect::<Vec<_>>();
                if inputs.len() > 1 {
                    return Ok(None);
                }
                if let Some(input) = inputs.first() {
                    let Some(parsed) =
                        parse_appearance_source(definition, &input.from, input.id, visited)?
                    else {
                        return Ok(None);
                    };
                    parsed
                } else {
                    ParsedAppearance::default()
                }
            }
        };
        parsed.entries.push(entry);
        parsed.topology_link_ids.push(consumer_link);
        return Ok(Some(parsed));
    }

    if !matches!(node.content(), NodeContent::Merge) {
        return Ok(None);
    }
    if definition
        .interface
        .parameters
        .iter()
        .any(|entry| entry.target.node_id == node.id)
        || definition
            .interface
            .media_inputs
            .iter()
            .any(|entry| entry.target.node_id == node.id)
        || definition
            .interface
            .signals
            .iter()
            .any(|entry| entry.source.node_id == node.id)
        || definition
            .interface
            .actions
            .iter()
            .any(|entry| entry.target.node_id == node.id)
    {
        return Ok(None);
    }
    let mut inputs = definition
        .graph
        .connections
        .iter()
        .filter(|connection| {
            connection.to.node_id == node.id && connection.to.port == MERGE_IMAGES_PORT
        })
        .collect::<Vec<_>>();
    inputs.sort_by_key(|connection| (connection.order, connection.id));
    if definition
        .graph
        .connections
        .iter()
        .filter(|connection| connection.to.node_id == node.id)
        .count()
        != 2
        || inputs.len() != 2
        || inputs[0].order != 0
        || inputs[1].order != 1
        || inputs
            .iter()
            .any(|connection| connection.blend_mode != BlendMode::Normal)
    {
        return Ok(None);
    }
    let Some(mut prior) =
        parse_appearance_source(definition, &inputs[0].from, inputs[0].id, visited)?
    else {
        return Ok(None);
    };
    let mut raster_visited = HashSet::new();
    let Some(raster) = parse_appearance_source(
        definition,
        &inputs[1].from,
        inputs[1].id,
        &mut raster_visited,
    )?
    else {
        return Ok(None);
    };
    if raster.entries.len() != 1 || raster.shape_source.is_none() {
        return Ok(None);
    }
    if let (Some(prior_source), Some(raster_source)) =
        (prior.shape_source.as_ref(), raster.shape_source.as_ref())
        && prior_source != raster_source
    {
        return Ok(None);
    }
    if prior.shape_source.is_none() {
        prior.shape_source = raster.shape_source;
    }
    prior.shape_links.extend(raster.shape_links);
    prior.entries.extend(raster.entries);
    prior.merge_node_ids.extend(raster.merge_node_ids);
    prior.merge_node_ids.push(node.id);
    prior.topology_link_ids.extend(raster.topology_link_ids);
    prior.topology_link_ids.push(consumer_link);
    Ok(Some(prior))
}

fn is_appearance_node(node: &Node) -> bool {
    matches!(
        node.content(),
        NodeContent::PluginOperation(content)
            if content.category == STYLE_CATEGORY && content.operation == STYLE_APPLY_OPERATION
    )
}

fn has_only_image_consumer(
    definition: &ModuleDefinition,
    node_id: uuid::Uuid,
    expected: ModuleConnectionId,
) -> bool {
    let mut consumers = definition
        .graph
        .connections
        .iter()
        .filter(|connection| connection.from.node_id == node_id);
    consumers.next().map(|connection| connection.id) == Some(expected) && consumers.next().is_none()
}

fn style_entry(
    definition: &ModuleDefinition,
    node: &Node,
) -> Option<(NodeClipAppearanceEntry, AppearanceInputKind)> {
    let NodeContent::PluginOperation(content) = node.content() else {
        return None;
    };
    if !node.enabled
        || node.bypassed
        || content.category != STYLE_CATEGORY
        || content.operation != STYLE_APPLY_OPERATION
    {
        return None;
    }
    let kind = appearance_input_kind(&content.declared_ports)?;
    Some((
        NodeClipAppearanceEntry {
            node_id: node.id,
            component_id: content.component_id.clone(),
            parameter_ids: operation_parameter_ids(definition, node.id, content)?,
        },
        kind,
    ))
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
