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
            SelectionAction::Remove
        } else if modifiers.shift {
            SelectionAction::Add
        } else if modifiers.ctrl {
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
