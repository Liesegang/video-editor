//! Frame-local input snapshot shared by editor interaction policies.

use egui::{Modifiers, Pos2};

#[derive(Clone, Copy)]
pub(crate) struct InteractionInput {
    pub(crate) pressed: bool,
    pub(crate) down: bool,
    pub(crate) released: bool,
    pub(crate) has_pointer: bool,
    pub(crate) pointer: Option<Pos2>,
    pub(crate) press_position: Option<Pos2>,
    pub(crate) press_modifiers: Modifiers,
    pub(crate) a_down: bool,
    pub(crate) a_down_at_press: bool,
    pub(crate) space_down: bool,
    pub(crate) middle_down: bool,
    pub(crate) focused: bool,
    pub(crate) escape: bool,
    pub(crate) pointer_released_before_a: bool,
}

pub(crate) fn interaction_input(ui: &egui::Ui) -> InteractionInput {
    ui.input(|input| {
        let primary_press =
            input
                .events
                .iter()
                .enumerate()
                .find_map(|(index, event)| match event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers,
                    } => Some((index, *pos, *modifiers)),
                    _ => None,
                });
        InteractionInput {
            pressed: input.pointer.primary_pressed(),
            down: input.pointer.primary_down(),
            released: input.pointer.primary_released(),
            has_pointer: input.pointer.has_pointer(),
            pointer: input.pointer.interact_pos(),
            press_position: primary_press
                .map(|(_, position, _)| position)
                .or_else(|| input.pointer.interact_pos()),
            press_modifiers: primary_press.map_or(input.modifiers, |(_, _, modifiers)| modifiers),
            a_down: input.key_down(egui::Key::A),
            a_down_at_press: primary_press.is_some_and(|(index, _, _)| {
                key_down_before_event(
                    egui::Key::A,
                    index,
                    input.key_down(egui::Key::A),
                    &input.events,
                )
            }),
            space_down: input.key_down(egui::Key::Space),
            middle_down: input.pointer.middle_down(),
            focused: input.focused,
            escape: input.key_pressed(egui::Key::Escape),
            pointer_released_before_a: pointer_release_precedes_a_release(&input.events),
        }
    })
}

fn key_down_before_event(
    key: egui::Key,
    event_index: usize,
    final_state: bool,
    events: &[egui::Event],
) -> bool {
    events
        .iter()
        .skip(event_index + 1)
        .rev()
        .fold(final_state, |down, event| match event {
            egui::Event::Key {
                key: event_key,
                pressed: true,
                repeat: false,
                ..
            } if *event_key == key => false,
            egui::Event::Key {
                key: event_key,
                pressed: false,
                ..
            } if *event_key == key => true,
            _ => down,
        })
}

fn pointer_release_precedes_a_release(events: &[egui::Event]) -> bool {
    let pointer_release = events.iter().position(|event| {
        matches!(
            event,
            egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed: false,
                ..
            }
        )
    });
    let a_release = events.iter().position(|event| {
        matches!(
            event,
            egui::Event::Key {
                key: egui::Key::A,
                pressed: false,
                ..
            }
        )
    });
    matches!((pointer_release, a_release), (Some(pointer), Some(a)) if pointer < a)
}
