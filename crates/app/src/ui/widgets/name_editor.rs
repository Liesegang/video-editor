//! One commit-on-finish name control for Inspector and Node context menus.
//! The host owns the draft and validates its domain; typing never edits a model.

use egui::{Id, Key, Response, Ui};

pub(crate) struct NameEdit {
    pub response: Response,
    pub value: Option<String>,
    pub error: Option<String>,
    pub cancelled: bool,
}

pub(crate) fn name_editor(
    ui: &mut Ui,
    id: Id,
    draft: &mut String,
    source: &str,
    width: f32,
    validate: impl FnOnce(&str) -> Result<(), String>,
) -> NameEdit {
    // Consume Escape before TextEdit and the enclosing popup interpret it.
    // Cancellation must not become a lost-focus commit in this frame.
    // egui's frame-begin focus navigation may already have handled Escape.
    let cancelled = ui.memory(|memory| memory.has_focus(id) || memory.had_focus_last_frame(id))
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Escape));
    if cancelled {
        source.clone_into(draft);
        ui.memory_mut(|memory| memory.surrender_focus(id));
    }
    let response = ui
        .horizontal(|ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(draft)
                    .id(id)
                    .desired_width(width),
            )
        })
        .inner;
    let error = validate(draft.trim()).err();
    if let Some(error) = &error {
        ui.add(
            egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color)).wrap(),
        );
    }
    let finished = !cancelled
        && (response.lost_focus()
            || (response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter))));
    let value =
        (finished && error.is_none() && draft.trim() != source).then(|| draft.trim().to_string());
    NameEdit {
        response,
        value,
        error,
        cancelled,
    }
}

#[cfg(test)]
mod tests;
