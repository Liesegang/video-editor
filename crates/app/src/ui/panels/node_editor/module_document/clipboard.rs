//! Native clipboard routing for the existing Module document and edit service.

use std::collections::HashSet;

use super::*;

const CLIPBOARD_PREFIX: &str = "ruvie.module-selection\n";

#[cfg(test)]
mod tests;

pub(super) fn copy_selection(
    context: &egui::Context,
    service: &TimelineEditorService,
    host: &ModuleEditorHost,
    nodes: &[Uuid],
    state: &mut AuthoringUiState,
) {
    let result = service
        .snapshot()
        .map_err(|error| error.to_string())
        .and_then(|project| {
            library::editor::ModuleSelectionClipboard::capture(
                &project,
                host.module_instance_id(),
                host.transition_id().and(host.captured_instance_path()),
                nodes,
            )
        })
        .and_then(|selection| serde_json::to_string(&selection).map_err(|error| error.to_string()));
    match result {
        Ok(json) => {
            context.copy_text(format!("{CLIPBOARD_PREFIX}{json}"));
            state.status = "Copied selected processing nodes".to_string();
        }
        Err(error) => state.error = Some(error),
    }
}

pub(super) fn paste_selection(
    service: &TimelineEditorService,
    host: &ModuleEditorHost,
    text: &str,
    position: egui::Pos2,
    state: &mut AuthoringUiState,
) {
    let Some(json) = text.strip_prefix(CLIPBOARD_PREFIX) else {
        state.status = "The clipboard does not contain nodes".to_string();
        return;
    };
    let result = serde_json::from_str::<library::editor::ModuleSelectionClipboard>(json)
        .map_err(|error| format!("Invalid node clipboard: {error}"))
        .and_then(|selection| {
            service
                .paste_instance_module_selection(
                    host.module_instance_id(),
                    host.transition_id().and(host.captured_instance_path()),
                    &selection,
                    [position.x, position.y],
                )
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(receipt) => {
            if let Some(NodeEditorDocument::ModuleDefinition { definition_id, .. }) =
                state.node_editor.active_document.as_mut()
            {
                *definition_id = receipt.definition_id;
            }
            state.node_editor.selected_nodes = receipt.node_ids.iter().copied().collect();
            state.node_editor.primary_node = receipt.node_ids.first().copied();
            state.node_editor.selected_connection = None;
            state.node_editor.node_drag_offsets.clear();
            state.node_editor.surface_interaction.cancel();
            state.status = format!("Pasted {} nodes", receipt.node_ids.len());
        }
        Err(error) => state.error = Some(error),
    }
}

pub(super) fn menu_actions(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    selection: &HashSet<Uuid>,
    clicked_node: Option<Uuid>,
    graph_position: egui::Pos2,
) -> Option<ModuleEditorAction> {
    let selected = selected_for_copy(definition, selection, clicked_node);
    let mut action = None;
    for (name, label, shortcut, enabled) in [
        ("copy", "Copy", "Ctrl+C", !selected.is_empty()),
        ("paste", "Paste", "Ctrl+V", true),
    ] {
        let response = ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut));
        let rect = ui
            .ctx()
            .layer_transform_to_global(ui.layer_id())
            .unwrap_or(egui::emath::TSTransform::IDENTITY)
            * response.rect;
        crate::qa::register_component_with_metadata(
            clicked_node.map_or_else(
                || format!("node_editor.menu.{name}"),
                |id| format!("node_editor.node_menu:{id}:{name}"),
            ),
            "node_menu_action",
            rect,
            enabled,
            Some(serde_json::json!({"action": name, "node_ids": selected})),
        );
        if response.clicked() {
            action = Some(if name == "copy" {
                ModuleEditorAction::CopyNodes(selected.clone())
            } else {
                ModuleEditorAction::RequestPaste(graph_position)
            });
        }
    }
    action
}

fn selected_for_copy(
    definition: &ModuleDefinition,
    selection: &HashSet<Uuid>,
    clicked: Option<Uuid>,
) -> Vec<Uuid> {
    let mut nodes: Vec<_> = match clicked.filter(|id| !selection.contains(id)) {
        Some(id) => vec![id],
        None => selection.iter().copied().collect(),
    };
    nodes.retain(|id| {
        definition.graph.nodes.contains_key(id)
            && !is_module_output_node(definition, *id)
            && !definition.is_protected_host_boundary_node(*id)
    });
    nodes.sort_unstable();
    nodes
}

pub(super) fn keyboard_actions(
    ui: &egui::Ui,
    state: &mut NodeEditorState,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
) -> Vec<ModuleEditorAction> {
    let expired_request = state
        .pending_paste
        .as_ref()
        .is_some_and(|(_, _, deadline)| ui.input(|input| input.time) > *deadline);
    if expired_request {
        state.pending_paste = None;
    }
    let requested = state.pending_paste.is_some();
    if !requested
        && (!state.keyboard_focused
            || ui.ctx().wants_keyboard_input()
            || egui::Popup::is_any_open(ui.ctx())
            || state.create_menu.is_some()
            || state.wire_menu.is_some())
    {
        return Vec::new();
    }
    let position = super::context_menu::visible_creation_position(
        ui.ctx()
            .pointer_latest_pos()
            .filter(|point| viewport.contains(*point))
            .unwrap_or_else(|| viewport.center()),
        viewport,
        transform,
    );
    let mut actions = Vec::new();
    let mut copied = false;
    let mut pasted = false;
    ui.input_mut(|input| {
        input.events.retain(|event| match event {
            egui::Event::Copy if !requested => {
                copied = true;
                false
            }
            egui::Event::Paste(text) => {
                pasted = true;
                if expired_request {
                    return false;
                }
                let pending = state.pending_paste.take();
                let host = state
                    .active_document
                    .as_ref()
                    .map(|document| match document {
                        NodeEditorDocument::ModuleDefinition { host, .. } => host,
                    });
                // A native clipboard response must never edit a newly opened document.
                if pending
                    .as_ref()
                    .is_none_or(|(origin, _, _)| Some(origin) == host)
                {
                    actions.push(ModuleEditorAction::PasteNodes {
                        text: text.clone(),
                        graph_position: pending.map_or(position, |(_, point, _)| point),
                    });
                }
                false
            }
            _ => true,
        });
        if !requested {
            copied |= input.consume_key(egui::Modifiers::COMMAND, egui::Key::C);
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::V) && !pasted {
                actions.push(ModuleEditorAction::RequestPaste(position));
            }
        }
    });
    if copied && !state.selected_nodes.is_empty() {
        actions.push(ModuleEditorAction::CopyNodes(
            state.selected_nodes.iter().copied().collect(),
        ));
    }
    actions
}
