use eframe::egui::Context;
use crate::command::{CommandRegistry, CommandId};

pub struct ShortcutManager;

impl ShortcutManager {
    pub fn new() -> Self {
        Self
    }

    pub fn handle_shortcuts(&self, ctx: &Context, registry: &CommandRegistry) -> Option<CommandId> {
        for cmd in &registry.commands {
            if let Some((modifiers, key)) = cmd.shortcut {
                if ctx.input(|i| i.key_pressed(key) && i.modifiers == modifiers) {
                    return Some(cmd.id);
                }
            }
        }
        None
    }
}
