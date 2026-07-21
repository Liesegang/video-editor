//! Read-only Clip/Track/Composition property-stack projection.
//!
//! This is presentation data rebuilt from the authoritative Project graph.
//! It intentionally contains cloned Properties and must never be persisted.

use std::collections::{BTreeSet, HashMap, HashSet};

use uuid::Uuid;

use super::super::lifecycle::ProjectManager;
use super::helpers::{connected_property_source, container_node_ids, container_properties};
use super::{build_projection, container_port_owner, resolve_graph_owners};
use crate::error::LibraryError;
use crate::model::project::{
    NodeContainer, PortAddress, PortDataType, PortDirection, PortOwner, Project,
};
use crate::model::property::{Property, PropertyDefinition, PropertyUiType, PropertyValue};
use crate::model::{GeneratorContent, Node, NodeContent};
use crate::plugin::{
    DECORATOR_CATEGORY, EFFECT_CATEGORY, EFFECTOR_CATEGORY, IMAGE_OPACITY_STYLE_COMPONENT_ID,
    IMAGE_TRANSFORM_COMPONENT_ID, SHAPE_TRANSFORM_COMPONENT_ID, STYLE_CATEGORY, TRANSFORM_CATEGORY,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticPropertyGroup {
    Timing,
    Container,
    Source,
    Transform,
    Decorator,
    Effector,
    Style,
    Effect,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticPropertyOwner {
    DirectClip(Uuid),
    SemanticContainer(NodeContainer),
    ExactNode(Uuid),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticPropertyAccess {
    Editable,
    Wired {
        source: PortAddress,
    },
    ReadOnly {
        reason: String,
        related_nodes: Vec<Uuid>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticAnimationSupport {
    ConstantOnly,
    Evaluator,
}

#[derive(Clone, Debug)]
pub struct SemanticPropertyEntry {
    key: String,
    label: String,
    definition: Option<PropertyDefinition>,
    property: Property,
    access: SemanticPropertyAccess,
    animation: SemanticAnimationSupport,
}

impl SemanticPropertyEntry {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn definition(&self) -> Option<&PropertyDefinition> {
        self.definition.as_ref()
    }

    pub fn property(&self) -> &Property {
        &self.property
    }

    pub fn access(&self) -> &SemanticPropertyAccess {
        &self.access
    }

    pub fn animation(&self) -> SemanticAnimationSupport {
        self.animation
    }
}

#[derive(Clone, Debug)]
pub struct SemanticPropertySection {
    stable_id: String,
    label: String,
    group: SemanticPropertyGroup,
    owner: SemanticPropertyOwner,
    node_id: Option<Uuid>,
    properties: Vec<SemanticPropertyEntry>,
    diagnostics: Vec<String>,
}

impl SemanticPropertySection {
    pub fn stable_id(&self) -> &str {
        &self.stable_id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn group(&self) -> SemanticPropertyGroup {
        self.group
    }

    pub fn owner(&self) -> SemanticPropertyOwner {
        self.owner
    }

    pub fn node_id(&self) -> Option<Uuid> {
        self.node_id
    }

    pub fn properties(&self) -> &[SemanticPropertyEntry] {
        &self.properties
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}

#[derive(Clone, Debug)]
pub struct SemanticContainerPropertyStack {
    owner: NodeContainer,
    sections: Vec<SemanticPropertySection>,
    diagnostics: Vec<String>,
}

impl SemanticContainerPropertyStack {
    pub fn owner(&self) -> NodeContainer {
        self.owner
    }

    pub fn sections(&self) -> &[SemanticPropertySection] {
        &self.sections
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}

impl ProjectManager {
    /// Enumerates the complete visual property stack without mutating or
    /// repairing the Project. Shape and Image connections define the generic
    /// chain, so future Shape -> Shape operations participate automatically.
    pub fn semantic_container_property_stack(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticContainerPropertyStack, LibraryError> {
        let project = self
            .project
            .read()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        build_stack(&project, owner, self)
    }
}

fn build_stack(
    project: &Project,
    owner: NodeContainer,
    manager: &ProjectManager,
) -> Result<SemanticContainerPropertyStack, LibraryError> {
    // Validate ownership even when the container currently has no visual result.
    let _ = container_node_ids(project, owner)?;
    let mut diagnostics = Vec::new();
    let mut sections = direct_sections(project, owner);

    let semantic = resolve_graph_owners(project, owner)
        .and_then(|resolved| {
            build_projection(project, owner, resolved, &manager.plugin_manager)
                .map(|projection| (resolved, projection))
        })
        .inspect_err(|error| {
            diagnostics.push(error.to_string());
        })
        .ok();

    let mut transform_section = semantic
        .as_ref()
        .map(|(_, projection)| semantic_section(projection, false));
    let mut opacity_section = semantic
        .as_ref()
        .map(|(_, projection)| semantic_section(projection, true));
    let transform_id = semantic
        .as_ref()
        .and_then(|(resolved, _)| resolved.transform.map(|(node_id, _)| node_id));
    let opacity_id = semantic.as_ref().and_then(|(resolved, _)| resolved.opacity);

    let ambiguous = semantic
        .is_none()
        .then(|| special_candidates(project, owner));
    for node_id in topological_visual_nodes(project, owner)? {
        if Some(node_id) == transform_id {
            if let Some(section) = transform_section.take() {
                sections.push(section);
            }
            continue;
        }
        if Some(node_id) == opacity_id {
            if let Some(section) = opacity_section.take() {
                sections.push(section);
            }
            continue;
        }
        let Some(node) = project.get_node(node_id) else {
            continue;
        };
        if let Some(section) = exact_node_section(project, owner, node, manager, ambiguous.as_ref())
        {
            sections.push(section);
        }
    }

    // An un-authored semantic control remains editable: its first write will
    // atomically synthesize the typed Node at the graph boundary.
    if let Some(section) = transform_section {
        sections.push(section);
    }
    if let Some(section) = opacity_section {
        sections.push(section);
    }

    Ok(SemanticContainerPropertyStack {
        owner,
        sections,
        diagnostics,
    })
}

fn direct_sections(project: &Project, owner: NodeContainer) -> Vec<SemanticPropertySection> {
    let mut sections = Vec::new();
    if let NodeContainer::Clip(clip_id) = owner
        && let Some(clip) = project.get_clip(clip_id)
    {
        let properties = crate::model::Clip::timing_property_definitions()
            .iter()
            .filter_map(|definition| {
                clip.timing_property_value(definition.name())
                    .map(|value| SemanticPropertyEntry {
                        key: definition.name().to_string(),
                        label: definition.label().to_string(),
                        definition: Some(definition.clone()),
                        property: Property::constant(value),
                        access: SemanticPropertyAccess::Editable,
                        animation: SemanticAnimationSupport::ConstantOnly,
                    })
            })
            .collect();
        sections.push(SemanticPropertySection {
            stable_id: "clip:timing".to_string(),
            label: "Timing".to_string(),
            group: SemanticPropertyGroup::Timing,
            owner: SemanticPropertyOwner::DirectClip(clip_id),
            node_id: None,
            properties,
            diagnostics: Vec::new(),
        });
    }

    if let Ok(properties) = container_properties(project, owner) {
        let mut keys = properties
            .iter()
            .map(|(key, _)| key.as_str())
            .filter(|key| !super::SEMANTIC_PROPERTIES.contains(key))
            .collect::<Vec<_>>();
        keys.sort_unstable();
        let entries = keys
            .into_iter()
            .filter_map(|key| {
                let property = properties.get(key)?.clone();
                Some(entry_from_property(
                    key,
                    None,
                    property,
                    if matches!(owner, NodeContainer::Clip(_)) {
                        SemanticPropertyAccess::Editable
                    } else {
                        SemanticPropertyAccess::ReadOnly {
                            reason: "Direct Track/Composition property authoring is not exposed by this facade"
                                .to_string(),
                            related_nodes: Vec::new(),
                        }
                    },
                ))
            })
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            let direct_owner = match owner {
                NodeContainer::Clip(id) => SemanticPropertyOwner::DirectClip(id),
                _ => SemanticPropertyOwner::SemanticContainer(owner),
            };
            sections.push(SemanticPropertySection {
                stable_id: "container:properties".to_string(),
                label: "Properties".to_string(),
                group: SemanticPropertyGroup::Container,
                owner: direct_owner,
                node_id: None,
                properties: entries,
                diagnostics: Vec::new(),
            });
        }
    }
    sections
}

fn semantic_section(
    projection: &super::SemanticContainerPropertyProjection,
    opacity: bool,
) -> SemanticPropertySection {
    let keys: &[&str] = if opacity {
        &["opacity"]
    } else {
        &super::TRANSFORM_PROPERTIES
    };
    let properties = keys
        .iter()
        .filter_map(|key| {
            let property = projection.properties().get(key)?.clone();
            let definition = projection
                .definitions()
                .iter()
                .find(|definition| definition.name() == *key)
                .cloned();
            let access = projection
                .binding(key)
                .and_then(|binding| binding.connected_source.clone())
                .map_or(SemanticPropertyAccess::Editable, |source| {
                    SemanticPropertyAccess::Wired { source }
                });
            Some(entry_from_property(key, definition, property, access))
        })
        .collect::<Vec<_>>();
    let node_id = keys
        .iter()
        .find_map(|key| projection.binding(key).and_then(|binding| binding.node_id));
    SemanticPropertySection {
        stable_id: if opacity {
            "semantic:opacity".to_string()
        } else {
            "semantic:transform".to_string()
        },
        label: if opacity { "Opacity" } else { "Transform" }.to_string(),
        group: if opacity {
            SemanticPropertyGroup::Style
        } else {
            SemanticPropertyGroup::Transform
        },
        owner: SemanticPropertyOwner::SemanticContainer(projection.owner()),
        node_id,
        properties,
        diagnostics: Vec::new(),
    }
}

fn exact_node_section(
    project: &Project,
    owner: NodeContainer,
    node: &Node,
    manager: &ProjectManager,
    ambiguous: Option<&HashMap<Uuid, (String, Vec<Uuid>)>>,
) -> Option<SemanticPropertySection> {
    node.properties().iter().next()?;
    let metadata = node_metadata(project, owner, node, manager);
    let mut diagnostics = metadata.diagnostic.into_iter().collect::<Vec<_>>();
    let definition_names = metadata
        .definitions
        .iter()
        .map(|definition| definition.name())
        .collect::<HashSet<_>>();
    for definition in &metadata.definitions {
        if node.properties().get(definition.name()).is_none() {
            diagnostics.push(format!(
                "Node {} is missing declared property {}",
                node.id,
                definition.name()
            ));
        }
    }

    let unavailable = metadata.unavailable_reason;
    let ambiguous_access = ambiguous.and_then(|items| items.get(&node.id));
    let mut properties = Vec::new();
    for definition in &metadata.definitions {
        let Some(property) = node.properties().get(definition.name()).cloned() else {
            continue;
        };
        let access = exact_access(
            project,
            node.id,
            definition.name(),
            ambiguous_access,
            unavailable.as_deref(),
        );
        properties.push(entry_from_property(
            definition.name(),
            Some(definition.clone()),
            property,
            access,
        ));
    }
    let mut unknown = node
        .properties()
        .iter()
        .filter(|(key, _)| !definition_names.contains(key.as_str()))
        .map(|(key, property)| (key.clone(), property.clone()))
        .collect::<Vec<_>>();
    unknown.sort_by(|left, right| left.0.cmp(&right.0));
    for (key, property) in unknown {
        diagnostics.push(format!(
            "Node {} property {key} has no authoritative metadata",
            node.id
        ));
        properties.push(entry_from_property(
            &key,
            inferred_definition(&key, &property),
            property,
            SemanticPropertyAccess::ReadOnly {
                reason: "Property has no authoritative metadata".to_string(),
                related_nodes: vec![node.id],
            },
        ));
    }
    Some(SemanticPropertySection {
        stable_id: format!("node:{}", node.id),
        label: metadata.label,
        group: metadata.group,
        owner: SemanticPropertyOwner::ExactNode(node.id),
        node_id: Some(node.id),
        properties,
        diagnostics,
    })
}

fn exact_access(
    project: &Project,
    node_id: Uuid,
    key: &str,
    ambiguous: Option<&(String, Vec<Uuid>)>,
    unavailable: Option<&str>,
) -> SemanticPropertyAccess {
    if let Some((reason, related_nodes)) = ambiguous {
        return SemanticPropertyAccess::ReadOnly {
            reason: reason.clone(),
            related_nodes: related_nodes.clone(),
        };
    }
    if let Some(reason) = unavailable {
        return SemanticPropertyAccess::ReadOnly {
            reason: reason.to_string(),
            related_nodes: vec![node_id],
        };
    }
    connected_property_source(project, node_id, key)
        .map_or(SemanticPropertyAccess::Editable, |source| {
            SemanticPropertyAccess::Wired { source }
        })
}

struct NodeMetadata {
    label: String,
    group: SemanticPropertyGroup,
    definitions: Vec<PropertyDefinition>,
    diagnostic: Option<String>,
    unavailable_reason: Option<String>,
}

fn node_metadata(
    project: &Project,
    owner: NodeContainer,
    node: &Node,
    manager: &ProjectManager,
) -> NodeMetadata {
    match node.content() {
        NodeContent::PluginOperation(operation) => {
            match manager.plugin_manager.operation_descriptor(
                &operation.category,
                &operation.component_id,
                &operation.operation,
            ) {
                Ok(descriptor) => {
                    let incompatible = (!descriptor
                    .is_execution_compatible_with_ports(&operation.declared_ports))
                .then(|| {
                    format!(
                        "Installed descriptor for {}/{}/{} is incompatible with persisted ports",
                        operation.category, operation.component_id, operation.operation
                    )
                });
                    NodeMetadata {
                        label: descriptor.label().to_string(),
                        group: operation_group(&operation.category, &operation.component_id),
                        definitions: descriptor.properties().to_vec(),
                        diagnostic: incompatible.clone(),
                        unavailable_reason: incompatible,
                    }
                }
                Err(error) => {
                    let reason = format!(
                        "Operation {}/{}/{} is unavailable: {error}",
                        operation.category, operation.component_id, operation.operation
                    );
                    NodeMetadata {
                        label: node.name.clone(),
                        group: operation_group(&operation.category, &operation.component_id),
                        definitions: Vec::new(),
                        diagnostic: Some(reason.clone()),
                        unavailable_reason: Some(reason),
                    }
                }
            }
        }
        NodeContent::Generator(generator) => {
            let kind = match generator {
                GeneratorContent::Text => "text",
                GeneratorContent::Shape => "shape",
                GeneratorContent::Solid => "solid",
                GeneratorContent::SkSL => "sksl",
            };
            converter_metadata(
                project,
                owner,
                node,
                manager,
                kind,
                SemanticPropertyGroup::Source,
            )
        }
        NodeContent::Media(media) => {
            let kind = project
                .get_asset(media.asset_id)
                .map(|asset| match asset.kind {
                    crate::model::asset::AssetKind::Video => "video",
                    crate::model::asset::AssetKind::Image => "image",
                    crate::model::asset::AssetKind::Audio => "audio",
                    _ => "unknown",
                });
            kind.map_or_else(
                || unavailable_metadata(node, "Media asset or converter kind is unavailable"),
                |kind| {
                    converter_metadata(
                        project,
                        owner,
                        node,
                        manager,
                        kind,
                        SemanticPropertyGroup::Source,
                    )
                },
            )
        }
        NodeContent::Value(value) => NodeMetadata {
            label: value.label().to_string(),
            group: SemanticPropertyGroup::Other,
            definitions: value.property_definitions().to_vec(),
            diagnostic: None,
            unavailable_reason: None,
        },
        NodeContent::CompositionInstance(_) | NodeContent::Merge => NodeMetadata {
            label: node.name.clone(),
            group: SemanticPropertyGroup::Source,
            definitions: Vec::new(),
            diagnostic: None,
            unavailable_reason: None,
        },
    }
}

fn converter_metadata(
    project: &Project,
    owner: NodeContainer,
    node: &Node,
    manager: &ProjectManager,
    kind: &str,
    group: SemanticPropertyGroup,
) -> NodeMetadata {
    let Some(converter) = manager.plugin_manager.get_entity_converter(kind) else {
        return unavailable_metadata(node, &format!("Entity converter {kind:?} is unavailable"));
    };
    let dimensions = project
        .find_containing_composition(container_port_owner(owner).id())
        .and_then(|id| project.get_composition(id))
        .map_or((1920, 1080), |composition| {
            (composition.width, composition.height)
        });
    NodeMetadata {
        label: node.name.clone(),
        group,
        definitions: converter.get_property_definitions(
            dimensions.0,
            dimensions.1,
            dimensions.0,
            dimensions.1,
        ),
        diagnostic: None,
        unavailable_reason: None,
    }
}

fn unavailable_metadata(node: &Node, reason: &str) -> NodeMetadata {
    NodeMetadata {
        label: node.name.clone(),
        group: SemanticPropertyGroup::Source,
        definitions: Vec::new(),
        diagnostic: Some(reason.to_string()),
        unavailable_reason: Some(reason.to_string()),
    }
}

fn operation_group(category: &str, component_id: &str) -> SemanticPropertyGroup {
    match category {
        TRANSFORM_CATEGORY => SemanticPropertyGroup::Transform,
        DECORATOR_CATEGORY => SemanticPropertyGroup::Decorator,
        EFFECTOR_CATEGORY => SemanticPropertyGroup::Effector,
        STYLE_CATEGORY if component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID => {
            SemanticPropertyGroup::Style
        }
        STYLE_CATEGORY => SemanticPropertyGroup::Style,
        EFFECT_CATEGORY => SemanticPropertyGroup::Effect,
        _ => SemanticPropertyGroup::Other,
    }
}

fn special_candidates(
    project: &Project,
    owner: NodeContainer,
) -> HashMap<Uuid, (String, Vec<Uuid>)> {
    let semantics = project.container_graph_semantics(container_port_owner(owner));
    let mut transform = Vec::new();
    let mut opacity = Vec::new();
    for node_id in container_node_ids(project, owner).unwrap_or_default() {
        let Some(NodeContent::PluginOperation(operation)) =
            project.get_node(*node_id).map(Node::content)
        else {
            continue;
        };
        if !semantics.structurally_reaches_output(PortOwner::Node(*node_id)) {
            continue;
        }
        if operation.category == TRANSFORM_CATEGORY
            && matches!(
                operation.component_id.as_str(),
                SHAPE_TRANSFORM_COMPONENT_ID | IMAGE_TRANSFORM_COMPONENT_ID
            )
        {
            transform.push(*node_id);
        }
        if operation.category == STYLE_CATEGORY
            && operation.component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID
        {
            opacity.push(*node_id);
        }
    }
    transform.sort_unstable();
    opacity.sort_unstable();
    let mut result = HashMap::new();
    for (label, candidates) in [("Transform", transform), ("Opacity", opacity)] {
        if candidates.is_empty() {
            continue;
        }
        let reason = format!(
            "Semantic {label} authority is ambiguous; select an exact Node: {}",
            candidates
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        for node_id in &candidates {
            result.insert(*node_id, (reason.clone(), candidates.clone()));
        }
    }
    result
}

fn topological_visual_nodes(
    project: &Project,
    owner: NodeContainer,
) -> Result<Vec<Uuid>, LibraryError> {
    let semantics = project.container_graph_semantics(container_port_owner(owner));
    let nodes = container_node_ids(project, owner)?
        .iter()
        .copied()
        .filter(|node_id| semantics.structurally_reaches_output(PortOwner::Node(*node_id)))
        .collect::<HashSet<_>>();
    let mut indegree = nodes
        .iter()
        .copied()
        .map(|node_id| (node_id, 0_usize))
        .collect::<HashMap<_, _>>();
    let mut outgoing = HashMap::<Uuid, Vec<(i64, Uuid, Uuid)>>::new();
    for connection in &project.connections {
        let (PortOwner::Node(from), PortOwner::Node(to)) =
            (connection.from.owner, connection.to.owner)
        else {
            continue;
        };
        if !nodes.contains(&from) || !nodes.contains(&to) {
            continue;
        }
        let source = project.port_definition(&connection.from, PortDirection::Output);
        let target = project.port_definition(&connection.to, PortDirection::Input);
        let visual = source.zip(target).is_some_and(|(source, target)| {
            matches!(source.data_type, PortDataType::Image | PortDataType::Shape)
                && source.data_type == target.data_type
        });
        if !visual {
            continue;
        }
        *indegree.entry(to).or_default() += 1;
        outgoing
            .entry(from)
            .or_default()
            .push((connection.order, connection.id, to));
    }
    for edges in outgoing.values_mut() {
        edges.sort_unstable();
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(node_id, degree)| (*degree == 0).then_some(*node_id))
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(nodes.len());
    while let Some(node_id) = ready.pop_first() {
        ordered.push(node_id);
        for (_, _, target) in outgoing.get(&node_id).into_iter().flatten() {
            let Some(degree) = indegree.get_mut(target) else {
                continue;
            };
            *degree = degree.saturating_sub(1);
            if *degree == 0 {
                ready.insert(*target);
            }
        }
    }
    if ordered.len() != nodes.len() {
        return Err(LibraryError::Project(format!(
            "Visual graph for {owner:?} contains a cycle"
        )));
    }
    Ok(ordered)
}

fn entry_from_property(
    key: &str,
    definition: Option<PropertyDefinition>,
    property: Property,
    access: SemanticPropertyAccess,
) -> SemanticPropertyEntry {
    let definition = definition.or_else(|| inferred_definition(key, &property));
    let label = definition.as_ref().map_or_else(
        || title_case(key),
        |definition| definition.label().to_string(),
    );
    SemanticPropertyEntry {
        key: key.to_string(),
        label,
        definition,
        property,
        access,
        animation: SemanticAnimationSupport::Evaluator,
    }
}

fn inferred_definition(key: &str, property: &Property) -> Option<PropertyDefinition> {
    let value = property.value()?.clone();
    let ui_type = match &value {
        PropertyValue::Number(_) => PropertyUiType::Float {
            min: -1_000_000.0,
            max: 1_000_000.0,
            step: 0.1,
            suffix: String::new(),
            min_hard_limit: false,
            max_hard_limit: false,
        },
        PropertyValue::Integer(_) => PropertyUiType::Integer {
            min: i64::MIN,
            max: i64::MAX,
            suffix: String::new(),
            min_hard_limit: false,
            max_hard_limit: false,
        },
        PropertyValue::String(_) => PropertyUiType::Text,
        PropertyValue::Boolean(_) => PropertyUiType::Bool,
        PropertyValue::Vec2(_) => PropertyUiType::vec2(""),
        PropertyValue::Vec3(_) => PropertyUiType::vec3(""),
        PropertyValue::Vec4(_) => PropertyUiType::vec4(""),
        PropertyValue::Color(_) => PropertyUiType::Color,
        PropertyValue::Array(_) | PropertyValue::Map(_) => return None,
    };
    Some(PropertyDefinition::new(
        key,
        ui_type,
        &title_case(key),
        value,
    ))
}

fn title_case(key: &str) -> String {
    key.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}
