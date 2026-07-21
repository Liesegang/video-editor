use crate::command::{CommandId, CommandRegistry};
use eframe::egui;
use egui_phosphor::regular as icons;

fn shortcut_text(registry: &CommandRegistry, command: CommandId) -> &str {
    registry
        .find(command)
        .map_or("", |registered| registered.shortcut_text.as_str())
}

fn trigger_text(pointer_gesture: &str, shortcut: &str) -> String {
    if shortcut.is_empty() {
        pointer_gesture.to_string()
    } else {
        format!("{pointer_gesture} / {shortcut}")
    }
}

fn command_name(command: CommandId) -> &'static str {
    match command {
        CommandId::NodeEditorCleanLayout => "node_editor.clean_layout",
        CommandId::NodeEditorCleanLayoutSelection => "node_editor.clean_layout.selection",
        CommandId::NodeEditorCleanLayoutContainer => "node_editor.clean_layout.container",
        CommandId::NodeEditorCleanLayoutAll => "node_editor.clean_layout.all",
        _ => "unknown",
    }
}

fn command_label(command: CommandId) -> &'static str {
    match command {
        CommandId::NodeEditorCleanLayout => "Clean layout",
        CommandId::NodeEditorCleanLayoutSelection => "Clean layout selection",
        CommandId::NodeEditorCleanLayoutContainer => "Clean layout current container",
        CommandId::NodeEditorCleanLayoutAll => "Clean layout all",
        _ => "Unknown command",
    }
}

fn register_layout_control(
    response: &egui::Response,
    id: &str,
    command: CommandId,
    scope: &str,
    shortcut: &str,
    modifier_actions: Option<serde_json::Value>,
) {
    crate::qa::register_component_with_metadata(
        id,
        "node_editor_layout_command",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "command_id": command_name(command),
            "label": command_label(command),
            "scope": scope,
            "shortcut": shortcut,
            "modifier_actions": modifier_actions,
        })),
    );
}

fn command_for_click(modifiers: egui::Modifiers, has_selection: bool) -> Option<CommandId> {
    if modifiers.command || modifiers.ctrl {
        has_selection.then_some(CommandId::NodeEditorCleanLayoutSelection)
    } else if modifiers.alt {
        Some(CommandId::NodeEditorCleanLayoutContainer)
    } else if modifiers.shift {
        Some(CommandId::NodeEditorCleanLayoutAll)
    } else {
        Some(CommandId::NodeEditorCleanLayout)
    }
}

/// Compact Node Editor layout affordance. The persistent control is an icon;
/// the explicit scopes live in its adjacent transient menu.
pub(in crate::ui::panels::node_editor) fn layout_toolbar(
    ui: &mut egui::Ui,
    registry: &CommandRegistry,
    has_selection: bool,
    container_label: &str,
) -> Option<CommandId> {
    let mut requested = None;
    let resolved_scope = if has_selection {
        "selection"
    } else {
        "container"
    };
    let smart_shortcut = shortcut_text(registry, CommandId::NodeEditorCleanLayout);
    let selection_modifier = if has_selection {
        "Selection"
    } else {
        "Selection (unavailable: select one or more nodes)"
    };
    let smart_trigger = trigger_text("Click", smart_shortcut);
    let all_trigger = trigger_text(
        "Shift+click",
        shortcut_text(registry, CommandId::NodeEditorCleanLayoutAll),
    );
    let container_trigger = trigger_text(
        "Option/Alt+click",
        shortcut_text(registry, CommandId::NodeEditorCleanLayoutContainer),
    );
    let selection_trigger = trigger_text(
        "Cmd/Ctrl+click",
        shortcut_text(registry, CommandId::NodeEditorCleanLayoutSelection),
    );
    let tooltip = format!(
        "{} Clean layout\n{} — {}\n{} — All\n{} — {}\n{} — {}",
        icons::TREE_STRUCTURE,
        smart_trigger,
        if has_selection {
            "Selection"
        } else {
            container_label
        },
        all_trigger,
        container_trigger,
        container_label,
        selection_trigger,
        selection_modifier,
    );

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let smart = ui.button(icons::TREE_STRUCTURE).on_hover_text(tooltip);
        smart.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, smart.enabled(), "Clean layout")
        });
        register_layout_control(
            &smart,
            "node_editor.layout.smart",
            CommandId::NodeEditorCleanLayout,
            resolved_scope,
            smart_shortcut,
            Some(serde_json::json!({
                "plain": "smart",
                "shift": "all",
                "alt": "container",
                "command_or_ctrl": "selection",
                "selection_enabled": has_selection,
            })),
        );
        if smart.clicked() {
            let modifiers = ui.input(|input| input.modifiers);
            requested = command_for_click(modifiers, has_selection);
        }

        let menu = ui.menu_button(icons::CARET_DOWN, |ui| {
            let selection = ui
                .add_enabled(has_selection, egui::Button::new("Selection"))
                .on_hover_text("Lay out selected nodes without moving unselected nodes");
            register_layout_control(
                &selection,
                "node_editor.layout.selection",
                CommandId::NodeEditorCleanLayoutSelection,
                "selection",
                shortcut_text(registry, CommandId::NodeEditorCleanLayoutSelection),
                None,
            );
            if selection.clicked() {
                requested = Some(CommandId::NodeEditorCleanLayoutSelection);
                ui.close();
            }

            let container = ui
                .button(container_label)
                .on_hover_text("Lay out the current container only");
            register_layout_control(
                &container,
                "node_editor.layout.container",
                CommandId::NodeEditorCleanLayoutContainer,
                "container",
                shortcut_text(registry, CommandId::NodeEditorCleanLayoutContainer),
                None,
            );
            if container.clicked() {
                requested = Some(CommandId::NodeEditorCleanLayoutContainer);
                ui.close();
            }

            let all_shortcut = shortcut_text(registry, CommandId::NodeEditorCleanLayoutAll);
            let all_label = if all_shortcut.is_empty() {
                "All".to_string()
            } else {
                format!("All    {all_shortcut}")
            };
            let all = ui
                .button(all_label)
                .on_hover_text("Lay out every container and node in this composition");
            register_layout_control(
                &all,
                "node_editor.layout.all",
                CommandId::NodeEditorCleanLayoutAll,
                "all",
                all_shortcut,
                None,
            );
            if all.clicked() {
                requested = Some(CommandId::NodeEditorCleanLayoutAll);
                ui.close();
            }
        });
        menu.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                menu.response.enabled(),
                "Layout scope",
            )
        });
        crate::qa::register_component_with_metadata(
            "node_editor.layout.scope_menu",
            "node_editor_layout_scope_menu",
            menu.response.rect,
            menu.response.enabled(),
            Some(serde_json::json!({
                "label": "Layout scope",
                "resolved_scope": resolved_scope,
            })),
        );
    });
    requested
}

#[cfg(test)]
mod tests {
    use super::command_for_click;
    use crate::command::CommandId;
    use eframe::egui::Modifiers;

    #[test]
    fn tree_icon_modifiers_resolve_to_explicit_layout_commands() {
        assert_eq!(
            command_for_click(Modifiers::NONE, false),
            Some(CommandId::NodeEditorCleanLayout)
        );
        assert_eq!(
            command_for_click(Modifiers::SHIFT, false),
            Some(CommandId::NodeEditorCleanLayoutAll)
        );
        assert_eq!(
            command_for_click(Modifiers::ALT, false),
            Some(CommandId::NodeEditorCleanLayoutContainer)
        );
        assert_eq!(command_for_click(Modifiers::COMMAND, false), None);
        assert_eq!(
            command_for_click(Modifiers::COMMAND, true),
            Some(CommandId::NodeEditorCleanLayoutSelection)
        );
    }
}
