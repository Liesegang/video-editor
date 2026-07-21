use std::collections::HashSet;
use std::sync::Arc;

use library::editor::project_service::{
    SemanticAnimationSupport, SemanticContainerPropertyStack, SemanticPropertyAccess,
    SemanticPropertyGroup, SemanticPropertyOwner,
};
use library::model::project::{NodeContainer, Project};
use library::model::property::{Property, PropertyDefinition, PropertyMap};
use library::model::Node;
use library::PropertyOwner;
use uuid::Uuid;

use crate::state::context_types::SelectionTarget;

use super::actions::graph_property_name;
use super::utils::{
    numeric_property_components, time_mapper_for_owner, PropertyComponent, TimeMapper,
};

#[derive(Clone)]
pub struct GraphPropertyRow {
    pub stable_id: String,
    pub label: String,
    pub property_key: String,
    pub definition: Option<PropertyDefinition>,
    pub property: Property,
    pub property_map: Arc<PropertyMap>,
    pub component: Option<PropertyComponent>,
    pub owner: SemanticPropertyOwner,
    pub access: SemanticPropertyAccess,
    pub animation: SemanticAnimationSupport,
    pub time_mapper: TimeMapper,
}

impl GraphPropertyRow {
    pub fn is_plottable(&self) -> bool {
        self.component.is_some() && !matches!(self.access, SemanticPropertyAccess::Wired { .. })
    }

    pub fn is_editable(&self) -> bool {
        matches!(self.access, SemanticPropertyAccess::Editable)
            && self.animation == SemanticAnimationSupport::Evaluator
    }

    pub fn access_label(&self) -> Option<String> {
        match &self.access {
            SemanticPropertyAccess::Editable => None,
            SemanticPropertyAccess::Wired { source } => Some(format!("Wired from {source:?}")),
            SemanticPropertyAccess::ReadOnly { reason, .. } => Some(reason.clone()),
        }
    }
}

#[derive(Clone)]
pub struct GraphPropertySection {
    pub stable_id: String,
    pub label: String,
    pub group: SemanticPropertyGroup,
    pub owner: SemanticPropertyOwner,
    pub node_id: Option<Uuid>,
    pub rows: Vec<GraphPropertyRow>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone)]
pub struct GraphPropertyProjection {
    pub target: SelectionTarget,
    pub sections: Vec<GraphPropertySection>,
    pub diagnostics: Vec<String>,
}

impl GraphPropertyProjection {
    pub fn exact_node(project: &Project, node: &Node, definitions: &[PropertyDefinition]) -> Self {
        let property_map = Arc::new(node.properties().clone());
        let owner = SemanticPropertyOwner::ExactNode(node.id);
        let mapper = time_mapper_for_owner(project, PropertyOwner::Node(node.id));
        let mut rows = Vec::new();
        let mut known = HashSet::with_capacity(definitions.len());
        for definition in definitions {
            if !known.insert(definition.name()) {
                continue;
            }
            let Some(property) = node.properties().get(definition.name()) else {
                continue;
            };
            append_rows(
                &mut rows,
                SelectionTarget::Node(node.id),
                "exact",
                definition.name(),
                definition.label(),
                Some(definition.clone()),
                property.clone(),
                Arc::clone(&property_map),
                owner,
                SemanticPropertyAccess::Editable,
                SemanticAnimationSupport::Evaluator,
                mapper,
            );
        }
        let mut extras = node
            .properties()
            .iter()
            .filter(|(key, _)| !known.contains(key.as_str()))
            .collect::<Vec<_>>();
        extras.sort_by_key(|(key, _)| key.as_str());
        for (key, property) in extras {
            append_rows(
                &mut rows,
                SelectionTarget::Node(node.id),
                "exact",
                key,
                key,
                None,
                property.clone(),
                Arc::clone(&property_map),
                owner,
                SemanticPropertyAccess::Editable,
                SemanticAnimationSupport::Evaluator,
                mapper,
            );
        }
        Self {
            target: SelectionTarget::Node(node.id),
            sections: vec![GraphPropertySection {
                stable_id: format!("node:{}", node.id),
                label: node.name.clone(),
                group: SemanticPropertyGroup::Other,
                owner,
                node_id: Some(node.id),
                rows,
                diagnostics: Vec::new(),
            }],
            diagnostics: Vec::new(),
        }
    }

    pub fn semantic(project: &Project, stack: &SemanticContainerPropertyStack) -> Self {
        let target = selection_for_container(stack.owner());
        let sections = stack
            .sections()
            .iter()
            .map(|section| {
                let mut property_map = PropertyMap::new();
                for entry in section.properties() {
                    property_map.set(entry.key().to_string(), entry.property().clone());
                }
                let property_map = Arc::new(property_map);
                let mapper = time_mapper_for_semantic_owner(project, section.owner());
                let mut rows = Vec::new();
                for entry in section.properties() {
                    append_rows(
                        &mut rows,
                        target,
                        section.stable_id(),
                        entry.key(),
                        entry.label(),
                        entry.definition().cloned(),
                        entry.property().clone(),
                        Arc::clone(&property_map),
                        section.owner(),
                        entry.access().clone(),
                        entry.animation(),
                        mapper,
                    );
                }
                GraphPropertySection {
                    stable_id: section.stable_id().to_string(),
                    label: section.label().to_string(),
                    group: section.group(),
                    owner: section.owner(),
                    node_id: section.node_id(),
                    rows,
                    diagnostics: section.diagnostics().to_vec(),
                }
            })
            .collect();
        Self {
            target,
            sections,
            diagnostics: stack.diagnostics().to_vec(),
        }
    }

    pub fn rows(&self) -> impl Iterator<Item = &GraphPropertyRow> {
        self.sections.iter().flat_map(|section| section.rows.iter())
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one transient Graph row preserves the complete semantic stack entry contract"
)]
fn append_rows(
    output: &mut Vec<GraphPropertyRow>,
    target: SelectionTarget,
    section_id: &str,
    property_key: &str,
    label: &str,
    definition: Option<PropertyDefinition>,
    property: Property,
    property_map: Arc<PropertyMap>,
    owner: SemanticPropertyOwner,
    access: SemanticPropertyAccess,
    animation: SemanticAnimationSupport,
    time_mapper: TimeMapper,
) {
    let components = numeric_property_components(definition.as_ref(), &property);
    if components.is_empty() {
        let stable_id = if matches!(target, SelectionTarget::Node(_)) {
            format!("node:{property_key}")
        } else {
            semantic_row_id(target, section_id, owner, property_key, None)
        };
        output.push(GraphPropertyRow {
            stable_id,
            label: label.to_string(),
            property_key: property_key.to_string(),
            definition,
            property,
            property_map,
            component: None,
            owner,
            access,
            animation,
            time_mapper,
        });
        return;
    }
    for component in components {
        let stable_id = if matches!(target, SelectionTarget::Node(_)) {
            graph_property_name(property_key, component)
        } else {
            semantic_row_id(target, section_id, owner, property_key, Some(component))
        };
        output.push(GraphPropertyRow {
            stable_id,
            label: format!("{label}.{}", component_label(component)),
            property_key: property_key.to_string(),
            definition: definition.clone(),
            property: property.clone(),
            property_map: Arc::clone(&property_map),
            component: Some(component),
            owner,
            access: access.clone(),
            animation,
            time_mapper,
        });
    }
}

fn semantic_row_id(
    target: SelectionTarget,
    section_id: &str,
    owner: SemanticPropertyOwner,
    property_key: &str,
    component: Option<PropertyComponent>,
) -> String {
    format!(
        "semantic:{}:{section_id}:{}:{property_key}{}",
        selection_id(target),
        semantic_owner_id(owner),
        component.map(component_suffix).unwrap_or_default(),
    )
}

fn selection_id(target: SelectionTarget) -> String {
    match target {
        SelectionTarget::Node(id) => format!("node:{id}"),
        SelectionTarget::Clip(id) => format!("clip:{id}"),
        SelectionTarget::Track(id) => format!("track:{id}"),
        SelectionTarget::Composition(id) => format!("composition:{id}"),
    }
}

fn semantic_owner_id(owner: SemanticPropertyOwner) -> String {
    match owner {
        SemanticPropertyOwner::DirectClip(id) => format!("clip:{id}"),
        SemanticPropertyOwner::ExactNode(id) => format!("node:{id}"),
        SemanticPropertyOwner::SemanticContainer(container) => {
            format!(
                "container:{}",
                selection_id(selection_for_container(container))
            )
        }
    }
}

fn component_label(component: PropertyComponent) -> &'static str {
    match component {
        PropertyComponent::Scalar => "value",
        PropertyComponent::X => "x",
        PropertyComponent::Y => "y",
        PropertyComponent::Z => "z",
        PropertyComponent::W => "w",
    }
}

fn component_suffix(component: PropertyComponent) -> &'static str {
    match component {
        PropertyComponent::Scalar => "",
        PropertyComponent::X => ".x",
        PropertyComponent::Y => ".y",
        PropertyComponent::Z => ".z",
        PropertyComponent::W => ".w",
    }
}

pub fn selection_for_container(container: NodeContainer) -> SelectionTarget {
    match container {
        NodeContainer::Clip(id) => SelectionTarget::Clip(id),
        NodeContainer::Track(id) => SelectionTarget::Track(id),
        NodeContainer::Composition(id) => SelectionTarget::Composition(id),
    }
}

pub fn container_for_selection(target: SelectionTarget) -> Option<NodeContainer> {
    match target {
        SelectionTarget::Node(_) => None,
        SelectionTarget::Clip(id) => Some(NodeContainer::Clip(id)),
        SelectionTarget::Track(id) => Some(NodeContainer::Track(id)),
        SelectionTarget::Composition(id) => Some(NodeContainer::Composition(id)),
    }
}

fn time_mapper_for_semantic_owner(project: &Project, owner: SemanticPropertyOwner) -> TimeMapper {
    match owner {
        SemanticPropertyOwner::DirectClip(id) => {
            time_mapper_for_owner(project, PropertyOwner::Clip(id))
        }
        SemanticPropertyOwner::ExactNode(id) => {
            time_mapper_for_owner(project, PropertyOwner::Node(id))
        }
        SemanticPropertyOwner::SemanticContainer(NodeContainer::Clip(id)) => {
            time_mapper_for_owner(project, PropertyOwner::Clip(id))
        }
        SemanticPropertyOwner::SemanticContainer(
            NodeContainer::Track(_) | NodeContainer::Composition(_),
        ) => TimeMapper::identity(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
    use library::model::frame::color::Color;
    use library::model::project::{NodeGraphBundle, PortAddress, PortOwner};
    use library::model::property::{PropertyUiType, PropertyValue, Vec3, Vec4};
    use library::model::{Clip, Composition};
    use library::plugin::PluginManager;
    use ordered_float::OrderedFloat;
    use std::sync::RwLock;

    fn number(value: f64) -> PropertyValue {
        PropertyValue::Number(OrderedFloat(value))
    }

    fn vec3(x: f64, y: f64, z: f64) -> PropertyValue {
        PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
        })
    }

    fn vec4(x: f64, y: f64, z: f64, w: f64) -> PropertyValue {
        PropertyValue::Vec4(Vec4 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
            w: OrderedFloat(w),
        })
    }

    #[test]
    fn exact_node_projection_keeps_definition_order_then_sorted_persisted_extras() {
        let definitions = vec![
            PropertyDefinition::new(
                "later",
                PropertyUiType::vec3(""),
                "Later",
                vec3(0.0, 0.0, 0.0),
            ),
            PropertyDefinition::new(
                "first",
                PropertyUiType::Float {
                    min: -10.0,
                    max: 10.0,
                    step: 0.1,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "First",
                number(0.0),
            ),
        ];
        let mut properties = PropertyMap::new();
        properties.set("first".to_string(), Property::constant(number(1.0)));
        properties.set("later".to_string(), Property::constant(vec3(2.0, 3.0, 4.0)));
        properties.set(
            "alpha_expression".to_string(),
            Property::expression("value".to_string(), vec4(5.0, 6.0, 7.0, 8.0)),
        );
        properties.set(
            "zeta_text".to_string(),
            Property::constant(PropertyValue::String("not plotted".to_string())),
        );
        let mut node_json =
            serde_json::to_value(Node::new_merge("exact")).expect("test Node serializes");
        node_json["properties"] =
            serde_json::to_value(properties).expect("test properties serialize");
        let node: Node = serde_json::from_value(node_json).expect("test Node deserializes");
        let mut project = Project::new("exact projection");
        project.add_node(node.clone());

        let projection = GraphPropertyProjection::exact_node(&project, &node, &definitions);
        let ids = projection
            .rows()
            .map(|row| row.stable_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
                "node:later.x",
                "node:later.y",
                "node:later.z",
                "node:first",
                "node:alpha_expression.x",
                "node:alpha_expression.y",
                "node:alpha_expression.z",
                "node:alpha_expression.w",
                "node:zeta_text",
            ]
        );
        assert!(!projection.rows().last().expect("text row").is_plottable());
        assert_eq!(projection.target, SelectionTarget::Node(node.id));
    }

    #[test]
    fn clip_stack_projection_preserves_sections_and_non_numeric_source_without_mutation() {
        let plugins = Arc::new(PluginManager::default());
        let factory = ProjectManager::new(
            Arc::new(RwLock::new(Project::new("factory"))),
            Arc::clone(&plugins),
        );
        let source = factory
            .create_generator_node(
                GeneratorNodeRequest::Solid {
                    color: Color {
                        r: 20,
                        g: 40,
                        b: 60,
                        a: 255,
                    },
                },
                160,
                90,
                160,
                90,
            )
            .expect("Solid generator factory succeeds");
        let source_id = source.id;
        let mut project = Project::new("semantic projection");
        let (composition, track) = Composition::new("main", 160, 90, 30.0, 2.0);
        let track_id = track.id;
        project.add_track(track).expect("track insertion succeeds");
        project
            .add_composition(composition)
            .expect("composition insertion succeeds");
        let clip = Clip::new("solid", 0.0, 2.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project
            .attach_clip_to_track(track_id, clip_id)
            .expect("clip attachment succeeds");
        project
            .insert_node_graph(
                NodeContainer::Clip(clip_id),
                NodeGraphBundle::new(vec![source], Vec::new(), Some(source_id)),
            )
            .expect("source graph insertion succeeds");
        let shared = Arc::new(RwLock::new(project));
        let manager = ProjectManager::new(Arc::clone(&shared), plugins);
        let before = shared.read().expect("project read").clone();
        let stack = manager
            .semantic_container_property_stack(NodeContainer::Clip(clip_id))
            .expect("Clip stack resolves");
        let projection =
            GraphPropertyProjection::semantic(&shared.read().expect("project read"), &stack);

        assert_eq!(projection.target, SelectionTarget::Clip(clip_id));
        assert_eq!(
            projection
                .sections
                .iter()
                .map(|section| section.stable_id.as_str())
                .collect::<Vec<_>>(),
            stack
                .sections()
                .iter()
                .map(|section| section.stable_id())
                .collect::<Vec<_>>()
        );
        let color = projection
            .rows()
            .find(|row| row.property_key == "color")
            .expect("Solid color row is preserved");
        assert_eq!(color.component, None);
        assert!(!color.is_plottable());
        assert!(!projection
            .rows()
            .any(|row| row.component == Some(PropertyComponent::W)));
        assert_eq!(*shared.read().expect("project read"), before);
    }

    #[test]
    fn wired_and_read_only_rows_fail_closed() {
        let mut map = PropertyMap::new();
        map.set("amount".to_string(), Property::constant(number(1.0)));
        let map = Arc::new(map);
        let source = PortAddress::new(PortOwner::Node(Uuid::new_v4()), "result");
        let mut wired = Vec::new();
        append_rows(
            &mut wired,
            SelectionTarget::Clip(Uuid::new_v4()),
            "wired",
            "amount",
            "Amount",
            None,
            Property::constant(number(1.0)),
            Arc::clone(&map),
            SemanticPropertyOwner::ExactNode(Uuid::new_v4()),
            SemanticPropertyAccess::Wired { source },
            SemanticAnimationSupport::Evaluator,
            TimeMapper::identity(),
        );
        let wired = wired.first().expect("Wired row is projected");
        assert!(!wired.is_plottable());
        assert!(!wired.is_editable());

        let mut read_only = Vec::new();
        append_rows(
            &mut read_only,
            SelectionTarget::Track(Uuid::new_v4()),
            "read-only",
            "amount",
            "Amount",
            None,
            Property::constant(number(1.0)),
            map,
            SemanticPropertyOwner::SemanticContainer(NodeContainer::Track(Uuid::new_v4())),
            SemanticPropertyAccess::ReadOnly {
                reason: "ambiguous owner".to_string(),
                related_nodes: Vec::new(),
            },
            SemanticAnimationSupport::Evaluator,
            TimeMapper::identity(),
        );
        let read_only = read_only.first().expect("read-only row is projected");
        assert!(read_only.is_plottable());
        assert!(!read_only.is_editable());
    }
}
