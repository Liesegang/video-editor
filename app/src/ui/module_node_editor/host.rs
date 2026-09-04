use super::*;

/// Render the explicitly opened Module document in the existing docked Node
/// Editor. Timeline containers and ordinary items are intentionally never
/// inferred as documents.
pub fn module_node_editor_panel(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) {
    let Some(ModuleNodeEditorDocument::ModuleDefinition {
        definition_id,
        host,
    }) = state.node_editor.active_document.clone()
    else {
        let response =
            ui.centered_and_justified(|ui| ui.weak("Open a Node Clip to edit its Module logic."));
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

    let instance_id = module_instance_id(&host);
    let Some(instance) = project.module_instances.get(&instance_id) else {
        render_unavailable_document(
            ui,
            state,
            "The Module instance for this Node Clip is no longer available.",
        );
        return;
    };
    let current_definition_id = instance.definition_id;
    if current_definition_id != definition_id {
        set_active_definition(&mut state.node_editor, current_definition_id);
    }
    let Some(definition) = project.module_definitions.get(&current_definition_id) else {
        render_unavailable_document(ui, state, "The Module definition is no longer available.");
        return;
    };
    let Some(invocation) = module_invocation(project, &host)
        .filter(|invocation| invocation.instance_id == instance_id)
    else {
        render_unavailable_document(
            ui,
            state,
            "The Module placement for this Node document is no longer available.",
        );
        return;
    };
    if apply_pending_layout(ui, definition, instance_id, state, service) {
        return;
    }

    let timeline = project
        .timelines
        .get(&state.active_timeline_id)
        .or_else(|| project.timelines.get(&project.root_timeline_id));
    let canvas_size = timeline
        .map(|timeline| (timeline.width, timeline.height))
        .unwrap_or((1920, 1080));
    let property_time = timeline.map_or(0.0, |timeline| {
        state.timeline.current_frame as f64 / timeline.fps.to_f64()
    });
    let actions = show_module_document(
        ui,
        definition,
        Some(invocation.output_id),
        &mut state.node_editor,
        plugins,
        property_time,
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

fn render_unavailable_document(ui: &mut egui::Ui, state: &mut AuthoringUiState, message: &str) {
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
    state.error = Some(message.to_string());
}

const fn module_instance_id(host: &ModuleEditorHost) -> ModuleInstanceId {
    match host {
        ModuleEditorHost::NodeClip {
            module_instance_id, ..
        }
        | ModuleEditorHost::Attachment {
            module_instance_id, ..
        } => *module_instance_id,
    }
}

fn module_invocation<'a>(
    project: &'a AuthoringProject,
    host: &ModuleEditorHost,
) -> Option<&'a ModuleInvocation> {
    match host {
        ModuleEditorHost::NodeClip {
            timeline_item_id, ..
        } => project.items.get(timeline_item_id).and_then(|item| {
            let SourceRef::Module(invocation) = &item.source else {
                return None;
            };
            Some(invocation)
        }),
        ModuleEditorHost::Attachment { attachment_id, .. } => project
            .attachments
            .get(attachment_id)
            .and_then(|attachment| {
                let AttachmentProcessor::Module(invocation) = &attachment.processor else {
                    return None;
                };
                Some(invocation)
            }),
    }
}

fn set_active_definition(
    state: &mut ModuleNodeEditorState,
    definition_id: library::model::authoring::ModuleDefinitionId,
) {
    if let Some(ModuleNodeEditorDocument::ModuleDefinition {
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
                        state.status = "Updated the Node Clip interface".to_string();
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
