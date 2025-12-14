use egui::Modifiers;

pub enum SelectionAction {
    Replace,
    Add,
    Toggle,
}

impl SelectionAction {
    pub fn from_modifiers(modifiers: &Modifiers) -> Self {
        if modifiers.shift {
            // In typical design apps, Shift adds to selection (or toggles range).
            // Here we treat Shift and Ctrl similarly as Toggle/Add for simplicity based on existing logic.
            SelectionAction::Toggle
        } else if modifiers.ctrl {
            SelectionAction::Toggle
        } else {
            SelectionAction::Replace
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ClickAction<T> {
    Select(T),
    Toggle(T),
    Clear,
    DoNothing,
}

pub fn get_click_action<T>(modifiers: &Modifiers, hovered_item: Option<T>) -> ClickAction<T> {
    match hovered_item {
        Some(item) => match SelectionAction::from_modifiers(modifiers) {
            SelectionAction::Replace => ClickAction::Select(item),
            SelectionAction::Add | SelectionAction::Toggle => ClickAction::Toggle(item),
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
}

pub fn get_box_action<T>(modifiers: &Modifiers, items_in_box: Vec<T>) -> BoxAction<T> {
    match SelectionAction::from_modifiers(modifiers) {
        SelectionAction::Replace => BoxAction::Replace(items_in_box),
        SelectionAction::Add | SelectionAction::Toggle => BoxAction::Add(items_in_box),
    }
}
