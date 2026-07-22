use crate::command::{CommandId, CommandRegistry};
use eframe::egui;
use egui_phosphor::regular as icons;

const DIRECTIONAL_LAYOUT_HELP: &str = "Directional branch layout\n\
Hold A + drag a node header\n\
Left/Up: upstream  Right/Down: downstream\n\
Shift: align  Option/Alt: distribute  Shift+Option/Alt: both";

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

fn command_icon(command: CommandId) -> &'static str {
    match command {
        CommandId::NodeEditorCleanLayout => icons::TREE_STRUCTURE,
        CommandId::NodeEditorCleanLayoutSelection => icons::BOUNDING_BOX,
        CommandId::NodeEditorCleanLayoutContainer => icons::STACK,
        CommandId::NodeEditorCleanLayoutAll => icons::GRAPH,
        _ => icons::QUESTION,
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
    let directional_drag =
        (command == CommandId::NodeEditorCleanLayout).then(directional_drag_metadata);
    let metadata = serde_json::json!({
        "command_id": command_name(command),
        "label": command_label(command),
        "presentation": "icon",
        "icon": command_icon(command),
        "scope": scope,
        "shortcut": shortcut,
        "modifier_actions": modifier_actions,
        "directional_drag": directional_drag,
    });
    #[cfg(test)]
    {
        crate::ui::panels::node_editor::capture_test_rect(id, response.rect);
        crate::ui::panels::node_editor::capture_test_metadata(id, &metadata);
    }
    crate::qa::register_component_with_metadata(
        id,
        "node_editor_layout_command",
        response.rect,
        response.enabled(),
        Some(metadata),
    );
}

fn directional_drag_metadata() -> serde_json::Value {
    serde_json::json!({
        "label": "Directional branch layout",
        "help": DIRECTIONAL_LAYOUT_HELP,
        "trigger": "hold_a_primary_drag",
        "target": "node_header_or_overview_node",
        "axis_lock": "dominant_axis_after_threshold",
        "directions": {
            "left": "upstream",
            "up": "upstream",
            "right": "downstream",
            "down": "downstream",
        },
        "modifiers": {
            "plain": "layout",
            "shift": "align",
            "alt": "distribute",
            "shift_alt": "align_and_distribute",
        },
    })
}

fn command_for_click(modifiers: egui::Modifiers, has_selection: bool) -> Option<CommandId> {
    let selection_modifier = modifiers.command || modifiers.ctrl;
    let scope_modifier_count =
        u8::from(selection_modifier) + u8::from(modifiers.alt) + u8::from(modifiers.shift);
    if scope_modifier_count > 1 {
        return None;
    }
    if selection_modifier {
        has_selection.then_some(CommandId::NodeEditorCleanLayoutSelection)
    } else if modifiers.alt {
        Some(CommandId::NodeEditorCleanLayoutContainer)
    } else if modifiers.shift {
        Some(CommandId::NodeEditorCleanLayoutAll)
    } else {
        Some(CommandId::NodeEditorCleanLayout)
    }
}

fn scope_tooltip(
    registry: &CommandRegistry,
    command: CommandId,
    description: &str,
    mouse_gesture: &str,
) -> String {
    let shortcut = shortcut_text(registry, command);
    let trigger = trigger_text(mouse_gesture, shortcut);
    format!(
        "{}  {}\n{}\n{}",
        command_icon(command),
        command_label(command),
        description,
        trigger,
    )
}

fn icon_command_button(
    ui: &mut egui::Ui,
    registry: &CommandRegistry,
    command: CommandId,
    component_id: &str,
    scope: &str,
    enabled: bool,
    tooltip: String,
) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(command_icon(command)).size(16.0));
    let response = ui.add_enabled(enabled, button).on_hover_text(tooltip);
    let response = if command == CommandId::NodeEditorCleanLayoutSelection {
        response.on_disabled_hover_text("Select one or more nodes")
    } else {
        response
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            response.enabled(),
            command_label(command),
        )
    });
    register_layout_control(
        &response,
        component_id,
        command,
        scope,
        shortcut_text(registry, command),
        None,
    );
    response
}

/// Compact Node Editor layout affordance. Every persistent control is an icon
/// with an accessible label; the tree icon also accepts scope modifiers so a
/// pointer-heavy workflow does not have to travel across the toolbar.
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
    let smart_tooltip = format!(
        "{} Clean layout\n{} — {}\n{} — All\n{} — {}\n{} — {}\n\n{}",
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
        DIRECTIONAL_LAYOUT_HELP,
    );

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let smart = ui
            .add(egui::Button::new(
                egui::RichText::new(command_icon(CommandId::NodeEditorCleanLayout)).size(16.0),
            ))
            .on_hover_text(smart_tooltip);
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

        let selection = icon_command_button(
            ui,
            registry,
            CommandId::NodeEditorCleanLayoutSelection,
            "node_editor.layout.selection",
            "selection",
            has_selection,
            scope_tooltip(
                registry,
                CommandId::NodeEditorCleanLayoutSelection,
                "Arrange selected nodes without moving unselected nodes",
                "Cmd/Ctrl+click the tree icon",
            ),
        );
        if selection.clicked() {
            requested = Some(CommandId::NodeEditorCleanLayoutSelection);
        }

        let container = icon_command_button(
            ui,
            registry,
            CommandId::NodeEditorCleanLayoutContainer,
            "node_editor.layout.container",
            "container",
            true,
            scope_tooltip(
                registry,
                CommandId::NodeEditorCleanLayoutContainer,
                &format!("Arrange {container_label} without moving sibling containers"),
                "Option/Alt+click the tree icon",
            ),
        );
        if container.clicked() {
            requested = Some(CommandId::NodeEditorCleanLayoutContainer);
        }

        let all = icon_command_button(
            ui,
            registry,
            CommandId::NodeEditorCleanLayoutAll,
            "node_editor.layout.all",
            "all",
            true,
            scope_tooltip(
                registry,
                CommandId::NodeEditorCleanLayoutAll,
                "Arrange every container and node in this composition",
                "Shift+click the tree icon",
            ),
        );
        if all.clicked() {
            requested = Some(CommandId::NodeEditorCleanLayoutAll);
        }
    });
    requested
}

#[cfg(test)]
mod tests {
    use super::{command_for_click, command_icon, layout_toolbar, DIRECTIONAL_LAYOUT_HELP};
    use crate::command::{CommandId, CommandRegistry};
    use crate::config::AppConfig;
    use crate::ui::panels::node_editor::{reset_test_rects, test_metadata, test_rect};
    use eframe::egui::{self, Event, Modifiers, PointerButton, RawInput};

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
        assert_eq!(
            command_for_click(Modifiers::COMMAND | Modifiers::SHIFT, true),
            None
        );
        assert_eq!(
            command_for_click(Modifiers::ALT | Modifiers::SHIFT, true),
            None
        );
    }

    fn render_toolbar(
        context: &egui::Context,
        registry: &CommandRegistry,
        events: Vec<Event>,
        has_selection: bool,
    ) -> Option<CommandId> {
        let mut requested = None;
        drop(context.run(
            RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(480.0, 100.0),
                )),
                events,
                ..RawInput::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    requested = layout_toolbar(ui, registry, has_selection, "Current track");
                });
            },
        ));
        requested
    }

    fn click_component(
        context: &egui::Context,
        registry: &CommandRegistry,
        component_id: &str,
        has_selection: bool,
    ) -> Option<CommandId> {
        let center = test_rect(component_id)
            .unwrap_or_else(|| panic!("missing layout control {component_id}"))
            .center();
        assert_eq!(
            render_toolbar(
                context,
                registry,
                vec![
                    Event::PointerMoved(center),
                    Event::PointerButton {
                        pos: center,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                ],
                has_selection,
            ),
            None,
        );
        render_toolbar(
            context,
            registry,
            vec![Event::PointerButton {
                pos: center,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            has_selection,
        )
    }

    #[test]
    fn direct_scope_icons_dispatch_commands_and_publish_accessible_metadata() {
        let context = egui::Context::default();
        let registry = CommandRegistry::new(&AppConfig::new());
        reset_test_rects();
        assert_eq!(render_toolbar(&context, &registry, Vec::new(), true), None);

        let controls = [
            (
                "node_editor.layout.selection",
                CommandId::NodeEditorCleanLayoutSelection,
                "selection",
            ),
            (
                "node_editor.layout.container",
                CommandId::NodeEditorCleanLayoutContainer,
                "container",
            ),
            (
                "node_editor.layout.all",
                CommandId::NodeEditorCleanLayoutAll,
                "all",
            ),
        ];
        for (id, command, scope) in controls {
            assert!(test_rect(id).is_some_and(|rect| rect.is_positive()));
            let metadata = test_metadata(id).expect("layout control metadata");
            assert_eq!(metadata["scope"], scope);
            assert_eq!(metadata["command_id"], super::command_name(command));
            assert!(metadata["label"]
                .as_str()
                .is_some_and(|label| !label.is_empty()));
            assert!(metadata["shortcut"]
                .as_str()
                .is_some_and(|shortcut| shortcut.ends_with('L')));
        }
        assert_ne!(
            command_icon(CommandId::NodeEditorCleanLayoutSelection),
            command_icon(CommandId::NodeEditorCleanLayoutAll)
        );

        let smart =
            test_metadata("node_editor.layout.smart").expect("smart layout control metadata");
        let directional_drag = smart["directional_drag"]
            .as_object()
            .expect("directional drag discoverability metadata");
        assert_eq!(
            directional_drag["help"],
            serde_json::Value::String(DIRECTIONAL_LAYOUT_HELP.to_string())
        );
        assert_eq!(directional_drag["trigger"], "hold_a_primary_drag");
        assert_eq!(directional_drag["directions"]["left"], "upstream");
        assert_eq!(directional_drag["directions"]["down"], "downstream");
        assert_eq!(directional_drag["modifiers"]["shift"], "align");
        assert_eq!(directional_drag["modifiers"]["alt"], "distribute");
        assert_eq!(
            directional_drag["modifiers"]["shift_alt"],
            "align_and_distribute"
        );

        assert_eq!(
            click_component(&context, &registry, "node_editor.layout.selection", true,),
            Some(CommandId::NodeEditorCleanLayoutSelection),
        );
        assert_eq!(render_toolbar(&context, &registry, Vec::new(), true), None);
        assert_eq!(
            click_component(&context, &registry, "node_editor.layout.all", true,),
            Some(CommandId::NodeEditorCleanLayoutAll),
        );
    }
}
