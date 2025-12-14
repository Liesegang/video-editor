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
}

#[derive(Clone, PartialEq)]
pub struct Command {
    pub id: CommandId,
    pub text: String,
    pub shortcut: Option<(Modifiers, Key)>,
    pub shortcut_text: String,
    pub allow_when_focused: bool,
    pub trigger_on_release: bool,
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
        }
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
