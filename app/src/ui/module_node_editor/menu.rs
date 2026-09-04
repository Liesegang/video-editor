//! Searchable creation catalog for Module graphs.
//!
//! Timeline containers are deliberately not representable in this request
//! type, so a Module menu cannot accidentally recreate the legacy Project
//! graph hierarchy.

use library::model::{native_node_catalog, GeneratorContent, NativeNodeFactory};
use library::plugin::{
    PluginManager, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, IMAGE_OPACITY_STYLE_COMPONENT_ID,
    IMAGE_TRANSFORM_COMPONENT_ID, STYLE_APPLY_OPERATION, STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION,
    TRANSFORM_CATEGORY,
};

use crate::ui::widgets::searchable_context_menu::SearchableItem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ModuleNodeCreateRequest {
    Native(String),
    PluginOperation {
        category: String,
        component_id: String,
        operation: String,
    },
}

fn item(
    label: impl Into<String>,
    category: impl Into<String>,
    keywords: impl IntoIterator<Item = impl Into<String>>,
    qa_id: impl Into<String>,
    value: ModuleNodeCreateRequest,
) -> SearchableItem<ModuleNodeCreateRequest> {
    let mut item = SearchableItem::new(label, value);
    item.category = Some(category.into());
    item.keywords = keywords.into_iter().map(Into::into).collect();
    item.qa_id = Some(qa_id.into());
    item.qa_metadata = Some(serde_json::json!({
        "action": "create_module_node",
        "document_kind": "module_definition",
    }));
    item
}

fn operation_item(
    plugins: &PluginManager,
    category: &str,
    component_id: String,
    operation: &str,
    menu_category: impl Into<String>,
    qa_id: impl Into<String>,
    extra_keywords: &[&str],
) -> Option<SearchableItem<ModuleNodeCreateRequest>> {
    let descriptor = plugins
        .operation_descriptor(category, &component_id, operation)
        .ok()?;
    let mut keywords = vec![
        descriptor.label().to_string(),
        component_id.clone(),
        category.to_string(),
        operation.to_string(),
    ];
    keywords.extend(extra_keywords.iter().map(|keyword| (*keyword).to_string()));
    let mut item = item(
        descriptor.label(),
        menu_category,
        keywords,
        qa_id,
        ModuleNodeCreateRequest::PluginOperation {
            category: category.to_string(),
            component_id,
            operation: operation.to_string(),
        },
    );
    item.qa_metadata = Some(serde_json::json!({
        "action": "create_module_node",
        "document_kind": "module_definition",
        "operation_category": descriptor.category(),
        "component_id": descriptor.component_id(),
        "operation": descriptor.operation(),
        "label": descriptor.label(),
    }));
    Some(item)
}

pub(super) fn module_node_menu_items(
    plugins: &PluginManager,
) -> Vec<SearchableItem<ModuleNodeCreateRequest>> {
    let mut items = native_node_catalog()
        .iter()
        .filter(|descriptor| module_runtime_supports_native(descriptor.factory()))
        .map(|descriptor| {
            let mut keywords = descriptor
                .keywords()
                .iter()
                .map(|keyword| (*keyword).to_string())
                .collect::<Vec<_>>();
            keywords.push(descriptor.catalog_id().to_string());
            let mut item = item(
                descriptor.label(),
                descriptor.category(),
                keywords,
                descriptor.qa_id(),
                ModuleNodeCreateRequest::Native(descriptor.catalog_id().to_string()),
            );
            item.qa_metadata = Some(serde_json::json!({
                "action": "create_module_node",
                "document_kind": "module_definition",
                "catalog_id": descriptor.catalog_id(),
                "runtime_status": descriptor.runtime_status().key(),
            }));
            item
        })
        .collect::<Vec<_>>();

    if let Some(item) = operation_item(
        plugins,
        TRANSFORM_CATEGORY,
        IMAGE_TRANSFORM_COMPONENT_ID.to_string(),
        TRANSFORM_APPLY_OPERATION,
        "Image Operations / Transform",
        "node_editor.menu.create.image_transform",
        &["image", "position", "rotation", "scale", "anchor"],
    ) {
        items.push(item);
    }

    if let Some(item) = operation_item(
        plugins,
        STYLE_CATEGORY,
        IMAGE_OPACITY_STYLE_COMPONENT_ID.to_string(),
        STYLE_APPLY_OPERATION,
        "Image Operations / Style",
        "node_editor.menu.create.image_opacity",
        &["image", "opacity", "style"],
    ) {
        items.push(item);
    }

    let mut effects = plugins.get_available_effects();
    effects.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    items.extend(
        effects
            .into_iter()
            .filter_map(|(component_id, _name, effect_category)| {
                operation_item(
                    plugins,
                    EFFECT_CATEGORY,
                    component_id.clone(),
                    EFFECT_APPLY_OPERATION,
                    format!("Image Effects / {effect_category}"),
                    format!("node_editor.menu.create.effect:{component_id}"),
                    &["effect", "image"],
                )
            }),
    );
    items
}

const fn module_runtime_supports_native(factory: NativeNodeFactory) -> bool {
    matches!(
        factory,
        NativeNodeFactory::Generator(GeneratorContent::Solid | GeneratorContent::SkSL)
            | NativeNodeFactory::Value(_)
            | NativeNodeFactory::Data(_)
            | NativeNodeFactory::Merge
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_request_type_has_no_timeline_container_variant() {
        let items = module_node_menu_items(&PluginManager::default());
        assert!(!items.is_empty());
        assert!(items.iter().all(|item| matches!(
            item.value,
            ModuleNodeCreateRequest::Native(_) | ModuleNodeCreateRequest::PluginOperation { .. }
        )));
    }

    #[test]
    fn menu_exposes_only_native_nodes_supported_by_the_module_runtime() {
        let items = module_node_menu_items(&PluginManager::default());
        for item in items {
            if let ModuleNodeCreateRequest::Native(catalog_id) = item.value {
                let descriptor = library::model::native_node_descriptor(&catalog_id)
                    .expect("menu catalog entry");
                assert!(module_runtime_supports_native(descriptor.factory()));
            }
        }
    }
}
