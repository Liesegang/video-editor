use crate::command::{CommandId, CommandRegistry};
use crate::context::context::EditorContext;
use eframe::egui::{Context, Modifiers};

pub struct ShortcutManager;

impl ShortcutManager {
    pub fn new() -> Self {
        Self
    }

    pub fn handle_shortcuts(
        &self,
        ctx: &Context,
        registry: &CommandRegistry,
        editor_ctx: &mut EditorContext,
    ) -> Option<CommandId> {
        let wants_keyboard_input = ctx.wants_keyboard_input();

        for cmd in &registry.commands {
            // If the UI wants input (e.g. typing in text box),
            // ONLY trigger commands that are:
            // 1. Explicitly allowed when focused
            // 2. USE A "STRONG" MODIFIER (Ctrl, Alt, Cmd).
            //    Simple keys (A, Space) or Shift+Key should be blocked to avoid interfering with typing.
            if wants_keyboard_input {
                if !cmd.allow_when_focused {
                    continue;
                }

                // Check if the command has strong modifiers
                let has_strong_modifiers = if let Some((modifiers, _)) = cmd.shortcut {
                    modifiers.command || modifiers.ctrl || modifiers.alt
                } else {
                    false
                };

                if !has_strong_modifiers {
                    continue;
                }
            }

            if let Some((modifiers, key)) = cmd.shortcut {
                if cmd.trigger_on_release {
                    // Handle Release Triggers (e.g. Playback on Space release)
                    if ctx.input(|i| i.key_released(key) && modifiers_match(i.modifiers, modifiers))
                    {
                        // Special logic for Hand Tool Interaction
                        // If we used the key for dragging (Hand Tool), do not toggle playback.
                        if cmd.id == CommandId::TogglePlayback
                            && editor_ctx.interaction.preview.handled_hand_tool_drag
                        {
                            // Reset state and consume event (don't return command)
                            editor_ctx.interaction.preview.handled_hand_tool_drag = false;
                            continue;
                        }

                        return Some(cmd.id);
                    }
                } else {
                    // Standard Press Triggers
                    if ctx.input(|i| i.key_pressed(key) && modifiers_match(i.modifiers, modifiers))
                    {
                        return Some(cmd.id);
                    }
                }
            }
        }
        None
    }
}

fn modifiers_match(event_modifiers: Modifiers, expected_modifiers: Modifiers) -> bool {
    // Exact match is ideal
    if event_modifiers == expected_modifiers {
        return true;
    }

    // Handle COMMAND abstraction
    // If expected uses COMMAND, we assume it covers Ctrl (Win/Linux) or Cmd (Mac).
    // The event_modifiers will have both COMMAND and the physical key (Ctrl/Cmd) set.
    if expected_modifiers.command {
        // Must have command set
        if !event_modifiers.command {
            return false;
        }
        // Must match Alt and Shift
        if event_modifiers.alt != expected_modifiers.alt {
            return false;
        }
        if event_modifiers.shift != expected_modifiers.shift {
            return false;
        }
        // We ignore discrepancies in Ctrl/MacCmd because COMMAND abstracts them
        return true;
    }

    false
}
