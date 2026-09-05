use super::command_triggered;
use crate::command::{CommandId, CommandRegistry};
use crate::config::AppConfig;
use eframe::egui::{self, Event, Key, Modifiers, RawInput};

fn space_event(pressed: bool) -> RawInput {
    RawInput {
        events: vec![Event::Key {
            key: Key::Space,
            physical_key: Some(Key::Space),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        ..RawInput::default()
    }
}

#[test]
fn playback_shortcut_triggers_on_release_so_hold_to_pan_remains_available() {
    let registry = CommandRegistry::new(&AppConfig::new());
    let command = registry
        .find(CommandId::TogglePlayback)
        .expect("playback command");
    let context = egui::Context::default();
    let mut pressed_result = false;
    drop(context.run(space_event(true), |context| {
        pressed_result = command_triggered(context, command);
    }));
    assert!(!pressed_result);

    let mut released_result = false;
    drop(context.run(space_event(false), |context| {
        released_result = command_triggered(context, command);
    }));
    assert!(released_result);
}

#[test]
fn redo_shortcut_is_not_consumed_by_the_earlier_undo_command() {
    let registry = CommandRegistry::new(&AppConfig::new());
    let undo = registry.find(CommandId::Undo).expect("Undo command");
    let redo = registry.find(CommandId::Redo).expect("Redo command");
    let context = egui::Context::default();
    let mut undo_triggered = false;
    let mut redo_triggered = false;
    drop(context.run(
        RawInput {
            events: vec![Event::Key {
                key: Key::Z,
                physical_key: Some(Key::Z),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND | Modifiers::SHIFT,
            }],
            ..RawInput::default()
        },
        |context| {
            undo_triggered = command_triggered(context, undo);
            redo_triggered = command_triggered(context, redo);
        },
    ));
    assert!(!undo_triggered);
    assert!(redo_triggered);
}
