//! Searchable creation catalog for Module graphs.
//!
//! Timeline containers are deliberately not representable in this request
//! type, so a Module menu cannot accidentally recreate the legacy Project
//! graph hierarchy.

use library::model::{native_node_catalog, GeneratorContent, NativeNodeFactory};
use library::plugin::{
    PluginManager, DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECTOR_APPLY_OPERATION,
    EFFECTOR_CATEGORY, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, IMAGE_OPACITY_STYLE_COMPONENT_ID,
    IMAGE_TRANSFORM_COMPONENT_ID, PATH_EFFECT_APPLY_OPERATION, PATH_EFFECT_CATEGORY,
    SHAPE_TRANSFORM_COMPONENT_ID, STYLE_APPLY_OPERATION, STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION,
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
    qa_kind: &str,
    display_kind: Option<&str>,
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
    if let Some(display_kind) = display_kind {
        keywords.push(display_kind.to_ascii_lowercase());
    }
    keywords.extend(extra_keywords.iter().map(|keyword| (*keyword).to_string()));
    let label = display_kind.map_or_else(
        || descriptor.label().to_string(),
        |display_kind| format!("{display_kind} · {}", descriptor.label()),
    );
    let mut item = item(
        label,
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
        "action": "create",
        "kind": qa_kind,
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
                "action": "create",
                "kind": "native",
                "document_kind": "module_definition",
                "catalog_id": descriptor.catalog_id(),
                "label": descriptor.label(),
                "category": descriptor.category(),
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
        "image_transform",
        None,
        &["image", "position", "rotation", "scale", "anchor"],
    ) {
        items.push(item);
    }

    if let Some(item) = operation_item(
        plugins,
        TRANSFORM_CATEGORY,
        SHAPE_TRANSFORM_COMPONENT_ID.to_string(),
        TRANSFORM_APPLY_OPERATION,
        "Shape Operations / Transform",
        "node_editor.menu.create.transform",
        "transform",
        None,
        &["shape", "position", "rotation", "scale", "anchor"],
    ) {
        items.push(item);
    }

    let mut styles = plugins.get_available_styles();
    styles.sort();
    items.extend(styles.into_iter().filter_map(|component_id| {
        let is_image_style = component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID;
        let qa_id = match component_id.as_str() {
            "fill" | "stroke" => format!("node_editor.menu.create.{component_id}"),
            IMAGE_OPACITY_STYLE_COMPONENT_ID => "node_editor.menu.create.image_opacity".to_string(),
            _ => format!("node_editor.menu.create.style:{component_id}"),
        };
        operation_item(
            plugins,
            STYLE_CATEGORY,
            component_id,
            STYLE_APPLY_OPERATION,
            if is_image_style {
                "Image Operations / Styles"
            } else {
                "Shape Operations / Styles"
            },
            qa_id,
            "style",
            Some("Style"),
            if is_image_style {
                &["image", "opacity", "style"]
            } else {
                &["shape", "style", "fill", "stroke"]
            },
        )
    }));

    let mut effectors = plugins.get_available_effectors();
    effectors.sort();
    items.extend(effectors.into_iter().filter_map(|component_id| {
        operation_item(
            plugins,
            EFFECTOR_CATEGORY,
            component_id.clone(),
            EFFECTOR_APPLY_OPERATION,
            "Shape Operations / Effectors",
            format!("node_editor.menu.create.effector:{component_id}"),
            "effector",
            Some("Effector"),
            &["shape", "ensemble", "animation"],
        )
    }));

    let mut path_effects = plugins.get_available_path_effects();
    path_effects.sort();
    items.extend(path_effects.into_iter().filter_map(|component_id| {
        let mut item = operation_item(
            plugins,
            PATH_EFFECT_CATEGORY,
            component_id.clone(),
            PATH_EFFECT_APPLY_OPERATION,
            "Shape Operations / Path Effects",
            format!("node_editor.menu.create.path_effect:{component_id}"),
            "path_effect",
            Some("Path Effect"),
            &["shape", "path", "geometry"],
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

    let mut decorators = plugins.get_available_decorators();
    decorators.sort();
    items.extend(decorators.into_iter().filter_map(|component_id| {
        operation_item(
            plugins,
            DECORATOR_CATEGORY,
            component_id.clone(),
            DECORATOR_APPLY_OPERATION,
            "Shape Operations / Decorators",
            format!("node_editor.menu.create.decorator:{component_id}"),
            "decorator",
            Some("Decorator"),
            &["shape", "ensemble", "backplate"],
        )
    }));

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
                    "effect",
                    Some("Effect"),
                    &["effect", "image"],
                )
            }),
    );
    items
}

const fn module_runtime_supports_native(factory: NativeNodeFactory) -> bool {
    matches!(
        factory,
        NativeNodeFactory::Generator(
            GeneratorContent::Text
                | GeneratorContent::Solid
                | GeneratorContent::Shape
                | GeneratorContent::SkSL
        ) | NativeNodeFactory::Value(_)
            | NativeNodeFactory::Data(_)
            | NativeNodeFactory::Merge
            | NativeNodeFactory::SoundMerge
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

    #[test]
    fn menu_exposes_shape_generators_supported_by_the_module_runtime() {
        let items = module_node_menu_items(&PluginManager::default());
        for (catalog_id, label, qa_id) in [
            ("native.text", "Text", "node_editor.menu.create.text"),
            ("native.shape", "Shape", "node_editor.menu.create.shape"),
        ] {
            let entry = items
                .iter()
                .find(|item| item.value == ModuleNodeCreateRequest::Native(catalog_id.to_string()))
                .unwrap_or_else(|| panic!("{label} is missing from the Module create menu"));
            assert_eq!(entry.label, label);
            assert_eq!(entry.qa_id.as_deref(), Some(qa_id));
        }
    }

    #[test]
    fn menu_exposes_the_sound_merge_supported_by_the_module_runtime() {
        let items = module_node_menu_items(&PluginManager::default());
        let entry = items
            .iter()
            .find(|item| {
                item.value == ModuleNodeCreateRequest::Native("native.sound.merge".to_string())
            })
            .expect("Audio Mix is missing from the Module create menu");
        assert_eq!(entry.label, "Audio Mix");
        assert_eq!(
            entry.qa_id.as_deref(),
            Some("node_editor.menu.create.sound_merge")
        );
    }

    #[test]
    fn menu_restores_the_production_shape_operation_catalog() {
        let items = module_node_menu_items(&PluginManager::default());
        for qa_id in [
            "node_editor.menu.create.transform",
            "node_editor.menu.create.fill",
            "node_editor.menu.create.stroke",
            "node_editor.menu.create.effector:transform",
            "node_editor.menu.create.decorator:backplate",
            "node_editor.menu.create.path_effect:trim",
        ] {
            assert!(
                items
                    .iter()
                    .any(|item| item.qa_id.as_deref() == Some(qa_id)),
                "production Module catalog is missing {qa_id}"
            );
        }
        let trim = items
            .iter()
            .find(|item| item.qa_id.as_deref() == Some("node_editor.menu.create.path_effect:trim"))
            .expect("Trim Path catalog item");
        assert_eq!(trim.qa_metadata.as_ref().unwrap()["kind"], "path_effect");
        assert_eq!(
            trim.qa_metadata.as_ref().unwrap()["shape_geometry"],
            "path_only"
        );
    }
}
