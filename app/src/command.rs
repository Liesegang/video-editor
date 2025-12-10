use eframe::egui::{Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    // File Menu
    NewProject,
    LoadProject,
    Save,
    SaveAs,
    Quit,

    // Edit Menu
    Undo,
    Redo,
    Delete,

    // View Menu
    ResetLayout,

    // Playback
    TogglePlayback,
}

pub struct Command {
    pub id: CommandId,
    pub text: &'static str,
    pub shortcut: Option<(Modifiers, Key)>,
    pub shortcut_text: &'static str,
}

pub struct CommandRegistry {
    pub commands: Vec<Command>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        let commands = vec![
            // File Menu
            Command {
                id: CommandId::NewProject,
                text: "New Project",
                shortcut: Some((Modifiers::COMMAND, Key::N)),
                shortcut_text: "Ctrl+N",
            },
            Command {
                id: CommandId::LoadProject,
                text: "Load Project...",
                shortcut: Some((Modifiers::COMMAND, Key::O)),
                shortcut_text: "Ctrl+O",
            },
            Command {
                id: CommandId::Save,
                text: "Save",
                shortcut: Some((Modifiers::COMMAND, Key::S)),
                shortcut_text: "Ctrl+S",
            },
            Command {
                id: CommandId::SaveAs,
                text: "Save As...",
                shortcut: Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::S)),
                shortcut_text: "Ctrl+Shift+S",
            },
            Command {
                id: CommandId::Quit,
                text: "Quit",
                shortcut: Some((Modifiers::COMMAND, Key::Q)),
                shortcut_text: "Ctrl+Q",
            },
            // Edit Menu
            Command {
                id: CommandId::Undo,
                text: "Undo",
                shortcut: Some((Modifiers::COMMAND, Key::Z)),
                shortcut_text: "Ctrl+Z",
            },
            Command {
                id: CommandId::Redo,
                text: "Redo",
                shortcut: Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)),
                shortcut_text: "Ctrl+Shift+Z",
            },
            Command {
                id: CommandId::Delete,
                text: "Delete",
                shortcut: Some((Modifiers::NONE, Key::Delete)),
                shortcut_text: "Del",
            },
            // View Menu
            Command {
                id: CommandId::ResetLayout,
                text: "Reset Layout",
                shortcut: None,
                shortcut_text: "",
            },
            // Playback (no menu item, but still a command)
            Command {
                id: CommandId::TogglePlayback,
                text: "Toggle Playback",
                shortcut: Some((Modifiers::NONE, Key::Space)),
                shortcut_text: "",
            },
        ];
        Self { commands }
    }

    pub fn find(&self, id: CommandId) -> Option<&Command> {
        self.commands.iter().find(|&cmd| cmd.id == id)
    }
}
