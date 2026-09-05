use std::collections::{HashMap, HashSet};

use egui_phosphor::regular as icons;
use library::editor::ModuleNodeRequest;
use library::model::authoring::{
    AttachmentProcessor, ModuleDefinitionSharing, SourceRef, TimelineId, TransitionId,
    TransitionMediaType,
};
use library::model::frame::color::Color;
use library::model::{native_node_descriptor, GeneratorContent, NativeNodeFactory};

use super::*;

/// Render the explicitly opened Module document in the existing docked Node
/// Editor. Timeline containers and ordinary items are intentionally never
/// inferred as documents.
pub fn node_editor_panel(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) {
    let Some(NodeEditorDocument::ModuleDefinition {
        definition_id,
        host,
    }) = state.node_editor.active_document.clone()
    else {
        let response = ui.centered_and_justified(|ui| {
            ui.weak("Open a Node Clip, Effect, or Transition to edit its Module logic.")
        });
        crate::qa::register_component_with_metadata(
            "node_editor.empty_document",
            "node_editor_empty_document",
            response.response.rect,
            false,
            Some(serde_json::json!({
                "reason": "no_explicit_module_document",
                "timeline_graph_expansion": false,
            })),
        );
        return;
    };

    let instance_id = host.module_instance_id();
    let Some(instance) = project.module_instances.get(&instance_id) else {
        render_unavailable_document(
            ui,
            "The Module instance for this document is no longer available.",
        );
        return;
    };
    let current_definition_id = instance.definition_id;
    if current_definition_id != definition_id {
        set_active_definition(&mut state.node_editor, current_definition_id);
    }
    let Some(definition) = project.module_definitions.get(&current_definition_id) else {
        render_unavailable_document(ui, "The Module definition is no longer available.");
        return;
    };
    if !host_owns_instance(project, &host, instance_id) {
        render_unavailable_document(
            ui,
            "The Module placement for this Node document is no longer available.",
        );
        return;
    }
    render_document_breadcrumb(ui, project, definition, &host);
    if state.node_editor.fit_requested {
        let viewport = ui.available_rect_before_wrap();
        if let Some(canvas) = super::surface::fit_module_document_canvas(definition, viewport) {
            state.node_editor.canvas = canvas;
            state.node_editor.fit_requested = false;
        }
    }
    if apply_pending_layout(ui, definition, instance_id, state, service) {
        return;
    }

    let property_context = super::clock::module_property_context(
        project,
        state.active_timeline_id,
        state.timeline.current_frame,
        &host,
    );
    let canvas_size = property_context.resolution;
    let actions = show_module_document(
        ui,
        definition,
        &project.palette,
        &mut state.node_editor,
        plugins,
        property_context,
    );
    apply_module_actions(
        actions,
        definition,
        instance_id,
        canvas_size,
        state,
        service,
        plugins,
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModuleHostPresentation {
    icon: &'static str,
    label: &'static str,
    media_type: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModuleSharingPresentation {
    label: &'static str,
    kind: &'static str,
    tooltip: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransitionDefinitionScope<'a> {
    transition_id: TransitionId,
    timeline_id: TimelineId,
    timeline_name: &'a str,
    affected_placement_count: usize,
}

fn render_document_breadcrumb(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    definition: &ModuleDefinition,
    host: &ModuleEditorHost,
) {
    let host_presentation = module_host_presentation(definition, host);
    let transition_host = host.transition_id().is_some();
    let transition_scope = transition_definition_scope(project, host);
    let sharing = module_sharing_presentation(&definition.sharing, transition_host);
    let breadcrumb = ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(egui::RichText::new(host_presentation.icon).size(17.0));
        ui.weak(host_presentation.label);
        ui.weak(icons::CARET_RIGHT);
        ui.strong(&definition.name);
        ui.weak(format!("\u{b7} {}", sharing.label))
            .on_hover_text(sharing.tooltip);
        if let Some(scope) = transition_scope {
            let noun = if scope.affected_placement_count == 1 {
                "placement"
            } else {
                "placements"
            };
            ui.weak(format!(
                "\u{b7} Timeline definition \u{b7} {} {noun}",
                scope.affected_placement_count
            ))
            .on_hover_text(transition_definition_scope_tooltip(scope));
        }
    });
    crate::qa::register_component_with_metadata(
        format!("node_editor.document_breadcrumb:{}", definition.id),
        "node_editor_document_breadcrumb",
        breadcrumb.response.rect,
        true,
        Some(document_breadcrumb_metadata(
            definition,
            host,
            host_presentation,
            sharing,
            transition_scope,
        )),
    );
    ui.separator();
}

fn document_breadcrumb_metadata(
    definition: &ModuleDefinition,
    host: &ModuleEditorHost,
    host_presentation: ModuleHostPresentation,
    sharing: ModuleSharingPresentation,
    transition_scope: Option<TransitionDefinitionScope<'_>>,
) -> serde_json::Value {
    let transition_host = host.transition_id().is_some();
    serde_json::json!({
        "definition_id": definition.id,
        "definition_name": definition.name,
        "host_kind": host.kind_name(),
        "media_type": host_presentation.media_type,
        "sharing": sharing.kind,
        "edit_scope": if transition_host {
            "timeline_definition"
        } else {
            "module_instance"
        },
        "instance_edit": !transition_host,
        "transition_id": host.transition_id(),
        "captured_instance_path": host.captured_instance_path(),
        "timeline_id": transition_scope.map(|scope| scope.timeline_id),
        "affected_placement_count": transition_scope.map(|scope| scope.affected_placement_count),
    })
}

fn transition_definition_scope<'a>(
    project: &'a AuthoringProject,
    host: &ModuleEditorHost,
) -> Option<TransitionDefinitionScope<'a>> {
    let transition_id = host.transition_id()?;
    let transition = project.transitions.get(&transition_id)?;
    let timeline = project.timelines.get(&transition.timeline_id)?;
    Some(TransitionDefinitionScope {
        transition_id,
        timeline_id: timeline.id,
        timeline_name: &timeline.name,
        affected_placement_count: timeline_definition_placement_count(project, timeline.id),
    })
}

fn transition_definition_scope_tooltip(scope: TransitionDefinitionScope<'_>) -> String {
    let impact = match scope.affected_placement_count {
        0 => "currently affects no concrete placement reachable from the Project root".to_string(),
        1 => "affects 1 concrete placement".to_string(),
        count => format!("affects all {count} concrete placements"),
    };
    format!(
        "Node topology for Transition {} belongs to Timeline definition \"{}\" and {impact}. Published parameter values and keyframes can still be overridden for a nested placement.",
        scope.transition_id, scope.timeline_name
    )
}

fn timeline_definition_placement_count(
    project: &AuthoringProject,
    target_timeline_id: TimelineId,
) -> usize {
    let mut children_by_timeline = HashMap::<TimelineId, Vec<TimelineId>>::new();
    for item in project.items.values() {
        let SourceRef::Composition(instance) = &item.source else {
            continue;
        };
        let Some(owner_timeline_id) = project
            .tracks
            .get(&item.track_id)
            .map(|track| track.timeline_id)
        else {
            continue;
        };
        children_by_timeline
            .entry(owner_timeline_id)
            .or_default()
            .push(instance.timeline_id);
    }

    fn count_from(
        current: TimelineId,
        target: TimelineId,
        children_by_timeline: &HashMap<TimelineId, Vec<TimelineId>>,
        visiting: &mut HashSet<TimelineId>,
        memoized: &mut HashMap<TimelineId, usize>,
    ) -> usize {
        if current == target {
            return 1;
        }
        if let Some(count) = memoized.get(&current) {
            return *count;
        }
        if !visiting.insert(current) {
            return 0;
        }
        let count = children_by_timeline
            .get(&current)
            .into_iter()
            .flatten()
            .fold(0_usize, |count, child| {
                count.saturating_add(count_from(
                    *child,
                    target,
                    children_by_timeline,
                    visiting,
                    memoized,
                ))
            });
        visiting.remove(&current);
        memoized.insert(current, count);
        count
    }

    count_from(
        project.root_timeline_id,
        target_timeline_id,
        &children_by_timeline,
        &mut HashSet::new(),
        &mut HashMap::new(),
    )
}

fn module_host_presentation(
    definition: &ModuleDefinition,
    host: &ModuleEditorHost,
) -> ModuleHostPresentation {
    match host {
        ModuleEditorHost::Transition { .. } => match definition
            .host_contract
            .transition()
            .map(|contract| contract.media_type)
        {
            Some(TransitionMediaType::Image) => ModuleHostPresentation {
                icon: icons::ARROWS_MERGE,
                label: "Image Transition",
                media_type: Some("image"),
            },
            Some(TransitionMediaType::Audio) => ModuleHostPresentation {
                icon: icons::WAVEFORM,
                label: "Audio Transition",
                media_type: Some("audio"),
            },
            None => ModuleHostPresentation {
                icon: icons::SHARE_NETWORK,
                label: "Transition Module",
                media_type: None,
            },
        },
        ModuleEditorHost::NodeClip { .. } => ModuleHostPresentation {
            icon: icons::SHARE_NETWORK,
            label: "Node Clip",
            media_type: None,
        },
        ModuleEditorHost::Attachment { .. } => ModuleHostPresentation {
            icon: icons::MAGIC_WAND,
            label: "Effect Module",
            media_type: None,
        },
    }
}

const fn module_sharing_presentation(
    sharing: &ModuleDefinitionSharing,
    transition_timeline_scope: bool,
) -> ModuleSharingPresentation {
    match (sharing, transition_timeline_scope) {
        (ModuleDefinitionSharing::Private, true) => ModuleSharingPresentation {
            label: "Private",
            kind: "private",
            tooltip: "This private Module belongs to its Transition's Timeline definition. Topology edits are shared by every concrete placement of that Timeline.",
        },
        (ModuleDefinitionSharing::SharedLocal, true) => ModuleSharingPresentation {
            label: "Shared local",
            kind: "shared_local",
            tooltip: "This Transition uses a locally shared Definition. A content edit creates private logic for the Timeline definition, shared by all of its concrete placements.",
        },
        (ModuleDefinitionSharing::ReusableTemplate(_), true) => ModuleSharingPresentation {
            label: "Reusable",
            kind: "reusable",
            tooltip: "This Transition uses a reusable Definition. A content edit creates private logic for the Timeline definition, shared by all of its concrete placements.",
        },
        (ModuleDefinitionSharing::Private, false) => ModuleSharingPresentation {
            label: "Private",
            kind: "private",
            tooltip: "Content edits apply only to this Module instance.",
        },
        (ModuleDefinitionSharing::SharedLocal, false) => ModuleSharingPresentation {
            label: "Shared local \u{b7} instance edit",
            kind: "shared_local",
            tooltip: "This instance uses a locally shared Definition. Content edits make this instance private.",
        },
        (ModuleDefinitionSharing::ReusableTemplate(_), false) => ModuleSharingPresentation {
            label: "Reusable \u{b7} instance edit",
            kind: "reusable",
            tooltip: "This instance uses a reusable Definition. Content edits make this instance private.",
        },
    }
}

fn apply_pending_layout(
    ui: &egui::Ui,
    definition: &ModuleDefinition,
    instance_id: ModuleInstanceId,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) -> bool {
    let Some(command) = state.node_editor.pending_layout_command.take() else {
        return false;
    };
    if !command.is_node_editor_layout() {
        state.error = Some("The requested command is not a Module layout command".to_string());
        return false;
    }
    let updates =
        layout::module_layout_updates(definition, command, &state.node_editor.selected_nodes);
    if updates.is_empty() {
        state.status = if command == crate::command::CommandId::NodeEditorCleanLayoutSelection
            && state.node_editor.selected_nodes.is_empty()
        {
            "Select one or more Module nodes to clean their layout".to_string()
        } else {
            "Module layout is already clean".to_string()
        };
        return false;
    }

    match service.set_instance_module_node_presentations(instance_id, updates) {
        Ok((definition_id, _)) => {
            set_active_definition(&mut state.node_editor, definition_id);
            state.node_editor.node_drag_offsets.clear();
            state.status = "Cleaned Module node layout".to_string();
            ui.ctx().request_repaint();
            true
        }
        Err(error) => {
            state.error = Some(error.to_string());
            false
        }
    }
}

fn render_unavailable_document(ui: &mut egui::Ui, message: &str) {
    let response =
        ui.centered_and_justified(|ui| ui.colored_label(ui.visuals().error_fg_color, message));
    crate::qa::register_component_with_metadata(
        "node_editor.unavailable_document",
        "node_editor_unavailable_document",
        response.response.rect,
        false,
        Some(serde_json::json!({
            "reason": "missing_module_document",
            "timeline_graph_expansion": false,
        })),
    );
}

fn host_owns_instance(
    project: &AuthoringProject,
    host: &ModuleEditorHost,
    expected_instance_id: ModuleInstanceId,
) -> bool {
    match host {
        ModuleEditorHost::NodeClip {
            timeline_item_id, ..
        } => {
            project.items.get(timeline_item_id).and_then(|item| {
                let SourceRef::Module(invocation) = &item.source else {
                    return None;
                };
                Some(invocation.instance_id)
            }) == Some(expected_instance_id)
        }
        ModuleEditorHost::Attachment { attachment_id, .. } => {
            project
                .attachments
                .get(attachment_id)
                .and_then(|attachment| {
                    let AttachmentProcessor::Module(invocation) = &attachment.processor else {
                        return None;
                    };
                    Some(invocation.instance_id)
                })
                == Some(expected_instance_id)
        }
        ModuleEditorHost::Transition {
            transition_id,
            instance_path,
            ..
        } => {
            let owns_instance = project
                .transitions
                .get(transition_id)
                .and_then(|transition| transition.processor.module_processor())
                .map(|processor| processor.instance_id)
                == Some(expected_instance_id);
            owns_instance
                && instance_path.as_ref().is_none_or(|path| {
                    project
                        .resolve_transition_module_instance_target(path, *transition_id)
                        .is_ok_and(|target| target.module_instance_id == expected_instance_id)
                })
        }
    }
}

fn set_active_definition(
    state: &mut NodeEditorState,
    definition_id: library::model::authoring::ModuleDefinitionId,
) {
    if let Some(NodeEditorDocument::ModuleDefinition {
        definition_id: active,
        ..
    }) = state.active_document.as_mut()
    {
        *active = definition_id;
    }
}

fn apply_module_actions(
    actions: Vec<ModuleEditorAction>,
    definition: &ModuleDefinition,
    instance_id: ModuleInstanceId,
    canvas_size: (u64, u64),
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) {
    for action in actions {
        match action {
            ModuleEditorAction::MoveNodes { node_ids, delta } => {
                for node_id in node_ids {
                    *state
                        .node_editor
                        .node_drag_offsets
                        .entry(node_id)
                        .or_insert(egui::Vec2::ZERO) += delta;
                }
            }
            ModuleEditorAction::FinishMove { outcome: _ } => {
                let offsets = std::mem::take(&mut state.node_editor.node_drag_offsets);
                for (node_id, offset) in offsets {
                    let Some(node) = definition.graph.nodes.get(&node_id) else {
                        continue;
                    };
                    match service.set_instance_module_node_presentation(
                        instance_id,
                        node_id,
                        [
                            node.ui_position[0] + offset.x,
                            node.ui_position[1] + offset.y,
                        ],
                        node.ui_size,
                        node.ui_collapsed,
                    ) {
                        Ok((definition_id, _)) => {
                            set_active_definition(&mut state.node_editor, definition_id);
                        }
                        Err(error) => state.error = Some(error.to_string()),
                    }
                }
            }
            ModuleEditorAction::Connect { from, to } => {
                let order = definition
                    .graph
                    .connections
                    .iter()
                    .filter(|connection| connection.to == to)
                    .count() as i64;
                match service.connect_instance_module_ports(instance_id, from, to, order) {
                    Ok((_, definition_id, _)) => {
                        set_active_definition(&mut state.node_editor, definition_id);
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            ModuleEditorAction::Reconnect {
                connection_id,
                from,
                to,
            } => {
                match service.reconnect_instance_module_connection(
                    instance_id,
                    connection_id,
                    from,
                    to,
                ) {
                    Ok((definition_id, _)) => {
                        set_active_definition(&mut state.node_editor, definition_id);
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            ModuleEditorAction::Disconnect(connection_id) => {
                match service.disconnect_instance_module_connection(instance_id, connection_id) {
                    Ok((definition_id, _)) => {
                        set_active_definition(&mut state.node_editor, definition_id);
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            ModuleEditorAction::DeleteNodes(node_ids) => {
                for node_id in node_ids {
                    match service.remove_instance_module_node(instance_id, node_id) {
                        Ok((definition_id, _)) => {
                            set_active_definition(&mut state.node_editor, definition_id);
                            state.node_editor.selected_nodes.remove(&node_id);
                            if state.node_editor.primary_node == Some(node_id) {
                                state.node_editor.primary_node = None;
                            }
                        }
                        Err(error) => state.error = Some(error.to_string()),
                    }
                }
            }
            ModuleEditorAction::DeleteConnections(connection_ids) => {
                for connection_id in connection_ids {
                    match service.disconnect_instance_module_connection(instance_id, connection_id)
                    {
                        Ok((definition_id, _)) => {
                            set_active_definition(&mut state.node_editor, definition_id);
                            if state.node_editor.selected_connection == Some(connection_id) {
                                state.node_editor.selected_connection = None;
                            }
                        }
                        Err(error) => state.error = Some(error.to_string()),
                    }
                }
            }
            ModuleEditorAction::SetNodeState {
                node_id,
                name,
                enabled,
                bypassed,
            } => match service.set_instance_module_node_state(
                instance_id,
                node_id,
                name,
                enabled,
                bypassed,
            ) {
                Ok((definition_id, _)) => {
                    set_active_definition(&mut state.node_editor, definition_id);
                }
                Err(error) => state.error = Some(error.to_string()),
            },
            ModuleEditorAction::SetNodeProperty {
                node_id,
                key,
                property,
            } => {
                match service.set_instance_module_node_property(instance_id, node_id, key, property)
                {
                    Ok((definition_id, _)) => {
                        set_active_definition(&mut state.node_editor, definition_id);
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            ModuleEditorAction::CreateNode {
                request,
                graph_position,
            } => {
                let Some(request) = authoring_node_request(request) else {
                    continue;
                };
                match service.create_module_node(plugins, request, canvas_size.0, canvas_size.1) {
                    Ok(mut node) => {
                        node.ui_position = [graph_position.x, graph_position.y];
                        match service.add_instance_module_node(instance_id, node) {
                            Ok((node_id, definition_id, _)) => {
                                set_active_definition(&mut state.node_editor, definition_id);
                                state.node_editor.selected_nodes = HashSet::from([node_id]);
                                state.node_editor.primary_node = Some(node_id);
                                state.node_editor.selected_connection = None;
                            }
                            Err(error) => state.error = Some(error.to_string()),
                        }
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            ModuleEditorAction::EditInterface(command) => {
                match service.edit_instance_module_interface(instance_id, command) {
                    Ok((_, definition_id, _)) => {
                        set_active_definition(&mut state.node_editor, definition_id);
                        state.status = "Updated the Module interface".to_string();
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
        }
    }
}

fn authoring_node_request(request: ModuleNodeCreateRequest) -> Option<ModuleNodeRequest> {
    match request {
        ModuleNodeCreateRequest::Native(catalog_id) => {
            let descriptor = native_node_descriptor(&catalog_id)?;
            Some(match descriptor.factory() {
                NativeNodeFactory::Generator(GeneratorContent::Text) => ModuleNodeRequest::Text {
                    text: "Hello World".to_string(),
                    font: library::editor::project_service::DEFAULT_TEXT_FONT.to_string(),
                },
                NativeNodeFactory::Generator(GeneratorContent::Solid) => ModuleNodeRequest::Solid {
                    color: Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                },
                NativeNodeFactory::Generator(GeneratorContent::Shape) => ModuleNodeRequest::Shape {
                    path: library::editor::project_service::DEFAULT_SHAPE_PATH.to_string(),
                    width: 100,
                    height: 100,
                },
                NativeNodeFactory::Generator(GeneratorContent::SkSL) => ModuleNodeRequest::SkSL {
                    shader: library::editor::project_service::DEFAULT_SKSL_SHADER.to_string(),
                },
                _ => ModuleNodeRequest::NativeCatalog { catalog_id },
            })
        }
        ModuleNodeCreateRequest::PluginOperation {
            category,
            component_id,
            operation,
        } => Some(ModuleNodeRequest::PluginOperation {
            category,
            component_id,
            operation,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{
        CompositionInstance, DurationPolicy, InstancePath, MediaTime, ModuleTemplateOrigin,
        RationalRate, TimelineInterval, TimelineItemId, TransitionId,
    };

    #[test]
    fn transition_breadcrumb_identifies_media_sharing_and_timeline_scope() {
        for (media_type, expected_label, expected_media) in [
            (TransitionMediaType::Image, "Image Transition", "image"),
            (TransitionMediaType::Audio, "Audio Transition", "audio"),
        ] {
            let (definition, _) = ModuleDefinition::new_transition(
                "Reusable Transition",
                ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
                media_type,
            )
            .expect("Transition Module fixture");
            let host = ModuleEditorHost::Transition {
                transition_id: TransitionId::new(),
                instance_path: None,
                module_instance_id: ModuleInstanceId::new(),
            };
            let presentation = module_host_presentation(&definition, &host);

            assert_eq!(presentation.label, expected_label);
            assert_eq!(host.kind_name(), "transition");
            assert_eq!(presentation.media_type, Some(expected_media));
            let sharing = module_sharing_presentation(&definition.sharing, true);
            assert_eq!(sharing.label, "Reusable");
            assert!(!sharing.tooltip.contains("this Module instance"));
            assert!(sharing.tooltip.contains("all of its concrete placements"));
        }

        let private = module_sharing_presentation(&ModuleDefinitionSharing::Private, true);
        assert_eq!(private.label, "Private");
        assert!(!private.tooltip.contains("this Module instance"));
        assert!(private.tooltip.contains("every concrete placement"));
    }

    #[test]
    fn transition_breadcrumb_metadata_keeps_path_and_definition_impact() {
        let (definition, _) = ModuleDefinition::new_transition(
            "Transition",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .expect("Transition Module fixture");
        let transition_id = TransitionId::new();
        let timeline_id = TimelineId::new();
        let instance_path = InstancePath::root(TimelineId::new()).nested(TimelineItemId::new());
        let host = ModuleEditorHost::Transition {
            transition_id,
            instance_path: Some(instance_path.clone()),
            module_instance_id: ModuleInstanceId::new(),
        };
        let host_presentation = module_host_presentation(&definition, &host);
        let sharing = module_sharing_presentation(&definition.sharing, true);
        let metadata = document_breadcrumb_metadata(
            &definition,
            &host,
            host_presentation,
            sharing,
            Some(TransitionDefinitionScope {
                transition_id,
                timeline_id,
                timeline_name: "Nested",
                affected_placement_count: 2,
            }),
        );

        assert_eq!(metadata["edit_scope"], "timeline_definition");
        assert_eq!(metadata["instance_edit"], false);
        assert_eq!(metadata["transition_id"], serde_json::json!(transition_id));
        assert_eq!(
            metadata["captured_instance_path"],
            serde_json::json!(instance_path)
        );
        assert_eq!(metadata["timeline_id"], serde_json::json!(timeline_id));
        assert_eq!(metadata["affected_placement_count"], 2);
    }

    #[test]
    fn timeline_definition_placement_count_includes_repeated_nested_placements() {
        let fps = RationalRate::new(30, 1).expect("fixture frame rate");
        let duration = MediaTime::from_whole_seconds(10);
        let project = AuthoringProject::new("placement count", 320, 180, fps, duration)
            .expect("fixture Project");
        let service = TimelineEditorService::new(project).expect("fixture service");
        let root = service.snapshot().expect("root snapshot");
        let root_timeline_id = root.root_timeline_id;
        let root_track_id = root.timelines[&root_timeline_id].track_order[0];
        let (nested_timeline_id, _, _) = service
            .add_timeline("Nested".to_string(), 320, 180, fps, duration)
            .expect("nested Timeline");
        for layer in 0..2 {
            service
                .add_item(
                    root_track_id,
                    format!("Nested {layer}"),
                    SourceRef::Composition(CompositionInstance {
                        timeline_id: nested_timeline_id,
                        duration_policy: DurationPolicy::Fixed,
                        parameter_overrides: std::collections::HashMap::new(),
                        transition_module_overrides: Vec::new(),
                    }),
                    TimelineInterval::new(MediaTime::zero(), duration).expect("placement interval"),
                    layer,
                )
                .expect("nested placement");
        }
        let project = service.snapshot().expect("placement snapshot");

        assert_eq!(
            timeline_definition_placement_count(&project, root_timeline_id),
            1
        );
        assert_eq!(
            timeline_definition_placement_count(&project, nested_timeline_id),
            2
        );
    }

    #[test]
    fn plugin_request_keeps_its_operation_identity() {
        let request = ModuleNodeCreateRequest::PluginOperation {
            category: "effect".to_string(),
            component_id: "blur".to_string(),
            operation: "effect.apply.v1".to_string(),
        };
        assert!(matches!(
            authoring_node_request(request),
            Some(ModuleNodeRequest::PluginOperation { category, component_id, operation })
                if category == "effect"
                    && component_id == "blur"
                    && operation == "effect.apply.v1"
        ));
    }

    #[test]
    fn generator_catalog_entries_use_the_project_independent_factory_requests() {
        assert!(matches!(
            authoring_node_request(ModuleNodeCreateRequest::Native("native.text".to_string())),
            Some(ModuleNodeRequest::Text { .. })
        ));
        assert!(matches!(
            authoring_node_request(ModuleNodeCreateRequest::Native(
                "native.solid-color".to_string()
            )),
            Some(ModuleNodeRequest::Solid { .. })
        ));
        assert!(matches!(
            authoring_node_request(ModuleNodeCreateRequest::Native("native.shape".to_string())),
            Some(ModuleNodeRequest::Shape { .. })
        ));
        assert!(matches!(
            authoring_node_request(ModuleNodeCreateRequest::Native(
                "native.sksl-shader".to_string()
            )),
            Some(ModuleNodeRequest::SkSL { .. })
        ));
    }
}
