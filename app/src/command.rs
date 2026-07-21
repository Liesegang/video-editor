use crate::config::AppConfig;
use crate::model::ui_types::Tab;
use eframe::egui::{Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CommandId {
    // File Menu
    NewProject,
    LoadProject,
    Save,
    SaveAs,
    Export,
    Quit,

    // Edit Menu
    Undo,
    Redo,
    Delete,
    Settings,

    // View Menu
    ResetLayout,
    TogglePanel(Tab),

    // Playback
    TogglePlayback,

    // Tools
    HandTool,
    ShowCommandPalette,

    // Node Editor
    NodeEditorCleanLayout,
    NodeEditorCleanLayoutSelection,
    NodeEditorCleanLayoutContainer,
    NodeEditorCleanLayoutAll,
}

impl CommandId {
    pub fn is_node_editor_layout(self) -> bool {
        matches!(
            self,
            Self::NodeEditorCleanLayout
                | Self::NodeEditorCleanLayoutSelection
                | Self::NodeEditorCleanLayoutContainer
                | Self::NodeEditorCleanLayoutAll
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandScope {
    Global,
    NodeEditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandContext {
    pub scope: CommandScope,
    pub has_node_selection: bool,
}

impl CommandContext {
    pub fn palette_origin(area: Self, focused: Self) -> Self {
        Self {
            scope: if area.scope == CommandScope::NodeEditor
                || focused.scope == CommandScope::NodeEditor
            {
                CommandScope::NodeEditor
            } else {
                CommandScope::Global
            },
            has_node_selection: area.has_node_selection || focused.has_node_selection,
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct Command {
    pub id: CommandId,
    pub text: String,
    pub shortcut: Option<(Modifiers, Key)>,
    pub shortcut_text: String,
    pub allow_when_focused: bool,
    pub trigger_on_release: bool,
    pub scope: CommandScope,
}

#[derive(Clone)]
pub struct CommandRegistry {
    pub commands: Vec<Command>,
}

fn get_shortcut_text(shortcut: &Option<(Modifiers, Key)>) -> String {
    if let Some((m, k)) = shortcut {
        let mut parts = Vec::new();
        if m.command {
            parts.push("Ctrl");
        }
        if m.ctrl && !m.command {
            parts.push("Ctrl");
        }
        if m.shift {
            parts.push("Shift");
        }
        if m.alt {
            parts.push("Alt");
        }
        let key_str = format!("{:?}", k);
        parts.push(&key_str);
        parts.join("+")
    } else {
        "".to_string()
    }
}

impl Command {
    fn new(
        id: CommandId,
        text: &str,
        shortcut: Option<(Modifiers, Key)>,
        allow_when_focused: bool,
        trigger_on_release: bool,
    ) -> Self {
        let shortcut_text = get_shortcut_text(&shortcut);
        Self {
            id,
            text: text.to_string(),
            shortcut,
            shortcut_text,
            allow_when_focused,
            trigger_on_release,
            scope: CommandScope::Global,
        }
    }

    fn in_scope(mut self, scope: CommandScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn is_available_in(&self, context: CommandContext) -> bool {
        let scope_matches = self.scope == CommandScope::Global || self.scope == context.scope;
        let selection_matches =
            self.id != CommandId::NodeEditorCleanLayoutSelection || context.has_node_selection;
        scope_matches && selection_matches
    }
}
// ...
impl CommandRegistry {
    pub fn new(config: &AppConfig) -> Self {
        let mut commands = vec![
            // File Menu
            Command::new(
                CommandId::NewProject,
                "New Project",
                Some((Modifiers::COMMAND, Key::N)),
                true,
                false,
            ),
            Command::new(
                CommandId::LoadProject,
                "Load Project...",
                Some((Modifiers::COMMAND, Key::O)),
                true,
                false,
            ),
            Command::new(
                CommandId::Save,
                "Save",
                Some((Modifiers::COMMAND, Key::S)),
                true,
                false,
            ),
            Command::new(
                CommandId::SaveAs,
                "Save As...",
                Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::S)),
                true,
                false,
            ),
            Command::new(
                CommandId::Export,
                "Export...",
                Some((Modifiers::COMMAND, Key::E)),
                true,
                false,
            ),
            Command::new(
                CommandId::Quit,
                "Quit",
                Some((Modifiers::COMMAND, Key::Q)),
                true,
                false,
            ),
            // Edit Menu
            Command::new(
                CommandId::Undo,
                "Undo",
                Some((Modifiers::COMMAND, Key::Z)),
                false,
                false,
            ),
            Command::new(
                CommandId::Redo,
                "Redo",
                Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)),
                false,
                false,
            ),
            Command::new(
                CommandId::Delete,
                "Delete",
                Some((Modifiers::NONE, Key::Delete)),
                false,
                false,
            ),
            Command::new(
                CommandId::Settings,
                "Settings...",
                Some((Modifiers::COMMAND, Key::Comma)),
                true,
                false,
            ),
            // View Menu
            Command::new(CommandId::ResetLayout, "Reset Layout", None, true, false),
            // Playback (no menu item, but still a command)
            Command::new(
                CommandId::TogglePlayback,
                "Toggle Playback",
                Some((Modifiers::NONE, Key::Space)),
                false,
                true, // Trigger on release
            ),
            // Tools
            Command::new(
                CommandId::HandTool,
                "Hand Tool (Hold)",
                Some((Modifiers::NONE, Key::Space)),
                true, // Allow focused for panning in text fields? Maybe no.
                false,
            ),
            Command::new(
                CommandId::ShowCommandPalette,
                "Command Palette",
                Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::P)),
                true,
                false,
            ),
            Command::new(
                CommandId::NodeEditorCleanLayout,
                "Node Editor: Clean Layout",
                Some((Modifiers::NONE, Key::L)),
                false,
                false,
            )
            .in_scope(CommandScope::NodeEditor),
            Command::new(
                CommandId::NodeEditorCleanLayoutSelection,
                "Node Editor: Clean Layout Selection",
                None,
                false,
                false,
            )
            .in_scope(CommandScope::NodeEditor),
            Command::new(
                CommandId::NodeEditorCleanLayoutContainer,
                "Node Editor: Clean Layout Current Container",
                None,
                false,
                false,
            )
            .in_scope(CommandScope::NodeEditor),
            Command::new(
                CommandId::NodeEditorCleanLayoutAll,
                "Node Editor: Clean Layout All",
                Some((Modifiers::SHIFT, Key::L)),
                false,
                false,
            )
            .in_scope(CommandScope::NodeEditor),
        ];

        // Register TogglePanel commands
        for tab in Tab::all() {
            commands.push(Command::new(
                CommandId::TogglePanel(*tab),
                tab.name(),
                None,
                true,
                false,
            ));
        }
        // Override defaults with user config
        for cmd in &mut commands {
            if let Some(loaded_shortcut_opt) = config.shortcuts.get(&cmd.id) {
                // If the key is present in the config map:
                // - Some(shortcut) -> Override with new shortcut
                // - None           -> Explicitly unbound (user cleared it)
                cmd.shortcut = *loaded_shortcut_opt;
                cmd.shortcut_text = get_shortcut_text(&cmd.shortcut);
            }
        }

        Self { commands }
    }

    pub fn find(&self, id: CommandId) -> Option<&Command> {
        self.commands.iter().find(|&cmd| cmd.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandContext, CommandId, CommandRegistry, CommandScope};
    use crate::config::AppConfig;
    use eframe::egui::{Key, Modifiers};

    #[test]
    fn node_layout_commands_are_stable_contextual_commands() {
        let registry = CommandRegistry::new(&AppConfig::new());
        let smart = registry
            .find(CommandId::NodeEditorCleanLayout)
            .expect("smart layout command");
        assert_eq!(smart.scope, CommandScope::NodeEditor);
        assert_eq!(smart.shortcut, Some((Modifiers::NONE, Key::L)));
        assert!(!smart.is_available_in(CommandContext {
            scope: CommandScope::Global,
            has_node_selection: false,
        }));
        assert!(smart.is_available_in(CommandContext {
            scope: CommandScope::NodeEditor,
            has_node_selection: false,
        }));

        let all = registry
            .find(CommandId::NodeEditorCleanLayoutAll)
            .expect("all layout command");
        assert_eq!(all.shortcut, Some((Modifiers::SHIFT, Key::L)));

        assert!(registry
            .find(CommandId::NodeEditorCleanLayoutSelection)
            .is_some());
        assert!(registry
            .find(CommandId::NodeEditorCleanLayoutContainer)
            .is_some());

        let selection = registry
            .find(CommandId::NodeEditorCleanLayoutSelection)
            .expect("selection layout command");
        assert!(!selection.is_available_in(CommandContext {
            scope: CommandScope::NodeEditor,
            has_node_selection: false,
        }));
        assert!(selection.is_available_in(CommandContext {
            scope: CommandScope::NodeEditor,
            has_node_selection: true,
        }));
    }

    #[test]
    fn palette_origin_accepts_area_or_focused_node_context() {
        let global = CommandContext {
            scope: CommandScope::Global,
            has_node_selection: false,
        };
        let node = CommandContext {
            scope: CommandScope::NodeEditor,
            has_node_selection: true,
        };
        assert_eq!(CommandContext::palette_origin(node, global), node);
        assert_eq!(CommandContext::palette_origin(global, node), node);
        assert_eq!(CommandContext::palette_origin(global, global), global);
    }
}
