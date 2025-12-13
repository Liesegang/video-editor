use crate::config::ShortcutConfig;
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
}

#[derive(Clone, PartialEq)]
pub struct Command {
    pub id: CommandId,
    pub text: String, // Changed to String to support dynamic text
    pub shortcut: Option<(Modifiers, Key)>,
    pub shortcut_text: String, // Made mutable
}

impl Command {
    fn new(id: CommandId, text: &str, shortcut: Option<(Modifiers, Key)>) -> Self {
        let shortcut_text = get_shortcut_text(&shortcut);
        Self {
            id,
            text: text.to_string(),
            shortcut,
            shortcut_text,
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct CommandRegistry {
    pub commands: Vec<Command>,
}

// Helper to create the text representation of a shortcut
fn get_shortcut_text(shortcut: &Option<(Modifiers, Key)>) -> String {
    if let Some((mods, key)) = shortcut {
        let mut parts = Vec::new();
        if mods.command {
            parts.push("Ctrl");
        }
        if mods.shift {
            parts.push("Shift");
        }
        if mods.alt {
            parts.push("Alt");
        }
        let key_str = format!("{:?}", key);
        parts.push(&key_str.as_str());
        parts.join("+")
    } else {
        String::new()
    }
}

impl CommandRegistry {
    pub fn new(config: &ShortcutConfig) -> Self {
        let mut commands = vec![
            // File Menu
            Command::new(
                CommandId::NewProject,
                "New Project",
                Some((Modifiers::COMMAND, Key::N)),
            ),
            Command::new(
                CommandId::LoadProject,
                "Load Project...",
                Some((Modifiers::COMMAND, Key::O)),
            ),
            Command::new(CommandId::Save, "Save", Some((Modifiers::COMMAND, Key::S))),
            Command::new(
                CommandId::SaveAs,
                "Save As...",
                Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::S)),
            ),
            Command::new(
                CommandId::Export,
                "Export...",
                Some((Modifiers::COMMAND, Key::E)),
            ),
            Command::new(CommandId::Quit, "Quit", Some((Modifiers::COMMAND, Key::Q))),
            // Edit Menu
            Command::new(CommandId::Undo, "Undo", Some((Modifiers::COMMAND, Key::Z))),
            Command::new(
                CommandId::Redo,
                "Redo",
                Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)),
            ),
            Command::new(
                CommandId::Delete,
                "Delete",
                Some((Modifiers::NONE, Key::Delete)),
            ),
            Command::new(CommandId::Settings, "Settings...", None),
            // View Menu
            Command::new(CommandId::ResetLayout, "Reset Layout", None),
            // Playback (no menu item, but still a command)
            Command::new(
                CommandId::TogglePlayback,
                "Toggle Playback",
                Some((Modifiers::NONE, Key::Space)),
            ),
        ];

        // Register TogglePanel commands
        for tab in Tab::all() {
            commands.push(Command::new(
                CommandId::TogglePanel(*tab),
                tab.name(), // Use tab name as command text
                None,       // No default shortcut for now, users can assign one
            ));
        }
        // Override defaults with user config
        for cmd in &mut commands {
            if let Some(loaded_shortcut) = config.shortcuts.get(&cmd.id) {
                cmd.shortcut = Some(*loaded_shortcut);
                cmd.shortcut_text = get_shortcut_text(&cmd.shortcut);
            }
        }

        Self { commands }
    }

    pub fn find(&self, id: CommandId) -> Option<&Command> {
        self.commands.iter().find(|&cmd| cmd.id == id)
    }
}
