use crate::ui::widgets::searchable_context_menu::SearchableItem;
use library::model::{Node, Project, ValueContent};
use library::plugin::{
    PluginManager, DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECTOR_APPLY_OPERATION,
    EFFECTOR_CATEGORY, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, IMAGE_OPACITY_STYLE_COMPONENT_ID,
    IMAGE_TRANSFORM_COMPONENT_ID, PATH_EFFECT_APPLY_OPERATION, PATH_EFFECT_CATEGORY,
    SHAPE_TRANSFORM_COMPONENT_ID, STYLE_APPLY_OPERATION, STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION,
    TRANSFORM_CATEGORY,
};
use uuid::Uuid;

use crate::ui::panels::node_editor::node_can_splice_connection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum NodeCreateRequest {
    Text,
    Solid,
    Shape,
    SkSL,
    ShapeTransform,
    ImageTransform,
    Value(ValueContent),
    Style(String),
    Effector(String),
    PathEffect(String),
    Decorator(String),
    Effect(String),
    Merge,
    Clip,
    Track,
    Composition,
}

impl NodeCreateRequest {
    pub(in crate::ui::panels::node_editor) fn qa_kind(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Solid => "solid",
            Self::Shape => "shape",
            Self::SkSL => "sksl",
            Self::ShapeTransform => "transform",
            Self::ImageTransform => "image_transform",
            Self::Value(value) => value.operation_key(),
            Self::Style(_) => "style",
            Self::Effector(_) => "effector",
            Self::PathEffect(_) => "path_effect",
            Self::Decorator(_) => "decorator",
            Self::Effect(_) => "effect",
            Self::Merge => "merge",
            Self::Clip => "clip",
            Self::Track => "track",
            Self::Composition => "composition",
        }
    }
}

fn node_create_menu_item(
    label: impl Into<String>,
    category: impl Into<String>,
    keywords: impl IntoIterator<Item = impl Into<String>>,
    qa_id: impl Into<String>,
    value: NodeCreateRequest,
) -> SearchableItem<NodeCreateRequest> {
    let mut item = SearchableItem::new(label, value);
    item.category = Some(category.into());
    item.keywords = keywords.into_iter().map(Into::into).collect();
    item.qa_id = Some(qa_id.into());
    item.qa_metadata = Some(serde_json::json!({
        "action": "create",
        "kind": item.value.qa_kind(),
    }));
    item
}

struct PluginOperationMenuItemSpec {
    descriptor_category: &'static str,
    operation: &'static str,
    component_id: String,
    menu_category: String,
    display_kind: &'static str,
    qa_id: String,
    request: NodeCreateRequest,
    extra_keywords: Vec<String>,
}

fn plugin_operation_menu_item(
    plugin_manager: &PluginManager,
    spec: PluginOperationMenuItemSpec,
) -> Option<SearchableItem<NodeCreateRequest>> {
    let PluginOperationMenuItemSpec {
        descriptor_category,
        operation,
        component_id,
        menu_category,
        display_kind,
        qa_id,
        request,
        extra_keywords,
    } = spec;
    let descriptor = match plugin_manager.operation_descriptor(
        descriptor_category,
        &component_id,
        operation,
    ) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            log::warn!(
                "Cannot expose {descriptor_category} operation {component_id} in the Node Editor: {error}"
            );
            return None;
        }
    };
    let label = descriptor.label().to_string();
    let mut keywords = vec![
        display_kind.to_lowercase(),
        label.clone(),
        component_id.clone(),
        descriptor.category().to_string(),
        descriptor.operation().to_string(),
    ];
    keywords.extend(extra_keywords);
    let mut item = node_create_menu_item(
        format!("{display_kind} · {label}"),
        menu_category,
        keywords,
        qa_id,
        request,
    );
    item.qa_metadata = Some(serde_json::json!({
        "action": "create",
        "kind": item.value.qa_kind(),
        "component_id": component_id,
        "operation_category": descriptor.category(),
        "operation": descriptor.operation(),
        "label": descriptor.label(),
    }));
    Some(item)
}

fn transform_operation_menu_item(
    plugin_manager: &PluginManager,
    component_id: &'static str,
    menu_category: &'static str,
    qa_id: &'static str,
    request: NodeCreateRequest,
    content_keyword: &'static str,
) -> Option<SearchableItem<NodeCreateRequest>> {
    let descriptor = match plugin_manager.operation_descriptor(
        TRANSFORM_CATEGORY,
        component_id,
        TRANSFORM_APPLY_OPERATION,
    ) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            log::warn!(
                "Cannot expose Transform operation {component_id} in the Node Editor: {error}"
            );
            return None;
        }
    };
    let mut item = node_create_menu_item(
        descriptor.label(),
        menu_category,
        [
            "root",
            "placement",
            "position",
            "rotation",
            "scale",
            "anchor",
            content_keyword,
            descriptor.category(),
            descriptor.component_id(),
            descriptor.operation(),
        ],
        qa_id,
        request,
    );
    item.qa_metadata = Some(serde_json::json!({
        "action": "create",
        "kind": item.value.qa_kind(),
        "component_id": descriptor.component_id(),
        "operation_category": descriptor.category(),
        "operation": descriptor.operation(),
        "label": descriptor.label(),
    }));
    Some(item)
}

pub(in crate::ui::panels::node_editor) fn node_create_menu_items(
    plugin_manager: &PluginManager,
) -> Vec<SearchableItem<NodeCreateRequest>> {
    let mut items = vec![
        node_create_menu_item(
            "Text",
            "Generators",
            ["title", "caption", "shape"],
            "node_editor.menu.create.text",
            NodeCreateRequest::Text,
        ),
        node_create_menu_item(
            "Solid Color",
            "Generators",
            ["solid", "color", "image"],
            "node_editor.menu.create.solid",
            NodeCreateRequest::Solid,
        ),
        node_create_menu_item(
            "Shape (Rectangle)",
            "Generators",
            ["shape", "rectangle", "path"],
            "node_editor.menu.create.shape",
            NodeCreateRequest::Shape,
        ),
        node_create_menu_item(
            "SkSL Shader",
            "Generators",
            ["sksl", "shader", "procedural", "image"],
            "node_editor.menu.create.sksl",
            NodeCreateRequest::SkSL,
        ),
    ];
    items.extend(ValueContent::ALL.into_iter().map(|value| {
        let keywords: &[&str] = match value {
            ValueContent::Fmod => &["fmod", "modulo", "remainder", "loop", "value", "number"],
            ValueContent::Add => &["add", "plus", "sum", "value", "number"],
            ValueContent::Subtract => &["subtract", "minus", "difference", "value", "number"],
            ValueContent::Multiply => &["multiply", "times", "product", "value", "number"],
            ValueContent::Divide => &["divide", "quotient", "ratio", "value", "number"],
        };
        node_create_menu_item(
            value.label(),
            "Math / Values",
            keywords.iter().copied(),
            format!("node_editor.menu.create.value:{}", value.operation_key()),
            NodeCreateRequest::Value(value),
        )
    }));
    items.extend([
        node_create_menu_item(
            "Merge",
            "Compositing",
            ["merge", "composite", "blend", "layers"],
            "node_editor.menu.create.merge",
            NodeCreateRequest::Merge,
        ),
        node_create_menu_item(
            "Container (Clip)",
            "Containers",
            ["clip", "container", "timeline"],
            "node_editor.menu.create.clip",
            NodeCreateRequest::Clip,
        ),
        node_create_menu_item(
            "Container (Track)",
            "Containers",
            ["track", "container", "timeline"],
            "node_editor.menu.create.track",
            NodeCreateRequest::Track,
        ),
        node_create_menu_item(
            "Container (Composition)",
            "Containers",
            ["composition", "container", "nested"],
            "node_editor.menu.create.composition",
            NodeCreateRequest::Composition,
        ),
    ]);

    for transform in [
        transform_operation_menu_item(
            plugin_manager,
            SHAPE_TRANSFORM_COMPONENT_ID,
            "Shape Operations / Transform",
            "node_editor.menu.create.transform",
            NodeCreateRequest::ShapeTransform,
            "shape",
        ),
        transform_operation_menu_item(
            plugin_manager,
            IMAGE_TRANSFORM_COMPONENT_ID,
            "Image Operations / Transform",
            "node_editor.menu.create.image_transform",
            NodeCreateRequest::ImageTransform,
            "image",
        ),
    ]
    .into_iter()
    .flatten()
    {
        items.push(transform);
    }

    let mut styles = plugin_manager.get_available_styles();
    styles.sort();
    items.extend(styles.into_iter().filter_map(|component_id| {
        let qa_id = match component_id.as_str() {
            "fill" | "stroke" => format!("node_editor.menu.create.{component_id}"),
            IMAGE_OPACITY_STYLE_COMPONENT_ID => "node_editor.menu.create.image_opacity".to_string(),
            _ => format!("node_editor.menu.create.style:{component_id}"),
        };
        let is_image_style = component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID;
        plugin_operation_menu_item(
            plugin_manager,
            PluginOperationMenuItemSpec {
                descriptor_category: STYLE_CATEGORY,
                operation: STYLE_APPLY_OPERATION,
                component_id: component_id.clone(),
                menu_category: if is_image_style {
                    "Image Operations / Styles".to_string()
                } else {
                    "Shape Operations / Styles".to_string()
                },
                display_kind: "Style",
                qa_id,
                request: NodeCreateRequest::Style(component_id),
                extra_keywords: vec![if is_image_style { "image" } else { "shape" }.to_string()],
            },
        )
    }));

    let mut effectors = plugin_manager.get_available_effectors();
    effectors.sort();
    items.extend(effectors.into_iter().filter_map(|component_id| {
        plugin_operation_menu_item(
            plugin_manager,
            PluginOperationMenuItemSpec {
                descriptor_category: EFFECTOR_CATEGORY,
                operation: EFFECTOR_APPLY_OPERATION,
                component_id: component_id.clone(),
                menu_category: "Shape Operations / Effectors".to_string(),
                display_kind: "Effector",
                qa_id: format!("node_editor.menu.create.effector:{component_id}"),
                request: NodeCreateRequest::Effector(component_id),
                extra_keywords: vec!["shape".to_string()],
            },
        )
    }));

    let mut path_effects = plugin_manager.get_available_path_effects();
    path_effects.sort();
    items.extend(path_effects.into_iter().filter_map(|component_id| {
        let mut item = plugin_operation_menu_item(
            plugin_manager,
            PluginOperationMenuItemSpec {
                descriptor_category: PATH_EFFECT_CATEGORY,
                operation: PATH_EFFECT_APPLY_OPERATION,
                component_id: component_id.clone(),
                menu_category: "Shape Operations / Path Effects".to_string(),
                display_kind: "Path Effect",
                qa_id: format!("node_editor.menu.create.path_effect:{component_id}"),
                request: NodeCreateRequest::PathEffect(component_id),
                extra_keywords: vec!["shape".to_string(), "path geometry".to_string()],
            },
        )?;
        if let Some(metadata) = item
            .qa_metadata
            .as_mut()
            .and_then(serde_json::Value::as_object_mut)
        {
            metadata.insert(
                "shape_geometry".to_string(),
                serde_json::Value::String("path_only".to_string()),
            );
            metadata.insert(
                "unsupported_shape_geometry".to_string(),
                serde_json::Value::String("text".to_string()),
            );
        }
        Some(item)
    }));

    let mut decorators = plugin_manager.get_available_decorators();
    decorators.sort();
    items.extend(decorators.into_iter().filter_map(|component_id| {
        plugin_operation_menu_item(
            plugin_manager,
            PluginOperationMenuItemSpec {
                descriptor_category: DECORATOR_CATEGORY,
                operation: DECORATOR_APPLY_OPERATION,
                component_id: component_id.clone(),
                menu_category: "Shape Operations / Decorators".to_string(),
                display_kind: "Decorator",
                qa_id: format!("node_editor.menu.create.decorator:{component_id}"),
                request: NodeCreateRequest::Decorator(component_id),
                extra_keywords: vec!["shape".to_string(), "ensemble".to_string()],
            },
        )
    }));

    let mut effects = plugin_manager.get_available_effects();
    effects.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    items.extend(
        effects
            .into_iter()
            .filter_map(|(effect_id, effect_name, effect_category)| {
                plugin_operation_menu_item(
                    plugin_manager,
                    PluginOperationMenuItemSpec {
                        descriptor_category: EFFECT_CATEGORY,
                        operation: EFFECT_APPLY_OPERATION,
                        component_id: effect_id.clone(),
                        menu_category: format!("Image Effects / {effect_category}"),
                        display_kind: "Effect",
                        qa_id: format!("node_editor.menu.create.effect:{effect_id}"),
                        request: NodeCreateRequest::Effect(effect_id),
                        extra_keywords: vec![effect_name, effect_category, "image".to_string()],
                    },
                )
            }),
    );
    items
}

pub(in crate::ui::panels::node_editor) fn create_operation_node_for_request(
    request: &NodeCreateRequest,
    plugin_manager: &PluginManager,
) -> Option<Node> {
    let result = match request {
        NodeCreateRequest::ShapeTransform => plugin_manager.create_shape_transform_operation_node(),
        NodeCreateRequest::ImageTransform => plugin_manager.create_image_transform_operation_node(),
        NodeCreateRequest::Style(component_id) => {
            plugin_manager.create_style_operation_node(component_id)
        }
        NodeCreateRequest::Effector(component_id) => {
            plugin_manager.create_effector_operation_node(component_id)
        }
        NodeCreateRequest::PathEffect(component_id) => {
            plugin_manager.create_path_effect_operation_node(component_id)
        }
        NodeCreateRequest::Decorator(component_id) => {
            plugin_manager.create_decorator_operation_node(component_id)
        }
        NodeCreateRequest::Effect(effect_id) => {
            plugin_manager.create_effect_operation_node(effect_id)
        }
        NodeCreateRequest::Merge => return Some(Node::new_merge("Merge")),
        NodeCreateRequest::Value(value) => return Some(Node::new_value(value.label(), *value)),
        NodeCreateRequest::Text
        | NodeCreateRequest::Solid
        | NodeCreateRequest::Shape
        | NodeCreateRequest::SkSL
        | NodeCreateRequest::Clip
        | NodeCreateRequest::Track
        | NodeCreateRequest::Composition => return None,
    };
    match result {
        Ok(node) => Some(node),
        Err(error) => {
            log::warn!("Cannot prepare operation Node for authoring: {error}");
            None
        }
    }
}

pub(in crate::ui::panels::node_editor) fn wire_splice_menu_items(
    project: &Project,
    connection_id: Uuid,
    plugin_manager: &PluginManager,
) -> Vec<SearchableItem<NodeCreateRequest>> {
    node_create_menu_items(plugin_manager)
        .into_iter()
        .filter_map(|mut item| {
            let node = create_operation_node_for_request(&item.value, plugin_manager)?;
            let node_id = node.id;
            let mut probe = project.clone();
            probe.add_node(node);
            if !node_can_splice_connection(&probe, connection_id, node_id) {
                return None;
            }
            let suffix = item
                .qa_id
                .as_deref()
                .and_then(|id| id.strip_prefix("node_editor.menu.create."))
                .unwrap_or(item.value.qa_kind());
            item.qa_id = Some(format!("node_editor.wire_menu.operation.{suffix}"));
            item.qa_metadata = Some(serde_json::json!({
                "action": "splice",
                "connection_id": connection_id,
                "kind": item.value.qa_kind(),
            }));
            Some(item)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::wire::insert_node_on_connection;
    use super::*;
    use crate::ui::panels::node_editor::test_fixture::fixture;
    use library::model::project::{
        PortAddress, PortDataType, PortDirection, PortOwner, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT,
        MERGE_IMAGES_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
    };
    use library::model::{BlendMode, NodeContainer, NodeContent};

    #[test]
    fn transform_menu_items_have_distinct_factories_and_typed_ports() {
        let plugins = PluginManager::default();
        let items = node_create_menu_items(&plugins);

        for (request, label, category, qa_id, component_id, input, output) in [
            (
                NodeCreateRequest::ShapeTransform,
                "Shape Transform",
                "Shape Operations / Transform",
                "node_editor.menu.create.transform",
                SHAPE_TRANSFORM_COMPONENT_ID,
                (SHAPE_INPUT_PORT, PortDataType::Shape),
                (SHAPE_OUTPUT_PORT, PortDataType::Shape),
            ),
            (
                NodeCreateRequest::ImageTransform,
                "Image Transform",
                "Image Operations / Transform",
                "node_editor.menu.create.image_transform",
                IMAGE_TRANSFORM_COMPONENT_ID,
                (IMAGE_INPUT_PORT, PortDataType::Image),
                (IMAGE_OUTPUT_PORT, PortDataType::Image),
            ),
        ] {
            let item = items
                .iter()
                .find(|item| item.value == request)
                .unwrap_or_else(|| panic!("{label} is missing from the Add menu"));
            assert_eq!(item.label, label);
            assert_eq!(item.category.as_deref(), Some(category));
            assert_eq!(item.qa_id.as_deref(), Some(qa_id));
            assert_eq!(
                item.qa_metadata.as_ref().unwrap()["component_id"],
                component_id
            );
            assert_eq!(
                item.qa_metadata.as_ref().unwrap()["operation_category"],
                TRANSFORM_CATEGORY
            );
            assert_eq!(
                item.qa_metadata.as_ref().unwrap()["operation"],
                TRANSFORM_APPLY_OPERATION
            );

            let node = create_operation_node_for_request(&request, &plugins)
                .unwrap_or_else(|| panic!("{label} factory is unavailable"));
            assert_eq!(
                node.properties()
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<std::collections::BTreeSet<_>>(),
                ["anchor", "position", "rotation", "scale"]
                    .into_iter()
                    .collect()
            );
            assert!(node.properties().get("opacity").is_none());
            let NodeContent::PluginOperation(operation) = node.content() else {
                panic!("{label} factory did not create a PluginOperation")
            };
            assert_eq!(operation.category, TRANSFORM_CATEGORY);
            assert_eq!(operation.component_id, component_id);
            assert_eq!(operation.operation, TRANSFORM_APPLY_OPERATION);
            for (key, direction, data_type) in [
                (TIME_PORT, PortDirection::Input, PortDataType::Number),
                (input.0, PortDirection::Input, input.1),
                (output.0, PortDirection::Output, output.1),
            ] {
                assert!(operation.declared_ports.iter().any(|port| {
                    port.key == key && port.direction == direction && port.data_type == data_type
                }));
            }
            for property in ["position", "rotation", "scale", "anchor"] {
                assert!(operation
                    .declared_ports
                    .iter()
                    .any(|port| port.key == format!("property:{property}")));
            }
        }

        let image_matches = crate::ui::widgets::searchable_context_menu::filter_searchable_items(
            &items,
            "image root placement",
        );
        assert!(image_matches
            .iter()
            .any(|index| items[*index].value == NodeCreateRequest::ImageTransform));
    }

    #[test]
    fn path_effect_menu_uses_four_descriptor_backed_shape_operations() {
        let plugins = PluginManager::default();
        let items = node_create_menu_items(&plugins);
        let expected = [
            ("corner", ["radius"].as_slice()),
            ("dash", ["intervals", "phase"].as_slice()),
            (
                "discrete",
                ["deviation", "seed", "segment_length"].as_slice(),
            ),
            ("trim", ["end", "start"].as_slice()),
        ];
        for (component_id, property_names) in expected {
            let request = NodeCreateRequest::PathEffect(component_id.to_string());
            let item = items
                .iter()
                .find(|item| item.value == request)
                .unwrap_or_else(|| panic!("missing Path Effect menu item {component_id}"));
            assert_eq!(
                item.category.as_deref(),
                Some("Shape Operations / Path Effects")
            );
            let expected_qa_id = format!("node_editor.menu.create.path_effect:{component_id}");
            assert_eq!(item.qa_id.as_deref(), Some(expected_qa_id.as_str()));
            let metadata = item.qa_metadata.as_ref().unwrap();
            assert_eq!(metadata["operation_category"], PATH_EFFECT_CATEGORY);
            assert_eq!(metadata["operation"], PATH_EFFECT_APPLY_OPERATION);
            assert_eq!(metadata["component_id"], component_id);
            assert_eq!(metadata["shape_geometry"], "path_only");
            assert_eq!(metadata["unsupported_shape_geometry"], "text");

            let node = create_operation_node_for_request(&request, &plugins)
                .expect("Path Effect request must use its descriptor factory");
            let actual_properties = node
                .properties()
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(actual_properties, property_names.iter().copied().collect());
            let NodeContent::PluginOperation(operation) = node.content() else {
                panic!("Path Effect factory returned a non-operation Node")
            };
            assert_eq!(operation.category, PATH_EFFECT_CATEGORY);
            assert_eq!(operation.component_id, component_id);
            assert_eq!(operation.operation, PATH_EFFECT_APPLY_OPERATION);
            for (key, direction, data_type) in [
                (TIME_PORT, PortDirection::Input, PortDataType::Number),
                (SHAPE_INPUT_PORT, PortDirection::Input, PortDataType::Shape),
                (
                    SHAPE_OUTPUT_PORT,
                    PortDirection::Output,
                    PortDataType::Shape,
                ),
            ] {
                assert!(operation.declared_ports.iter().any(|port| {
                    port.key == key && port.direction == direction && port.data_type == data_type
                }));
            }
        }
    }

    #[test]
    fn image_opacity_style_is_discoverable_in_the_typed_image_menu() {
        let plugins = PluginManager::default();
        let items = node_create_menu_items(&plugins);
        let request = NodeCreateRequest::Style(IMAGE_OPACITY_STYLE_COMPONENT_ID.to_string());
        let item = items
            .iter()
            .find(|item| item.value == request)
            .expect("Image Opacity Style is missing from the Add menu");
        assert_eq!(item.label, "Style · Image Opacity");
        assert_eq!(item.category.as_deref(), Some("Image Operations / Styles"));
        assert_eq!(
            item.qa_id.as_deref(),
            Some("node_editor.menu.create.image_opacity")
        );
        assert_eq!(
            item.qa_metadata.as_ref().unwrap()["component_id"],
            IMAGE_OPACITY_STYLE_COMPONENT_ID
        );

        let node = create_operation_node_for_request(&request, &plugins)
            .expect("Image Opacity Style factory is unavailable");
        assert_eq!(
            node.properties()
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["opacity"]
        );
        let NodeContent::PluginOperation(operation) = node.content() else {
            panic!("Image Opacity factory did not create a PluginOperation")
        };
        for (key, direction, data_type) in [
            (IMAGE_INPUT_PORT, PortDirection::Input, PortDataType::Image),
            (
                IMAGE_OUTPUT_PORT,
                PortDirection::Output,
                PortDataType::Image,
            ),
        ] {
            assert!(operation.declared_ports.iter().any(|port| {
                port.key == key && port.direction == direction && port.data_type == data_type
            }));
        }
    }

    #[test]
    fn image_transform_wire_menu_and_insert_preserve_the_image_wire() {
        let plugins = PluginManager::default();
        let (mut project, composition_id, _, clip_id, source_id, merge_id) = fixture();
        let connection_id = project
            .connections
            .iter()
            .find(|connection| {
                connection.from == PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT)
                    && connection.to
                        == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
            })
            .expect("fixture Image wire is missing")
            .id;
        project
            .set_connection_blend_mode(connection_id, BlendMode::Multiply)
            .expect("fixture Image wire accepts blend metadata");
        let original = project
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .expect("fixture Image wire is missing")
            .clone();

        let wire_items = wire_splice_menu_items(&project, connection_id, &plugins);
        assert!(wire_items
            .iter()
            .any(|item| item.value == NodeCreateRequest::ImageTransform));
        assert!(!wire_items
            .iter()
            .any(|item| item.value == NodeCreateRequest::ShapeTransform));

        let image_transform =
            create_operation_node_for_request(&NodeCreateRequest::ImageTransform, &plugins)
                .expect("Image Transform wire request uses its operation factory");
        let transform_id = image_transform.id;
        assert!(insert_node_on_connection(
            &mut project,
            connection_id,
            image_transform,
            egui::pos2(610.0, 440.0),
            composition_id,
        ));
        assert_eq!(
            project.find_node_container(transform_id),
            Some(NodeContainer::Clip(clip_id))
        );

        let downstream = project
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .expect("splice replaced the downstream wire identity");
        assert_eq!(
            downstream.from,
            PortAddress::new(PortOwner::Node(transform_id), IMAGE_OUTPUT_PORT)
        );
        assert_eq!(downstream.to, original.to);
        assert_eq!(downstream.order, original.order);
        assert_eq!(downstream.blend_mode, original.blend_mode);
        assert!(project.connections.iter().any(|connection| {
            connection.from == original.from
                && connection.to
                    == PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT)
        }));
    }
}
