//! Selection gestures shared by editor surfaces.
//!
//! Preview, Timeline, and other panels must agree on modifier semantics so a
//! click never changes meaning when the same object is manipulated elsewhere.

use egui::Modifiers;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SelectionAction {
    Replace,
    Add,
    Remove,
    Toggle,
}

impl SelectionAction {
    pub fn from_modifiers(modifiers: &Modifiers) -> Self {
        if modifiers.shift && modifiers.ctrl {
            Self::Remove
        } else if modifiers.shift {
            Self::Add
        } else if modifiers.ctrl || modifiers.command {
            Self::Toggle
        } else {
            Self::Replace
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ClickAction<T> {
    Select(T),
    Add(T),
    Remove(T),
    Toggle(T),
    Clear,
    DoNothing,
}

pub fn get_click_action<T>(modifiers: &Modifiers, hovered_item: Option<T>) -> ClickAction<T> {
    match hovered_item {
        Some(item) => match SelectionAction::from_modifiers(modifiers) {
            SelectionAction::Replace => ClickAction::Select(item),
            SelectionAction::Add => ClickAction::Add(item),
            SelectionAction::Remove => ClickAction::Remove(item),
            SelectionAction::Toggle => ClickAction::Toggle(item),
        },
        None => match SelectionAction::from_modifiers(modifiers) {
            SelectionAction::Replace => ClickAction::Clear,
            SelectionAction::Add | SelectionAction::Remove | SelectionAction::Toggle => {
                ClickAction::DoNothing
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_plain_click_clears_but_modified_click_preserves_selection() {
        assert_eq!(
            get_click_action::<u8>(&Modifiers::default(), None),
            ClickAction::Clear
        );
        assert_eq!(
            get_click_action::<u8>(
                &Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
                None,
            ),
            ClickAction::DoNothing
        );
    }

    #[test]
    fn platform_command_and_control_both_toggle() {
        for modifiers in [
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            Modifiers {
                command: true,
                ..Modifiers::default()
            },
        ] {
            assert_eq!(
                get_click_action(&modifiers, Some(7_u8)),
                ClickAction::Toggle(7)
            );
        }
    }
}
