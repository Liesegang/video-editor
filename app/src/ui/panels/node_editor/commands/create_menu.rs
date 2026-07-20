use crate::ui::widgets::searchable_context_menu::SearchableItem;
use library::model::{Node, Project};
use library::plugin::{
    PluginManager, DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECTOR_APPLY_OPERATION,
    EFFECTOR_CATEGORY, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, STYLE_APPLY_OPERATION,
    STYLE_CATEGORY,
};
use uuid::Uuid;

use crate::ui::panels::node_editor::node_can_splice_connection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum NodeCreateRequest {
    Text,
    Solid,
    Shape,
    SkSL,
    TimeModulo,
    Style(String),
    Effector(String),
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
            Self::TimeModulo => "time_modulo",
            Self::Style(_) => "style",
            Self::Effector(_) => "effector",
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
        node_create_menu_item(
            "Time Modulo",
            "Timing / Values",
            ["time", "modulo", "loop", "remainder", "value", "number"],
            "node_editor.menu.create.time_modulo",
            NodeCreateRequest::TimeModulo,
        ),
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
    ];

    let mut styles = plugin_manager.get_available_styles();
    styles.sort();
    items.extend(styles.into_iter().filter_map(|component_id| {
        let qa_id = match component_id.as_str() {
            "fill" | "stroke" => format!("node_editor.menu.create.{component_id}"),
            _ => format!("node_editor.menu.create.style:{component_id}"),
        };
        plugin_operation_menu_item(
            plugin_manager,
            PluginOperationMenuItemSpec {
                descriptor_category: STYLE_CATEGORY,
                operation: STYLE_APPLY_OPERATION,
                component_id: component_id.clone(),
                menu_category: "Shape Operations / Styles".to_string(),
                display_kind: "Style",
                qa_id,
                request: NodeCreateRequest::Style(component_id),
                extra_keywords: vec!["shape".to_string(), "image".to_string()],
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
        NodeCreateRequest::Style(component_id) => {
            plugin_manager.create_style_operation_node(component_id)
        }
        NodeCreateRequest::Effector(component_id) => {
            plugin_manager.create_effector_operation_node(component_id)
        }
        NodeCreateRequest::Decorator(component_id) => {
            plugin_manager.create_decorator_operation_node(component_id)
        }
        NodeCreateRequest::Effect(effect_id) => {
            plugin_manager.create_effect_operation_node(effect_id)
        }
        NodeCreateRequest::Merge => return Some(Node::new_merge("Merge")),
        NodeCreateRequest::TimeModulo => return Some(Node::new_time_modulo("Time Modulo")),
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
            log::warn!("Cannot prepare operation Node for wire insertion: {error}");
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
