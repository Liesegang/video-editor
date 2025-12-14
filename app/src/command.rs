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
    ShowCommandPalette,
}

#[derive(Clone, PartialEq)]
pub struct Command {
    pub id: CommandId,
    pub text: String,
    pub shortcut: Option<(Modifiers, Key)>,
    pub shortcut_text: String,
    pub allow_when_focused: bool,
}

impl Command {
    fn new(
        id: CommandId,
        text: &str,
        shortcut: Option<(Modifiers, Key)>,
        allow_when_focused: bool,
    ) -> Self {
        let shortcut_text = get_shortcut_text(&shortcut);
        Self {
            id,
            text: text.to_string(),
            shortcut,
            shortcut_text,
            allow_when_focused,
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
    pub fn new(config: &AppConfig) -> Self {
        let mut commands = vec![
            // File Menu
            Command::new(
                CommandId::NewProject,
                "New Project",
                Some((Modifiers::COMMAND, Key::N)),
                true,
            ),
            Command::new(
                CommandId::LoadProject,
                "Load Project...",
                Some((Modifiers::COMMAND, Key::O)),
                true,
            ),
            Command::new(CommandId::Save, "Save", Some((Modifiers::COMMAND, Key::S)), true),
            Command::new(
                CommandId::SaveAs,
                "Save As...",
                Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::S)),
                true,
            ),
            Command::new(
                CommandId::Export,
                "Export...",
                Some((Modifiers::COMMAND, Key::E)),
                true,
            ),
            Command::new(CommandId::Quit, "Quit", Some((Modifiers::COMMAND, Key::Q)), true),
            // Edit Menu
            Command::new(CommandId::Undo, "Undo", Some((Modifiers::COMMAND, Key::Z)), false),
            Command::new(
                CommandId::Redo,
                "Redo",
                Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)),
                false,
            ),
            Command::new(
                CommandId::Delete,
                "Delete",
                Some((Modifiers::NONE, Key::Delete)),
                false,
            ),
            Command::new(
                CommandId::Settings,
                "Settings...",
                Some((Modifiers::COMMAND, Key::Comma)),
                true,
            ),
            // View Menu
            Command::new(CommandId::ResetLayout, "Reset Layout", None, true),
            // Playback (no menu item, but still a command)
            Command::new(
                CommandId::TogglePlayback,
                "Toggle Playback",
                Some((Modifiers::NONE, Key::Space)),
                false,
            ),
            // Tools
            Command::new(
                CommandId::ShowCommandPalette,
                "Command Palette",
                Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::P)),
                true,
            ),
        ];

        // Register TogglePanel commands
        for tab in Tab::all() {
            commands.push(Command::new(
                CommandId::TogglePanel(*tab),
                tab.name(), // Use tab name as command text
                None,       // No default shortcut for now, users can assign one
                true,       // Toggling panels should work even when focused
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
