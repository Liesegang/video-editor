use crate::command::{CommandContext, CommandId, CommandRegistry};
use crate::state::context::EditorContext;
use eframe::egui::{Context, InputState, Key, Modifiers};

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
        command_context: CommandContext,
    ) -> Option<CommandId> {
        let wants_keyboard_input = ctx.wants_keyboard_input();

        for cmd in &registry.commands {
            if !cmd.is_available_in(command_context) {
                continue;
            }
            if cmd.id.is_node_editor_layout()
                && (ctx.input(|input| {
                    input.pointer.any_down()
                        || input.pointer.any_released()
                        || input.raw_scroll_delta != eframe::egui::Vec2::ZERO
                        || (input.zoom_delta() - 1.0).abs() > f32::EPSILON
                }) || crate::action::node_layout_command_blocked(&editor_ctx.node_editor_state))
            {
                continue;
            }
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
                            && editor_ctx.interaction.handled_hand_tool_drag
                        {
                            // Reset state and consume event (don't return command)
                            editor_ctx.interaction.handled_hand_tool_drag = false;
                            continue;
                        }

                        return Some(cmd.id);
                    }
                } else {
                    // Standard Press Triggers
                    if ctx.input(|i| {
                        let pressed = if cmd.id.is_node_editor_layout() {
                            key_pressed_once(i, key)
                        } else {
                            i.key_pressed(key)
                        };
                        pressed && modifiers_match(i.modifiers, modifiers)
                    }) {
                        return Some(cmd.id);
                    }
                }
            }
        }
        None
    }
}

fn key_pressed_once(input: &InputState, key: Key) -> bool {
    input.events.iter().any(|event| {
        matches!(
            event,
            eframe::egui::Event::Key {
                key: event_key,
                pressed: true,
                repeat: false,
                ..
            } if *event_key == key
        )
    })
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

#[cfg(test)]
mod tests {
    use super::ShortcutManager;
    use crate::command::{CommandContext, CommandId, CommandRegistry, CommandScope};
    use crate::config::AppConfig;
    use crate::state::context::EditorContext;
    use eframe::egui::{self, Event, Key, Modifiers, PointerButton, RawInput};

    fn key_input_with_repeat(modifiers: Modifiers, repeat: bool) -> RawInput {
        RawInput {
            modifiers,
            events: vec![Event::Key {
                key: Key::L,
                physical_key: Some(Key::L),
                pressed: true,
                repeat,
                modifiers,
            }],
            ..RawInput::default()
        }
    }

    fn key_input(modifiers: Modifiers) -> RawInput {
        key_input_with_repeat(modifiers, false)
    }

    fn dispatch(
        context: &egui::Context,
        input: RawInput,
        command_context: CommandContext,
    ) -> Option<CommandId> {
        let registry = CommandRegistry::new(&AppConfig::new());
        let manager = ShortcutManager::new();
        let mut editor = EditorContext::new(uuid::Uuid::new_v4());
        let mut dispatched = None;
        drop(context.run(input, |context| {
            dispatched = manager.handle_shortcuts(context, &registry, &mut editor, command_context);
        }));
        dispatched
    }

    fn node_context(has_node_selection: bool) -> CommandContext {
        CommandContext {
            scope: CommandScope::NodeEditor,
            has_node_selection,
        }
    }

    #[test]
    fn node_layout_shortcuts_dispatch_explicit_scopes_only_in_node_editor() {
        let cases = [
            (Modifiers::NONE, CommandId::NodeEditorCleanLayout),
            (
                Modifiers::COMMAND,
                CommandId::NodeEditorCleanLayoutSelection,
            ),
            (Modifiers::ALT, CommandId::NodeEditorCleanLayoutContainer),
            (Modifiers::SHIFT, CommandId::NodeEditorCleanLayoutAll),
        ];
        for (modifiers, expected) in cases {
            assert_eq!(
                dispatch(
                    &egui::Context::default(),
                    key_input(modifiers),
                    node_context(true)
                ),
                Some(expected),
                "modifiers={modifiers:?}",
            );
        }

        assert_eq!(
            dispatch(
                &egui::Context::default(),
                key_input(Modifiers::NONE),
                CommandContext {
                    scope: CommandScope::Global,
                    has_node_selection: true,
                },
            ),
            None,
        );
        assert_eq!(
            dispatch(
                &egui::Context::default(),
                key_input(Modifiers::COMMAND),
                node_context(false),
            ),
            None,
        );
    }

    #[test]
    fn node_layout_shortcuts_do_not_fire_while_editing_text() {
        let context = egui::Context::default();
        let registry = CommandRegistry::new(&AppConfig::new());
        let manager = ShortcutManager::new();
        let mut editor = EditorContext::new(uuid::Uuid::new_v4());
        let mut text = String::new();

        drop(context.run(RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                ui.text_edit_singleline(&mut text).request_focus();
            });
        }));

        let mut dispatched = None;
        drop(context.run(key_input(Modifiers::COMMAND), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                ui.text_edit_singleline(&mut text);
            });
            assert!(context.wants_keyboard_input());
            dispatched =
                manager.handle_shortcuts(context, &registry, &mut editor, node_context(true));
        }));
        assert_eq!(dispatched, None);
    }

    fn pointer_button(position: egui::Pos2, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton {
            pos: position,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn node_layout_shortcuts_wait_for_all_pointer_gestures_and_the_release_frame() {
        for button in [PointerButton::Middle, PointerButton::Secondary] {
            let position = egui::pos2(40.0, 30.0);
            let mut input = key_input(Modifiers::NONE);
            input.events.insert(0, Event::PointerMoved(position));
            input
                .events
                .insert(1, pointer_button(position, button, true));
            assert_eq!(
                dispatch(&egui::Context::default(), input, node_context(true)),
                None,
                "button={button:?}",
            );
        }

        let context = egui::Context::default();
        let position = egui::pos2(40.0, 30.0);
        drop(context.run(
            RawInput {
                events: vec![
                    Event::PointerMoved(position),
                    pointer_button(position, PointerButton::Primary, true),
                ],
                ..RawInput::default()
            },
            |_| {},
        ));
        let mut release_and_key = key_input(Modifiers::NONE);
        release_and_key
            .events
            .insert(0, pointer_button(position, PointerButton::Primary, false));
        assert_eq!(
            dispatch(&context, release_and_key, node_context(true)),
            None
        );

        let key_event = key_input(Modifiers::NONE)
            .events
            .into_iter()
            .next()
            .expect("key event");
        let scroll_and_key = RawInput {
            events: vec![
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 24.0),
                    modifiers: Modifiers::NONE,
                },
                key_event,
            ],
            ..RawInput::default()
        };
        assert_eq!(
            dispatch(
                &egui::Context::default(),
                scroll_and_key,
                node_context(true)
            ),
            None,
        );
        let repeat_context = egui::Context::default();
        drop(repeat_context.run(key_input(Modifiers::NONE), |_| {}));
        assert_eq!(
            dispatch(
                &repeat_context,
                key_input_with_repeat(Modifiers::NONE, true),
                node_context(true),
            ),
            None,
        );
    }
}
