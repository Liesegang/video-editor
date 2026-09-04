//! Timeline/Preview semantic property authoring over the authoritative graph.
//!
//! `Clip`, `Track`, and `Composition` remain the selected semantic identities,
//! but placement and alpha are owned by typed operation Nodes. This module is
//! a derived read/write facade only; it never persists an intermediate model.

use std::collections::{HashMap, HashSet, VecDeque};

use super::lifecycle::ProjectManager;
use crate::animation::EasingFunction;
use crate::error::LibraryError;
use crate::model::NodeContent;
use crate::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeContainer, NodeGraphBundle, PortAddress, PortDataType,
    PortDirection, PortOwner, Project, ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use crate::model::property::{
    KeyframeId, KeyframeUpdate, Property, PropertyDefinition, PropertyMap, PropertyValue,
};
use crate::plugin::{
    IMAGE_OPACITY_STYLE_COMPONENT_ID, IMAGE_TRANSFORM_COMPONENT_ID, SHAPE_TRANSFORM_COMPONENT_ID,
    STYLE_APPLY_OPERATION, STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
};
use uuid::Uuid;

mod decorator_stack;
mod effect_stack;
mod helpers;
mod stack_projection;
mod style_stack;
mod transform;

pub use decorator_stack::{SemanticDecoratorChain, SemanticDecoratorStack};
pub use effect_stack::SemanticEffectStack;
use helpers::*;
pub use stack_projection::{
    SemanticAnimationSupport, SemanticContainerPropertyStack, SemanticPropertyAccess,
    SemanticPropertyEntry, SemanticPropertyGroup, SemanticPropertyOwner, SemanticPropertySection,
};
pub use style_stack::{SemanticStyleBranch, SemanticStyleStack};

const TRANSFORM_PROPERTIES: [&str; 4] = ["position", "rotation", "scale", "anchor"];
const SEMANTIC_PROPERTIES: [&str; 5] = ["position", "rotation", "scale", "anchor", "opacity"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticPropertyBinding {
    pub node_id: Option<Uuid>,
    pub connected_source: Option<PortAddress>,
}

/// Ephemeral read projection for Timeline/Preview/Inspector. The cloned
/// properties are discarded after rendering; Project Nodes remain authority.
#[derive(Clone, Debug)]
pub struct SemanticContainerPropertyProjection {
    owner: NodeContainer,
    definitions: Vec<PropertyDefinition>,
    properties: PropertyMap,
    bindings: HashMap<String, SemanticPropertyBinding>,
}

impl SemanticContainerPropertyProjection {
    pub fn owner(&self) -> NodeContainer {
        self.owner
    }

    pub fn definitions(&self) -> &[PropertyDefinition] {
        &self.definitions
    }

    pub fn properties(&self) -> &PropertyMap {
        &self.properties
    }

    pub fn binding(&self, property: &str) -> Option<&SemanticPropertyBinding> {
        self.bindings.get(property)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransformKind {
    Shape,
    Image,
}

#[derive(Clone, Copy, Debug)]
struct ResolvedGraphOwners {
    transform: Option<(Uuid, TransformKind)>,
    opacity: Option<Uuid>,
}

impl ProjectManager {
    pub fn semantic_container_property_projection(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticContainerPropertyProjection, LibraryError> {
        let project = self
            .project
            .read()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let resolved = resolve_graph_owners(&project, owner)?;
        build_projection(&project, owner, resolved, &self.plugin_manager)
    }

    pub fn update_semantic_container_property_or_keyframe(
        &self,
        owner: NodeContainer,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.mutate_semantic_property(owner, property_key, |property| {
            match property.evaluator.as_str() {
                "keyframe" => {
                    if property
                        .upsert_keyframe_with_id(time, value, easing)
                        .is_none()
                    {
                        return Err("property cannot be keyframed".to_string());
                    }
                }
                "constant" => *property = Property::constant(value),
                _ => {
                    property.properties.insert("value".to_string(), value);
                }
            }
            Ok(())
        })
    }

    pub fn replace_semantic_container_property(
        &self,
        owner: NodeContainer,
        property_key: &str,
        property: Property,
    ) -> Result<(), LibraryError> {
        self.mutate_semantic_property(owner, property_key, |target| {
            *target = property;
            Ok(())
        })
    }

    pub fn set_semantic_container_property_attribute(
        &self,
        owner: NodeContainer,
        property_key: &str,
        attribute_key: String,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.mutate_semantic_property(owner, property_key, |property| {
            property.properties.insert(attribute_key, attribute_value);
            Ok(())
        })
    }

    pub fn add_semantic_container_keyframe(
        &self,
        owner: NodeContainer,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<KeyframeId, LibraryError> {
        let mut inserted = None;
        self.mutate_semantic_property(owner, property_key, |property| {
            inserted = property.upsert_keyframe_with_id(time, value, easing);
            inserted
                .map(|_| ())
                .ok_or_else(|| "property cannot be keyframed".to_string())
        })?;
        inserted.ok_or_else(|| {
            LibraryError::Project(format!(
                "Semantic property {property_key} on {owner:?} did not create a keyframe"
            ))
        })
    }

    pub fn update_semantic_container_keyframe_by_id(
        &self,
        owner: NodeContainer,
        property_key: &str,
        keyframe_id: KeyframeId,
        update: KeyframeUpdate,
    ) -> Result<(), LibraryError> {
        self.mutate_semantic_property(owner, property_key, |property| {
            property
                .update_keyframe_by_id(keyframe_id, update)
                .then_some(())
                .ok_or_else(|| format!("keyframe {keyframe_id} was not found"))
        })
    }

    pub fn remove_semantic_container_keyframe_by_id(
        &self,
        owner: NodeContainer,
        property_key: &str,
        keyframe_id: KeyframeId,
    ) -> Result<(), LibraryError> {
        self.mutate_semantic_property(owner, property_key, |property| {
            property
                .remove_keyframe_by_id(keyframe_id)
                .then_some(())
                .ok_or_else(|| format!("keyframe {keyframe_id} was not found"))
        })
    }

    fn mutate_semantic_property(
        &self,
        owner: NodeContainer,
        property_key: &str,
        mutate: impl FnOnce(&mut Property) -> Result<(), String>,
    ) -> Result<(), LibraryError> {
        if !SEMANTIC_PROPERTIES.contains(&property_key) {
            return Err(LibraryError::Project(format!(
                "Semantic property {property_key:?} is not one of {}",
                SEMANTIC_PROPERTIES.join(", ")
            )));
        }
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let mut candidate = project.clone();
        let resolved = ensure_graph_owners(&mut candidate, owner, &self.plugin_manager)?;
        let node_id = semantic_property_node(resolved, property_key).ok_or_else(|| {
            LibraryError::Project(format!(
                "Semantic property {property_key} on {owner:?} has no graph owner"
            ))
        })?;
        reject_wired_property(&candidate, node_id, property_key, owner)?;
        let mut property = candidate
            .get_node(node_id)
            .and_then(|node| node.properties().get(property_key))
            .cloned()
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Semantic Node {node_id} is missing property {property_key}"
                ))
            })?;
        mutate(&mut property).map_err(|reason| {
            LibraryError::Project(format!(
                "Cannot edit semantic property {property_key} on {owner:?}: {reason}"
            ))
        })?;
        candidate
            .get_node_mut(node_id)
            .ok_or_else(|| LibraryError::Project(format!("Semantic Node {node_id} not found")))?
            .set_property(property_key.to_string(), property)
            .map_err(LibraryError::Project)?;
        validate_candidate(&candidate, owner)?;
        *project = candidate;
        Ok(())
    }
}

fn semantic_property_node(resolved: ResolvedGraphOwners, property: &str) -> Option<Uuid> {
    if property == "opacity" {
        resolved.opacity
    } else {
        resolved.transform.map(|(node_id, _)| node_id)
    }
}

fn build_projection(
    project: &Project,
    owner: NodeContainer,
    resolved: ResolvedGraphOwners,
    plugins: &crate::plugin::PluginManager,
) -> Result<SemanticContainerPropertyProjection, LibraryError> {
    let mut definitions = crate::plugin::transforms::property_definitions();
    definitions.extend(crate::plugin::styles::image_opacity_property_definitions());
    let legacy = container_properties(project, owner)?;
    let mut properties = PropertyMap::from_definitions(&definitions);
    let mut bindings = HashMap::new();

    for property_key in TRANSFORM_PROPERTIES {
        let node_id = resolved.transform.map(|(node_id, _)| node_id);
        let property = projected_property(project, legacy, node_id, property_key, false)?;
        properties.set(property_key.to_string(), property);
        bindings.insert(
            property_key.to_string(),
            SemanticPropertyBinding {
                node_id,
                connected_source: node_id
                    .and_then(|id| connected_property_source(project, id, property_key)),
            },
        );
    }

    let opacity_id = resolved.opacity;
    let opacity = projected_property(project, legacy, opacity_id, "opacity", true)?;
    properties.set("opacity".to_string(), opacity);
    bindings.insert(
        "opacity".to_string(),
        SemanticPropertyBinding {
            node_id: opacity_id,
            connected_source: opacity_id
                .and_then(|id| connected_property_source(project, id, "opacity")),
        },
    );

    // Descriptor lookup is intentional: a later-installed replacement for a
    // Style component remains visible, but an unavailable contract does not
    // silently become a mutable semantic facade.
    let _ = plugins.operation_descriptor(
        STYLE_CATEGORY,
        IMAGE_OPACITY_STYLE_COMPONENT_ID,
        STYLE_APPLY_OPERATION,
    )?;
    Ok(SemanticContainerPropertyProjection {
        owner,
        definitions,
        properties,
        bindings,
    })
}

fn projected_property(
    project: &Project,
    legacy: &PropertyMap,
    node_id: Option<Uuid>,
    key: &str,
    scale_legacy_opacity: bool,
) -> Result<Property, LibraryError> {
    let legacy_property = legacy.get(key);
    let authored_legacy = legacy_property.filter(|value| !is_neutral_legacy(key, value));
    if let Some(node_id) = node_id {
        let node_property = project
            .get_node(node_id)
            .and_then(|node| node.properties().get(key))
            .ok_or_else(|| {
                LibraryError::Project(format!("Semantic Node {node_id} is missing property {key}"))
            })?;
        if let Some(legacy_property) = authored_legacy {
            if connected_property_source(project, node_id, key).is_some()
                || !is_default_graph_property(key, node_property)
            {
                return Err(conflicting_authority(node_id, key));
            }
            return if scale_legacy_opacity {
                scale_number_property(legacy_property, 0.01)
            } else {
                Ok(legacy_property.clone())
            };
        }
        return Ok(node_property.clone());
    }
    match legacy_property {
        Some(property) if scale_legacy_opacity => scale_number_property(property, 0.01),
        Some(property) => Ok(property.clone()),
        None => default_graph_property(key),
    }
}

fn resolve_graph_owners(
    project: &Project,
    owner: NodeContainer,
) -> Result<ResolvedGraphOwners, LibraryError> {
    let final_owners = resolve_final_image_owners(project, owner)?;
    let transform = final_owners
        .transform
        .map(|node_id| (node_id, TransformKind::Image))
        .or_else(|| {
            dominating_image_transform(project, owner)
                .map(|node_id| (node_id, TransformKind::Image))
        })
        .or_else(|| {
            common_shape_transform(project, owner).map(|node_id| (node_id, TransformKind::Shape))
        });
    Ok(ResolvedGraphOwners {
        transform,
        opacity: final_owners.opacity,
    })
}

#[derive(Default)]
struct FinalImageOwners {
    transform: Option<Uuid>,
    opacity: Option<Uuid>,
}

fn resolve_final_image_owners(
    project: &Project,
    owner: NodeContainer,
) -> Result<FinalImageOwners, LibraryError> {
    let Some(mut cursor) = container_output_node_id(project, owner)? else {
        return Ok(FinalImageOwners::default());
    };
    let mut owners = FinalImageOwners::default();
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(cursor) {
            return Err(LibraryError::Project(format!(
                "Final semantic Image trunk for {owner:?} contains a cycle"
            )));
        }
        let Some(node) = project.get_node(cursor) else {
            break;
        };
        let NodeContent::PluginOperation(operation) = node.content() else {
            break;
        };
        let slot = if operation.category == TRANSFORM_CATEGORY
            && operation.component_id == IMAGE_TRANSFORM_COMPONENT_ID
            && operation.operation == TRANSFORM_APPLY_OPERATION
        {
            &mut owners.transform
        } else if operation.category == STYLE_CATEGORY
            && operation.component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID
            && operation.operation == STYLE_APPLY_OPERATION
        {
            &mut owners.opacity
        } else {
            break;
        };
        if let Some(existing) = slot.replace(cursor) {
            return Err(LibraryError::Project(format!(
                "Semantic property owner for {owner:?} is ambiguous on the final Image trunk: Nodes {existing}, {cursor}"
            )));
        }
        let incoming = connections_to_port(
            project,
            &PortAddress::new(PortOwner::Node(cursor), IMAGE_INPUT_PORT),
        );
        let [connection] = incoming.as_slice() else {
            return Err(LibraryError::Project(format!(
                "Final semantic Image Node {cursor} has {} Image inputs",
                incoming.len()
            )));
        };
        let PortOwner::Node(upstream) = connection.from.owner else {
            break;
        };
        cursor = upstream;
    }
    Ok(owners)
}

fn dominating_image_transform(project: &Project, owner: NodeContainer) -> Option<Uuid> {
    let output_id = container_output_node_id(project, owner).ok().flatten()?;
    let mut candidates = container_node_ids(project, owner)
        .ok()?
        .iter()
        .copied()
        .filter(|node_id| project.get_node(*node_id).is_some_and(is_image_transform))
        .filter(|node_id| image_node_dominates_output(project, output_id, *node_id))
        .map(|node_id| {
            (
                image_distance_from_output(project, output_id, node_id),
                node_id,
            )
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(distance, node_id)| (*distance, *node_id));
    candidates.first().map(|(_, node_id)| *node_id)
}

fn image_node_dominates_output(project: &Project, output_id: Uuid, candidate: Uuid) -> bool {
    fn visit(
        project: &Project,
        cursor: Uuid,
        candidate: Uuid,
        visiting: &mut HashSet<Uuid>,
        memo: &mut HashMap<Uuid, bool>,
    ) -> bool {
        if cursor == candidate {
            return true;
        }
        if let Some(result) = memo.get(&cursor) {
            return *result;
        }
        if !visiting.insert(cursor) {
            return false;
        }
        let upstream = image_input_sources(project, cursor);
        let result = !upstream.is_empty()
            && upstream
                .into_iter()
                .all(|node_id| visit(project, node_id, candidate, visiting, memo));
        visiting.remove(&cursor);
        memo.insert(cursor, result);
        result
    }
    visit(
        project,
        output_id,
        candidate,
        &mut HashSet::new(),
        &mut HashMap::new(),
    )
}

fn image_distance_from_output(project: &Project, output_id: Uuid, candidate: Uuid) -> usize {
    let mut queue = VecDeque::from([(output_id, 0_usize)]);
    let mut visited = HashSet::new();
    while let Some((cursor, distance)) = queue.pop_front() {
        if cursor == candidate {
            return distance;
        }
        if visited.insert(cursor) {
            queue.extend(
                image_input_sources(project, cursor)
                    .into_iter()
                    .map(|node_id| (node_id, distance + 1)),
            );
        }
    }
    usize::MAX
}

fn image_input_sources(project: &Project, node_id: Uuid) -> Vec<Uuid> {
    project
        .connections
        .iter()
        .filter(|connection| connection.to.owner == PortOwner::Node(node_id))
        .filter(|connection| {
            project
                .port_definition(&connection.to, PortDirection::Input)
                .is_some_and(|port| port.data_type == PortDataType::Image)
        })
        .filter_map(|connection| match connection.from.owner {
            PortOwner::Node(upstream) => Some(upstream),
            _ => None,
        })
        .collect()
}

fn common_shape_transform(project: &Project, owner: NodeContainer) -> Option<Uuid> {
    let semantics = project.container_graph_semantics(container_port_owner(owner));
    let mut boundaries = container_node_ids(project, owner)
        .ok()?
        .iter()
        .copied()
        .filter(|node_id| {
            semantics.structurally_reaches_output(PortOwner::Node(*node_id))
                && project.get_node(*node_id).is_some_and(is_shape_style)
        })
        .filter_map(|style_id| {
            let incoming = connections_to_port(
                project,
                &PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
            );
            let [connection] = incoming.as_slice() else {
                return None;
            };
            Some(connection.from.clone())
        })
        .collect::<Vec<_>>();
    if boundaries.is_empty()
        && container_output_node_id(project, owner)
            .ok()
            .flatten()
            .is_none()
        && let Ok(source) = terminal_shape_source(project, owner)
    {
        boundaries.push(source);
    }
    let paths = boundaries
        .into_iter()
        .map(|source| shape_transform_path(project, source))
        .collect::<Vec<_>>();
    let first = paths.first()?;
    let common = first
        .iter()
        .copied()
        .filter(|node_id| paths.iter().all(|path| path.contains(node_id)))
        .collect::<Vec<_>>();
    common.into_iter().min_by_key(|node_id| {
        paths
            .iter()
            .filter_map(|path| path.iter().position(|candidate| candidate == node_id))
            .max()
            .unwrap_or(usize::MAX)
    })
}

fn shape_transform_path(project: &Project, mut source: PortAddress) -> Vec<Uuid> {
    let mut transforms = Vec::new();
    let mut visited = HashSet::new();
    while let PortOwner::Node(node_id) = source.owner {
        if !visited.insert(node_id) {
            break;
        }
        if project.get_node(node_id).is_some_and(is_shape_transform) {
            transforms.push(node_id);
        }
        let incoming = connections_to_port(
            project,
            &PortAddress::new(PortOwner::Node(node_id), SHAPE_INPUT_PORT),
        );
        let [connection] = incoming.as_slice() else {
            break;
        };
        source = connection.from.clone();
    }
    transforms
}

fn connections_to_port(project: &Project, target: &PortAddress) -> Vec<ProjectConnection> {
    project
        .connections
        .iter()
        .filter(|connection| &connection.to == target)
        .cloned()
        .collect()
}

fn is_image_transform(node: &crate::model::Node) -> bool {
    matches!(
        node.content(),
        NodeContent::PluginOperation(operation)
            if operation.category == TRANSFORM_CATEGORY
                && operation.component_id == IMAGE_TRANSFORM_COMPONENT_ID
                && operation.operation == TRANSFORM_APPLY_OPERATION
    )
}

fn is_shape_transform(node: &crate::model::Node) -> bool {
    matches!(
        node.content(),
        NodeContent::PluginOperation(operation)
            if operation.category == TRANSFORM_CATEGORY
                && operation.component_id == SHAPE_TRANSFORM_COMPONENT_ID
                && operation.operation == TRANSFORM_APPLY_OPERATION
    )
}

fn is_shape_style(node: &crate::model::Node) -> bool {
    matches!(
        node.content(),
        NodeContent::PluginOperation(operation)
            if operation.category == STYLE_CATEGORY
                && operation.component_id != IMAGE_OPACITY_STYLE_COMPONENT_ID
                && operation.operation == STYLE_APPLY_OPERATION
    )
}

fn ensure_graph_owners(
    project: &mut Project,
    owner: NodeContainer,
    plugins: &crate::plugin::PluginManager,
) -> Result<ResolvedGraphOwners, LibraryError> {
    let mut resolved = resolve_graph_owners(project, owner)?;
    if resolved.transform.is_none() {
        resolved.transform = Some(insert_transform(project, owner, plugins)?);
    }
    if resolved.opacity.is_none() {
        resolved.opacity = Some(append_image_opacity(project, owner, plugins)?);
    }
    absorb_legacy_transform(project, owner, resolved)?;
    absorb_legacy_opacity(project, owner, resolved)?;
    Ok(resolved)
}

fn insert_transform(
    project: &mut Project,
    owner: NodeContainer,
    plugins: &crate::plugin::PluginManager,
) -> Result<(Uuid, TransformKind), LibraryError> {
    let semantics = project.container_graph_semantics(container_port_owner(owner));
    let mut shape_style_inputs = Vec::new();
    for node_id in container_node_ids(project, owner)? {
        let Some(node) = project.get_node(*node_id) else {
            continue;
        };
        let NodeContent::PluginOperation(operation) = node.content() else {
            continue;
        };
        if operation.category != STYLE_CATEGORY
            || operation.component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID
            || operation.operation != STYLE_APPLY_OPERATION
            || !semantics.structurally_reaches_output(PortOwner::Node(*node_id))
            || !operation.declared_ports.iter().any(|port| {
                port.key == SHAPE_INPUT_PORT
                    && port.direction == crate::model::project::PortDirection::Input
            })
        {
            continue;
        }
        let target = PortAddress::new(PortOwner::Node(*node_id), SHAPE_INPUT_PORT);
        let incoming = project
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .collect::<Vec<_>>();
        let [connection] = incoming.as_slice() else {
            return Err(LibraryError::Project(format!(
                "Cannot synthesize Shape Transform for {owner:?}: Style Node {node_id} has {} Shape inputs",
                incoming.len()
            )));
        };
        shape_style_inputs.push((connection.id, connection.from.clone()));
    }

    if shape_style_inputs.is_empty() {
        if let Ok(source) = terminal_shape_source(project, owner) {
            let mut transform = plugins.create_shape_transform_operation_node()?;
            position_after_source(project, &mut transform, &source, 240.0);
            let transform_id = transform.id;
            project
                .insert_node_graph(
                    owner,
                    NodeGraphBundle::new(
                        vec![transform],
                        vec![ProjectConnection::new(
                            source,
                            PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT),
                            0,
                        )],
                        None,
                    ),
                )
                .map_err(|error| LibraryError::Project(error.to_string()))?;
            Ok((transform_id, TransformKind::Shape))
        } else {
            insert_image_transform(project, owner, plugins)
                .map(|node_id| (node_id, TransformKind::Image))
        }
    } else {
        let source = shape_style_inputs[0].1.clone();
        if shape_style_inputs
            .iter()
            .any(|(_, candidate)| candidate != &source)
        {
            return insert_image_transform(project, owner, plugins)
                .map(|node_id| (node_id, TransformKind::Image));
        }
        let mut transform = plugins.create_shape_transform_operation_node()?;
        position_after_source(project, &mut transform, &source, 240.0);
        let transform_id = transform.id;
        project
            .insert_node_graph(
                owner,
                NodeGraphBundle::new(
                    vec![transform],
                    vec![ProjectConnection::new(
                        source,
                        PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT),
                        0,
                    )],
                    None,
                ),
            )
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        for (connection_id, _) in shape_style_inputs {
            let connection = project
                .connections
                .iter_mut()
                .find(|connection| connection.id == connection_id)
                .ok_or_else(|| {
                    LibraryError::Project(format!(
                        "Shape Style connection {connection_id} disappeared during synthesis"
                    ))
                })?;
            connection.from = PortAddress::new(PortOwner::Node(transform_id), SHAPE_OUTPUT_PORT);
        }
        validate_candidate(project, owner)?;
        Ok((transform_id, TransformKind::Shape))
    }
}

fn insert_image_transform(
    project: &mut Project,
    owner: NodeContainer,
    plugins: &crate::plugin::PluginManager,
) -> Result<Uuid, LibraryError> {
    let opacity = resolve_graph_owners(project, owner)?.opacity;
    if let Some(opacity_id) = opacity {
        let target = PortAddress::new(PortOwner::Node(opacity_id), IMAGE_INPUT_PORT);
        let incoming = project
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .collect::<Vec<_>>();
        let [connection] = incoming.as_slice() else {
            return Err(LibraryError::Project(format!(
                "Cannot synthesize Image Transform for {owner:?}: Image Opacity Node {opacity_id} has {} Image inputs",
                incoming.len()
            )));
        };
        let connection_id = connection.id;
        let source = connection.from.clone();
        let mut transform = plugins.create_image_transform_operation_node()?;
        position_after_source(project, &mut transform, &source, 240.0);
        let transform_id = transform.id;
        project
            .insert_node_graph(
                owner,
                NodeGraphBundle::new(
                    vec![transform],
                    vec![ProjectConnection::new(
                        source,
                        PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
                        0,
                    )],
                    None,
                ),
            )
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        let downstream = project
            .connections
            .iter_mut()
            .find(|connection| connection.id == connection_id)
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Image Opacity connection {connection_id} disappeared during synthesis"
                ))
            })?;
        downstream.from = PortAddress::new(PortOwner::Node(transform_id), IMAGE_OUTPUT_PORT);
        validate_candidate(project, owner)?;
        return Ok(transform_id);
    }

    let output_id = container_output_node_id(project, owner)?.ok_or_else(|| {
        LibraryError::Project(format!(
            "Cannot synthesize Image Transform for {owner:?}: container has NoOutput"
        ))
    })?;
    let mut transform = plugins.create_image_transform_operation_node()?;
    let source = PortAddress::new(PortOwner::Node(output_id), IMAGE_OUTPUT_PORT);
    position_after_source(project, &mut transform, &source, 240.0);
    let transform_id = transform.id;
    project
        .insert_node_graph(
            owner,
            NodeGraphBundle::new(
                vec![transform],
                vec![ProjectConnection::new(
                    source,
                    PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
                    0,
                )],
                Some(transform_id),
            ),
        )
        .map_err(|error| LibraryError::Project(error.to_string()))?;
    Ok(transform_id)
}

fn append_image_opacity(
    project: &mut Project,
    owner: NodeContainer,
    plugins: &crate::plugin::PluginManager,
) -> Result<Uuid, LibraryError> {
    let output_id = container_output_node_id(project, owner)?.ok_or_else(|| {
        LibraryError::Project(format!(
            "Cannot synthesize Image Opacity for {owner:?}: container has NoOutput"
        ))
    })?;
    let source = PortAddress::new(PortOwner::Node(output_id), IMAGE_OUTPUT_PORT);
    let mut opacity = plugins.create_image_opacity_style_operation_node()?;
    position_after_source(project, &mut opacity, &source, 240.0);
    let opacity_id = opacity.id;
    project
        .insert_node_graph(
            owner,
            NodeGraphBundle::new(
                vec![opacity],
                vec![ProjectConnection::new(
                    source,
                    PortAddress::new(PortOwner::Node(opacity_id), IMAGE_INPUT_PORT),
                    0,
                )],
                Some(opacity_id),
            ),
        )
        .map_err(|error| LibraryError::Project(error.to_string()))?;
    Ok(opacity_id)
}

fn absorb_legacy_transform(
    project: &mut Project,
    owner: NodeContainer,
    resolved: ResolvedGraphOwners,
) -> Result<(), LibraryError> {
    let legacy = container_properties(project, owner)?.clone();
    let transform_id = resolved
        .transform
        .map(|(node_id, _)| node_id)
        .ok_or_else(|| {
            LibraryError::Project("Semantic Transform was not synthesized".to_string())
        })?;
    for key in TRANSFORM_PROPERTIES {
        let Some(property) = legacy.get(key) else {
            continue;
        };
        if !is_neutral_legacy(key, property) {
            absorb_property(project, owner, transform_id, key, property.clone())?;
        }
    }
    let properties = container_properties_mut(project, owner)?;
    for key in TRANSFORM_PROPERTIES {
        properties.remove(key);
    }
    Ok(())
}

fn absorb_legacy_opacity(
    project: &mut Project,
    owner: NodeContainer,
    resolved: ResolvedGraphOwners,
) -> Result<(), LibraryError> {
    let legacy = container_properties(project, owner)?.clone();
    let opacity_id = resolved.opacity.ok_or_else(|| {
        LibraryError::Project("Semantic Image Opacity was not synthesized".to_string())
    })?;
    if let Some(property) = legacy.get("opacity")
        && !is_neutral_legacy("opacity", property)
    {
        absorb_property(
            project,
            owner,
            opacity_id,
            "opacity",
            scale_number_property(property, 0.01)?,
        )?;
    }
    container_properties_mut(project, owner)?.remove("opacity");
    Ok(())
}

fn absorb_property(
    project: &mut Project,
    owner: NodeContainer,
    node_id: Uuid,
    key: &str,
    legacy: Property,
) -> Result<(), LibraryError> {
    if let Some(source) = connected_property_source(project, node_id, key) {
        return Err(LibraryError::Project(format!(
            "Cannot absorb legacy {key} for {owner:?}: semantic Node {node_id} property is wired from {source:?}"
        )));
    }
    let target = project
        .get_node(node_id)
        .and_then(|node| node.properties().get(key))
        .cloned()
        .ok_or_else(|| {
            LibraryError::Project(format!("Semantic Node {node_id} is missing property {key}"))
        })?;
    if !is_default_graph_property(key, &target) {
        return Err(conflicting_authority(node_id, key));
    }
    project
        .get_node_mut(node_id)
        .ok_or_else(|| LibraryError::Project(format!("Semantic Node {node_id} not found")))?
        .set_property(key.to_string(), legacy)
        .map_err(LibraryError::Project)?;
    Ok(())
}

fn reject_wired_property(
    project: &Project,
    node_id: Uuid,
    key: &str,
    owner: NodeContainer,
) -> Result<(), LibraryError> {
    if let Some(source) = connected_property_source(project, node_id, key) {
        return Err(LibraryError::Project(format!(
            "Cannot edit semantic {key} for {owner:?}: Node {node_id} property is wired from {source:?}; edit the wire or exact Node instead"
        )));
    }
    Ok(())
}
