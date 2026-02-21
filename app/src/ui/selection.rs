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
        if modifiers.shift && modifiers.command {
            SelectionAction::Remove
        } else if modifiers.shift {
            SelectionAction::Add
        } else if modifiers.command {
            SelectionAction::Toggle
        } else {
            SelectionAction::Replace
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ClickAction<T> {
    Select(T), // Replace selection with item
    Add(T),    // Add item to selection (ensure selected)
    Remove(T), // Remove item from selection (ensure deselected)
    Toggle(T), // Toggle item selection
    Clear,     // Clear selection
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
            _ => ClickAction::DoNothing,
        },
    }
}

#[derive(Debug, PartialEq)]
pub enum BoxAction<T> {
    Replace(Vec<T>),
    Add(Vec<T>),
    Remove(Vec<T>),
}

pub fn get_box_action<T>(modifiers: &Modifiers, items_in_box: Vec<T>) -> BoxAction<T> {
    match SelectionAction::from_modifiers(modifiers) {
        SelectionAction::Replace => BoxAction::Replace(items_in_box),
        SelectionAction::Add => BoxAction::Add(items_in_box),
        SelectionAction::Toggle => BoxAction::Add(items_in_box), // Toggle usually adds in box selection too? Or toggles each?
        // User said "Ctrl=Toggle". For box selection, Toggle often means XOR.
        // But usually standard behavior is "Add" or "Toggle Invert".
        // Let's assume Box Toggle ~ Add (or Invert).
        // For now, I'll map Toggle to Add for Box, as "Toggle" box is rare/complex to define (flip all?).
        // Wait, if I map Toggle to Add, then Ctrl+Drag adds.
        // User request: "Shift... Add, Ctrl toggle, CtrlShift remove".
        // "Ctrl toggle" might imply XOR.
        // But previously `SelectionAction::Toggle => BoxAction::Add`.
        // So I will keep `Toggle => BoxAction::Add` for now unless "XOR" is requested.
        // Note: `Remove` is explicitly requested.
        SelectionAction::Remove => BoxAction::Remove(items_in_box),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;

    // ── Domain: SelectionAction (modifier → action mapping) ──

    #[test]
    fn no_modifiers_yields_replace() {
        let action = SelectionAction::from_modifiers(&Modifiers::NONE);
        assert_eq!(action, SelectionAction::Replace);
    }

    #[test]
    fn shift_yields_add() {
        let action = SelectionAction::from_modifiers(&Modifiers::SHIFT);
        assert_eq!(action, SelectionAction::Add);
    }

    #[test]
    fn command_yields_toggle() {
        let mods = Modifiers {
            command: true,
            ..Modifiers::NONE
        };
        let action = SelectionAction::from_modifiers(&mods);
        assert_eq!(action, SelectionAction::Toggle);
    }

    #[test]
    fn shift_command_yields_remove() {
        let mods = Modifiers {
            shift: true,
            command: true,
            ..Modifiers::NONE
        };
        let action = SelectionAction::from_modifiers(&mods);
        assert_eq!(action, SelectionAction::Remove);
    }

    // ── Domain: ClickAction (modifier + hover → click behavior) ──

    #[test]
    fn click_item_no_modifiers_selects() {
        let action = get_click_action(&Modifiers::NONE, Some(42));
        assert_eq!(action, ClickAction::Select(42));
    }

    #[test]
    fn click_item_shift_adds() {
        let action = get_click_action(&Modifiers::SHIFT, Some(42));
        assert_eq!(action, ClickAction::Add(42));
    }

    #[test]
    fn click_item_command_toggles() {
        let mods = Modifiers {
            command: true,
            ..Modifiers::NONE
        };
        let action = get_click_action(&mods, Some(42));
        assert_eq!(action, ClickAction::Toggle(42));
    }

    #[test]
    fn click_item_shift_command_removes() {
        let mods = Modifiers {
            shift: true,
            command: true,
            ..Modifiers::NONE
        };
        let action = get_click_action(&mods, Some(42));
        assert_eq!(action, ClickAction::Remove(42));
    }

    #[test]
    fn click_empty_no_modifiers_clears() {
        let action: ClickAction<i32> = get_click_action(&Modifiers::NONE, None);
        assert_eq!(action, ClickAction::Clear);
    }

    #[test]
    fn click_empty_shift_does_nothing() {
        let action: ClickAction<i32> = get_click_action(&Modifiers::SHIFT, None);
        assert_eq!(action, ClickAction::DoNothing);
    }

    #[test]
    fn click_empty_command_does_nothing() {
        let mods = Modifiers {
            command: true,
            ..Modifiers::NONE
        };
        let action: ClickAction<i32> = get_click_action(&mods, None);
        assert_eq!(action, ClickAction::DoNothing);
    }

    // ── Domain: BoxAction (modifier + box selection → action) ──

    #[test]
    fn box_no_modifiers_replaces() {
        let action = get_box_action(&Modifiers::NONE, vec![1, 2, 3]);
        assert_eq!(action, BoxAction::Replace(vec![1, 2, 3]));
    }

    #[test]
    fn box_shift_adds() {
        let action = get_box_action(&Modifiers::SHIFT, vec![1, 2]);
        assert_eq!(action, BoxAction::Add(vec![1, 2]));
    }

    #[test]
    fn box_shift_command_removes() {
        let mods = Modifiers {
            shift: true,
            command: true,
            ..Modifiers::NONE
        };
        let action = get_box_action(&mods, vec![1, 2]);
        assert_eq!(action, BoxAction::Remove(vec![1, 2]));
    }
}
