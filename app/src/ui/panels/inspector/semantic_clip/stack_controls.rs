//! Typed stack controls for the Clip semantic facade.

use std::collections::HashMap;

use egui::{RichText, Ui};
use egui_phosphor::regular as icons;
use library::editor::project_service::{
    SemanticContainerPropertyStack, SemanticDecoratorStack, SemanticEffectStack,
    SemanticPropertyGroup, SemanticStyleStack,
};
use library::model::project::{
    NodeContainer, PortDataType, PortDirection, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT,
    SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY,
    IMAGE_TRANSFORM_COMPONENT_ID, SHAPE_TRANSFORM_COMPONENT_ID, STYLE_APPLY_OPERATION,
    STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
};
use library::{EditorService, LibraryError};
use uuid::Uuid;

use crate::action::HistoryManager;
use crate::ui::widgets::searchable_context_menu::{
    searchable_menu_button, show_searchable_items_with_qa, SearchableItem,
};

mod qa;

use qa::{register_action_button, register_stack_row, render_query_error, StackQaAction};

#[derive(Clone, Debug, PartialEq, Eq)]
enum StackAction {
    EnsureTransform,
    AppendEffect(String),
    ReorderEffects(Vec<Uuid>),
    RemoveEffect(Uuid),
    AppendStyle {
        component_id: String,
        after: Option<Uuid>,
    },
    ReorderStyles(Vec<Uuid>),
    RemoveStyle(Uuid),
    AppendDecoratorForStyle {
        component_id: String,
        style_anchor_id: Uuid,
    },
    AppendDecoratorAfter {
        component_id: String,
        decorator_anchor_id: Uuid,
    },
    ReorderDecoratorsForStyle {
        style_anchor_id: Uuid,
        requested: Vec<Uuid>,
    },
    RemoveDecoratorForStyle {
        style_anchor_id: Uuid,
        decorator_id: Uuid,
    },
}

pub(super) fn render(
    ui: &mut Ui,
    clip_id: Uuid,
    property_stack: &SemanticContainerPropertyStack,
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    needs_refresh: &mut bool,
) {
    let owner = NodeContainer::Clip(clip_id);
    let effect_stack = project_service.semantic_container_effect_stack(owner);
    let style_stack = project_service.semantic_container_style_stack(owner);
    let decorator_stack = project_service.semantic_container_decorator_stack(owner);
    let response = egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            let mut pending = None;
            ui.horizontal(|ui| {
                ui.strong("Clip processing stacks");
                ui.label(RichText::new("authoritative graph").small().weak());
            });
            ui.separator();

            pending = pending
                .or_else(|| render_transform_control(ui, clip_id, property_stack, project_service));
            pending = pending.or_else(|| match &style_stack {
                Ok(stack) => render_style_stack(ui, clip_id, stack, project_service),
                Err(error) => {
                    render_query_error(ui, clip_id, "style", error);
                    None
                }
            });
            pending = pending.or_else(|| match &decorator_stack {
                Ok(stack) => render_decorator_stack(ui, clip_id, stack, project_service),
                Err(error) => {
                    render_query_error(ui, clip_id, "decorator", error);
                    None
                }
            });
            pending = pending.or_else(|| match &effect_stack {
                Ok(stack) => render_effect_stack(ui, clip_id, stack, project_service),
                Err(error) => {
                    render_query_error(ui, clip_id, "effect", error);
                    None
                }
            });
            pending
        });
    crate::qa::register_component_with_metadata(
        format!("inspector.semantic.stacks:{clip_id}"),
        "inspector_semantic_stacks",
        response.response.rect,
        true,
        Some(serde_json::json!({
            "clip_id": clip_id,
            "mutates_wires_directly": false,
            "selection_identity": "clip",
            "history_policy": "one_snapshot_after_success",
        })),
    );

    let error_id = ui.make_persistent_id(("semantic_stack_action_error", clip_id));
    if let Some(action) = response.inner {
        match execute_with_history(project_service, history_manager, owner, action) {
            Ok(()) => {
                ui.data_mut(|data| data.remove_temp::<String>(error_id));
                *needs_refresh = true;
            }
            Err(error) => {
                log::error!("Semantic Clip stack action failed: {error}");
                ui.data_mut(|data| data.insert_temp(error_id, error.to_string()));
            }
        }
    }
    if let Some(message) = ui.data(|data| data.get_temp::<String>(error_id)) {
        let response = ui.colored_label(ui.visuals().error_fg_color, &message);
        crate::qa::register_component_with_metadata(
            format!("inspector.semantic.stacks:{clip_id}.action_error"),
            "inspector_semantic_action_error",
            response.rect,
            true,
            Some(serde_json::json!({
                "clip_id": clip_id,
                "message": message,
                "history_committed": false,
                "selection_changed": false,
                "fail_closed": true,
            })),
        );
    }
}

fn render_transform_control(
    ui: &mut Ui,
    clip_id: Uuid,
    stack: &SemanticContainerPropertyStack,
    project_service: &EditorService,
) -> Option<StackAction> {
    let has_transform = stack.sections().iter().any(|section| {
        section.group() == SemanticPropertyGroup::Transform && section.node_id().is_some()
    });
    stack_heading(ui, "Transform", "typed Shape/Image root placement");
    if has_transform {
        ui.label(RichText::new("Root Transform is present").small().weak());
        return None;
    }
    add_menu(
        ui,
        clip_id,
        "transform",
        "root",
        "Add Transform",
        transform_catalog(project_service, clip_id),
    )
}

fn render_effect_stack(
    ui: &mut Ui,
    clip_id: Uuid,
    stack: &SemanticEffectStack,
    project_service: &EditorService,
) -> Option<StackAction> {
    stack_heading(
        ui,
        "Effects",
        "final post-Merge chain · upstream → downstream",
    );
    let ids = stack.node_ids();
    let labels = node_labels(project_service, ids);
    let mut action = None;
    for (index, node_id) in ids.iter().copied().enumerate() {
        let row = ui.horizontal(|ui| {
            ui.label(format!("{} · {}", index + 1, label_for(&labels, node_id)));
            if action.is_none() {
                action = order_buttons(
                    ui,
                    clip_id,
                    "effect",
                    node_id,
                    None,
                    index,
                    ids,
                    StackAction::ReorderEffects,
                    StackAction::RemoveEffect(node_id),
                    "main-flow wire reconnect only",
                );
            }
        });
        register_stack_row(
            row.response.rect,
            clip_id,
            "effect",
            node_id,
            index,
            "upstream_to_downstream",
            None,
        );
    }
    action.or_else(|| {
        add_menu(
            ui,
            clip_id,
            "effect",
            "tail",
            "Add Effect at end",
            effect_catalog(project_service, clip_id),
        )
    })
}

fn render_style_stack(
    ui: &mut Ui,
    clip_id: Uuid,
    stack: &SemanticStyleStack,
    project_service: &EditorService,
) -> Option<StackAction> {
    stack_heading(ui, "Styles", "frontmost → backmost Shape rasterization");
    let ids = stack.node_ids();
    let labels = node_labels(project_service, ids);
    let mut action = None;
    for (index, node_id) in ids.iter().copied().enumerate() {
        let row = ui.horizontal(|ui| {
            ui.label(format!("{} · {}", index + 1, label_for(&labels, node_id)));
            if action.is_none() {
                action = order_buttons(
                    ui,
                    clip_id,
                    "style",
                    node_id,
                    None,
                    index,
                    ids,
                    StackAction::ReorderStyles,
                    StackAction::RemoveStyle(node_id),
                    "reorder existing Merge wires",
                );
            }
        });
        register_stack_row(
            row.response.rect,
            clip_id,
            "style",
            node_id,
            index,
            "frontmost_to_backmost",
            None,
        );
        if action.is_none() {
            let items = style_catalog(project_service, clip_id, Some(node_id));
            action = add_menu(
                ui,
                clip_id,
                "style",
                &format!("after:{node_id}"),
                "Add Style after",
                items,
            );
        }
    }
    if ids.is_empty() && action.is_none() {
        action = add_menu(
            ui,
            clip_id,
            "style",
            "root",
            "Add Style",
            style_catalog(project_service, clip_id, None),
        );
    }
    action
}

fn render_decorator_stack(
    ui: &mut Ui,
    clip_id: Uuid,
    stack: &SemanticDecoratorStack,
    project_service: &EditorService,
) -> Option<StackAction> {
    stack_heading(ui, "Decorators", "anchored Shape branches · root → leaf");
    if stack.chains().is_empty() {
        ui.label(
            RichText::new("Add a Style before adding a Decorator")
                .small()
                .weak(),
        );
        return None;
    }
    let labels = node_labels(project_service, stack.node_ids());
    let mut action = None;
    for chain in stack.chains() {
        for style_anchor_id in chain.style_anchor_ids().iter().copied() {
            ui.label(
                RichText::new(format!("Style {}", short_id(style_anchor_id)))
                    .small()
                    .strong(),
            );
            for (index, node_id) in chain.node_ids().iter().copied().enumerate() {
                let row = ui.horizontal(|ui| {
                    ui.label(format!("{} · {}", index + 1, label_for(&labels, node_id)));
                    if action.is_none() {
                        action = decorator_order_buttons(
                            ui,
                            clip_id,
                            style_anchor_id,
                            node_id,
                            index,
                            chain.node_ids(),
                        );
                    }
                });
                register_stack_row(
                    row.response.rect,
                    clip_id,
                    "decorator",
                    node_id,
                    index,
                    "root_to_leaf",
                    Some(style_anchor_id),
                );
                if action.is_none() {
                    action = add_menu(
                        ui,
                        clip_id,
                        "decorator",
                        &format!("after:{node_id}"),
                        "Add Decorator after",
                        decorator_catalog(
                            project_service,
                            clip_id,
                            DecoratorAnchor::Decorator(node_id),
                        ),
                    );
                }
            }
            if chain.node_ids().is_empty() && action.is_none() {
                action = add_menu(
                    ui,
                    clip_id,
                    "decorator",
                    &format!("style:{style_anchor_id}"),
                    "Add Decorator",
                    decorator_catalog(
                        project_service,
                        clip_id,
                        DecoratorAnchor::Style(style_anchor_id),
                    ),
                );
            }
        }
    }
    action
}

fn stack_heading(ui: &mut Ui, label: &str, description: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.strong(label);
        ui.label(RichText::new(description).small().weak());
    });
}

#[allow(
    clippy::too_many_arguments,
    reason = "one compact stack row exposes identity-preserving move and remove commands with QA metadata"
)]
fn order_buttons(
    ui: &mut Ui,
    clip_id: Uuid,
    kind: &str,
    node_id: Uuid,
    style_anchor_id: Option<Uuid>,
    index: usize,
    ids: &[Uuid],
    reorder: impl Fn(Vec<Uuid>) -> StackAction,
    remove: StackAction,
    mutation_semantics: &str,
) -> Option<StackAction> {
    let mut action = None;
    let up = ui
        .add_enabled(index > 0, egui::Button::new(icons::ARROW_UP).small())
        .on_hover_text("Move earlier");
    register_action_button(
        &up,
        clip_id,
        kind,
        node_id,
        style_anchor_id,
        StackQaAction::MoveUp,
        mutation_semantics,
    );
    if up.clicked() {
        let mut requested = ids.to_vec();
        requested.swap(index, index - 1);
        action = Some(reorder(requested));
    }
    let down = ui
        .add_enabled(
            index + 1 < ids.len(),
            egui::Button::new(icons::ARROW_DOWN).small(),
        )
        .on_hover_text("Move later");
    register_action_button(
        &down,
        clip_id,
        kind,
        node_id,
        style_anchor_id,
        StackQaAction::MoveDown,
        mutation_semantics,
    );
    if down.clicked() {
        let mut requested = ids.to_vec();
        requested.swap(index, index + 1);
        action = Some(reorder(requested));
    }
    let remove_button = ui
        .small_button(icons::TRASH)
        .on_hover_text(format!("Remove {kind}"));
    register_action_button(
        &remove_button,
        clip_id,
        kind,
        node_id,
        style_anchor_id,
        StackQaAction::Remove,
        "typed semantic remove",
    );
    if remove_button.clicked() {
        action = Some(remove);
    }
    action
}

fn decorator_order_buttons(
    ui: &mut Ui,
    clip_id: Uuid,
    style_anchor_id: Uuid,
    node_id: Uuid,
    index: usize,
    ids: &[Uuid],
) -> Option<StackAction> {
    order_buttons(
        ui,
        clip_id,
        "decorator",
        node_id,
        Some(style_anchor_id),
        index,
        ids,
        |requested| StackAction::ReorderDecoratorsForStyle {
            style_anchor_id,
            requested,
        },
        StackAction::RemoveDecoratorForStyle {
            style_anchor_id,
            decorator_id: node_id,
        },
        "reconnect anchored Shape chain only",
    )
}

fn add_menu(
    ui: &mut Ui,
    clip_id: Uuid,
    kind: &str,
    scope: &str,
    label: &str,
    items: Vec<SearchableItem<StackAction>>,
) -> Option<StackAction> {
    let menu_id = format!("inspector.semantic.menu.{kind}:{clip_id}:{scope}");
    let response = ui
        .add_enabled_ui(!items.is_empty(), |ui| {
            searchable_menu_button(ui, format!("{} {label}", icons::PLUS), |ui| {
                ui.set_min_width(290.0);
                ui.set_min_height(240.0_f32.min(ui.available_height().max(0.0)));
                show_searchable_items_with_qa(
                    ui,
                    &menu_id,
                    Some(&format!("{menu_id}.query")),
                    &items,
                )
            })
        })
        .inner;
    crate::qa::register_component_with_metadata(
        format!("inspector.semantic.{kind}:{clip_id}.add:{scope}"),
        "inspector_semantic_add",
        response.response.rect,
        response.response.enabled(),
        Some(serde_json::json!({
            "clip_id": clip_id,
            "stack": kind,
            "action": "open_add_menu",
            "descriptor_count": items.len(),
            "browse_mode": "hierarchical_accordion",
            "search_mode": "flat",
        })),
    );
    response.inner.flatten()
}

fn effect_catalog(
    project_service: &EditorService,
    clip_id: Uuid,
) -> Vec<SearchableItem<StackAction>> {
    let plugins = project_service.get_plugin_manager();
    let mut effects = plugins.get_available_effects();
    effects.sort_by(|left, right| left.2.cmp(&right.2).then_with(|| left.1.cmp(&right.1)));
    effects
        .into_iter()
        .filter_map(|(component_id, name, category)| {
            let mut item = descriptor_item(
                plugins.as_ref(),
                DescriptorItem {
                    descriptor_category: EFFECT_CATEGORY,
                    operation: EFFECT_APPLY_OPERATION,
                    component_id: component_id.clone(),
                    menu_category: format!("Effect / {category}"),
                    keywords: vec![name, category, "image".to_string()],
                    qa_id: format!("inspector.semantic.menu.effect:{clip_id}.item:{component_id}"),
                    action: StackAction::AppendEffect(component_id),
                    input: (IMAGE_INPUT_PORT, PortDataType::Image),
                    output: (IMAGE_OUTPUT_PORT, PortDataType::Image),
                },
            )?;
            extend_qa_metadata(
                &mut item,
                serde_json::json!({
                    "insertion": "final_post_merge_effect_tail",
                    "existing_effects_preserved": true,
                }),
            );
            Some(item)
        })
        .collect()
}

fn style_catalog(
    project_service: &EditorService,
    clip_id: Uuid,
    after: Option<Uuid>,
) -> Vec<SearchableItem<StackAction>> {
    let plugins = project_service.get_plugin_manager();
    let mut styles = plugins.get_available_styles();
    styles.sort();
    styles
        .into_iter()
        .filter_map(|component_id| {
            let mut item = descriptor_item(
                plugins.as_ref(),
                DescriptorItem {
                    descriptor_category: STYLE_CATEGORY,
                    operation: STYLE_APPLY_OPERATION,
                    component_id: component_id.clone(),
                    menu_category: "Style / Shape".to_string(),
                    keywords: vec!["appearance".to_string(), "shape".to_string()],
                    qa_id: format!("inspector.semantic.menu.style:{clip_id}.item:{component_id}"),
                    action: StackAction::AppendStyle {
                        component_id,
                        after,
                    },
                    input: (SHAPE_INPUT_PORT, PortDataType::Shape),
                    output: (IMAGE_OUTPUT_PORT, PortDataType::Image),
                },
            )?;
            extend_qa_metadata(
                &mut item,
                serde_json::json!({
                    "anchor_style_id": after,
                    "insertion": if after.is_some() { "after_style" } else { "unambiguous_shape_source" },
                }),
            );
            Some(item)
        })
        .collect()
}

#[derive(Clone, Copy)]
enum DecoratorAnchor {
    Style(Uuid),
    Decorator(Uuid),
}

fn decorator_catalog(
    project_service: &EditorService,
    clip_id: Uuid,
    anchor: DecoratorAnchor,
) -> Vec<SearchableItem<StackAction>> {
    let plugins = project_service.get_plugin_manager();
    let mut decorators = plugins.get_available_decorators();
    decorators.sort();
    decorators
        .into_iter()
        .filter_map(|component_id| {
            let action = match anchor {
                DecoratorAnchor::Style(style_anchor_id) => StackAction::AppendDecoratorForStyle {
                    component_id: component_id.clone(),
                    style_anchor_id,
                },
                DecoratorAnchor::Decorator(decorator_anchor_id) => {
                    StackAction::AppendDecoratorAfter {
                        component_id: component_id.clone(),
                        decorator_anchor_id,
                    }
                }
            };
            let mut item = descriptor_item(
                plugins.as_ref(),
                DescriptorItem {
                    descriptor_category: DECORATOR_CATEGORY,
                    operation: DECORATOR_APPLY_OPERATION,
                    component_id: component_id.clone(),
                    menu_category: "Decorator / Shape".to_string(),
                    keywords: vec!["shape".to_string(), "geometry".to_string()],
                    qa_id: format!(
                        "inspector.semantic.menu.decorator:{clip_id}.item:{component_id}"
                    ),
                    action,
                    input: (SHAPE_INPUT_PORT, PortDataType::Shape),
                    output: (SHAPE_OUTPUT_PORT, PortDataType::Shape),
                },
            )?;
            let (anchor_kind, anchor_id) = match anchor {
                DecoratorAnchor::Style(id) => ("style", id),
                DecoratorAnchor::Decorator(id) => ("decorator", id),
            };
            extend_qa_metadata(
                &mut item,
                serde_json::json!({
                    "anchor_kind": anchor_kind,
                    "anchor_id": anchor_id,
                }),
            );
            Some(item)
        })
        .collect()
}

fn transform_catalog(
    project_service: &EditorService,
    clip_id: Uuid,
) -> Vec<SearchableItem<StackAction>> {
    let plugins = project_service.get_plugin_manager();
    let descriptors = [SHAPE_TRANSFORM_COMPONENT_ID, IMAGE_TRANSFORM_COMPONENT_ID]
        .into_iter()
        .filter_map(|component_id| {
            plugins
                .operation_descriptor(TRANSFORM_CATEGORY, component_id, TRANSFORM_APPLY_OPERATION)
                .ok()
        })
        .collect::<Vec<_>>();
    if descriptors.is_empty() {
        return Vec::new();
    }
    let mut item = SearchableItem::new(
        "Root Transform · typed automatically",
        StackAction::EnsureTransform,
    );
    item.category = Some("Transform / Root placement".to_string());
    item.keywords = descriptors
        .iter()
        .flat_map(|descriptor| {
            [
                descriptor.label().to_string(),
                descriptor.component_id().to_string(),
            ]
        })
        .chain([
            "position".to_string(),
            "rotation".to_string(),
            "scale".to_string(),
        ])
        .collect();
    item.qa_id = Some(format!(
        "inspector.semantic.menu.transform:{clip_id}.item:root"
    ));
    item.qa_metadata = Some(serde_json::json!({
        "clip_id": clip_id,
        "action": "ensure_transform",
        "typed_by_graph": true,
        "components": descriptors
            .iter()
            .map(|descriptor| descriptor.component_id())
            .collect::<Vec<_>>(),
    }));
    vec![item]
}

struct DescriptorItem {
    descriptor_category: &'static str,
    operation: &'static str,
    component_id: String,
    menu_category: String,
    keywords: Vec<String>,
    qa_id: String,
    action: StackAction,
    input: (&'static str, PortDataType),
    output: (&'static str, PortDataType),
}

fn descriptor_item(
    plugins: &library::plugin::PluginManager,
    spec: DescriptorItem,
) -> Option<SearchableItem<StackAction>> {
    let descriptor = plugins
        .operation_descriptor(spec.descriptor_category, &spec.component_id, spec.operation)
        .ok()?;
    let supports = descriptor.declared_ports().iter().any(|port| {
        port.key == spec.input.0
            && port.direction == PortDirection::Input
            && port.data_type == spec.input.1
    }) && descriptor.declared_ports().iter().any(|port| {
        port.key == spec.output.0
            && port.direction == PortDirection::Output
            && port.data_type == spec.output.1
    });
    if !supports {
        return None;
    }
    let mut item = SearchableItem::new(descriptor.label(), spec.action);
    item.category = Some(spec.menu_category);
    item.keywords = spec.keywords;
    item.keywords.extend([
        spec.component_id.clone(),
        descriptor.category().to_string(),
        descriptor.operation().to_string(),
    ]);
    item.qa_id = Some(spec.qa_id);
    item.qa_metadata = Some(serde_json::json!({
        "action": "append",
        "component_id": spec.component_id,
        "category": descriptor.category(),
        "operation": descriptor.operation(),
        "label": descriptor.label(),
        "typed_input": format!("{:?}", spec.input.1).to_lowercase(),
        "typed_output": format!("{:?}", spec.output.1).to_lowercase(),
    }));
    Some(item)
}

fn extend_qa_metadata(item: &mut SearchableItem<StackAction>, extra: serde_json::Value) {
    let Some(target) = item
        .qa_metadata
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    let Some(extra) = extra.as_object() else {
        return;
    };
    target.extend(extra.clone());
}

fn execute_with_history(
    project_service: &EditorService,
    history_manager: &mut HistoryManager,
    owner: NodeContainer,
    action: StackAction,
) -> Result<(), LibraryError> {
    execute(project_service, owner, action)?;
    let snapshot = project_service
        .get_project()
        .read()
        .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?
        .clone();
    history_manager.push_project_state(snapshot);
    Ok(())
}

fn execute(
    project_service: &EditorService,
    owner: NodeContainer,
    action: StackAction,
) -> Result<(), LibraryError> {
    match action {
        StackAction::EnsureTransform => project_service
            .ensure_semantic_container_transform(owner)
            .map(|_| ()),
        StackAction::AppendEffect(component_id) => project_service
            .append_semantic_container_effect(owner, &component_id)
            .map(|_| ()),
        StackAction::ReorderEffects(requested) => {
            project_service.reorder_semantic_container_effects(owner, &requested)
        }
        StackAction::RemoveEffect(node_id) => {
            project_service.remove_semantic_container_effect(owner, node_id)
        }
        StackAction::AppendStyle {
            component_id,
            after,
        } => after.map_or_else(
            || {
                project_service
                    .append_semantic_container_style(owner, &component_id)
                    .map(|_| ())
            },
            |anchor| {
                project_service
                    .append_semantic_container_style_after(owner, &component_id, Some(anchor))
                    .map(|_| ())
            },
        ),
        StackAction::ReorderStyles(requested) => {
            project_service.reorder_semantic_container_styles(owner, &requested)
        }
        StackAction::RemoveStyle(node_id) => {
            project_service.remove_semantic_container_style(owner, node_id)
        }
        StackAction::AppendDecoratorForStyle {
            component_id,
            style_anchor_id,
        } => project_service
            .append_semantic_container_decorator_for_style(owner, &component_id, style_anchor_id)
            .map(|_| ()),
        StackAction::AppendDecoratorAfter {
            component_id,
            decorator_anchor_id,
        } => project_service
            .append_semantic_container_decorator_after(owner, &component_id, decorator_anchor_id)
            .map(|_| ()),
        StackAction::ReorderDecoratorsForStyle {
            style_anchor_id,
            requested,
        } => project_service.reorder_semantic_container_decorators_for_style(
            owner,
            style_anchor_id,
            &requested,
        ),
        StackAction::RemoveDecoratorForStyle {
            style_anchor_id,
            decorator_id,
        } => project_service.remove_semantic_container_decorator_for_style(
            owner,
            style_anchor_id,
            decorator_id,
        ),
    }
}

fn node_labels(project_service: &EditorService, ids: &[Uuid]) -> HashMap<Uuid, String> {
    project_service
        .get_project()
        .read()
        .map(|project| {
            ids.iter()
                .filter_map(|node_id| {
                    project
                        .get_node(*node_id)
                        .map(|node| (*node_id, node.name.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn label_for(labels: &HashMap<Uuid, String>, node_id: Uuid) -> String {
    labels
        .get(&node_id)
        .cloned()
        .unwrap_or_else(|| format!("Unavailable {}", short_id(node_id)))
}

fn short_id(id: Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

#[cfg(test)]
mod tests;
