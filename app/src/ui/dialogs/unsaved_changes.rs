use egui_phosphor::regular as icons;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardedProjectAction {
    NewProject,
    OpenProject,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

pub fn show(
    context: &egui::Context,
    project_name: &str,
    action: GuardedProjectAction,
) -> Option<UnsavedChoice> {
    let mut choice = None;
    crate::ui::widgets::modal::Modal::new("Unsaved changes")
        .resizable(false)
        .min_width(390.0)
        .show(context, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(icons::WARNING)
                        .size(28.0)
                        .color(egui::Color32::from_rgb(245, 181, 64)),
                );
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(project_name).strong());
                    ui.label(format!(
                        "Save your changes before {}?",
                        action_label(action)
                    ));
                });
            });
            ui.add_space(12.0);
            crate::ui::dialogs::dialog_footer(ui, |ui| {
                let save = ui.button(format!("{} Save", icons::FLOPPY_DISK));
                crate::qa::register_component("unsaved.save", "unsaved_dialog_button", save.rect);
                if save.clicked() {
                    choice = Some(UnsavedChoice::Save);
                }
                let discard = ui.button(format!("{} Discard", icons::TRASH));
                crate::qa::register_component(
                    "unsaved.discard",
                    "unsaved_dialog_button",
                    discard.rect,
                );
                if discard.clicked() {
                    choice = Some(UnsavedChoice::Discard);
                }
                let cancel = ui.button("Cancel");
                crate::qa::register_component(
                    "unsaved.cancel",
                    "unsaved_dialog_button",
                    cancel.rect,
                );
                if cancel.clicked() {
                    choice = Some(UnsavedChoice::Cancel);
                }
            });
        });
    choice
}

const fn action_label(action: GuardedProjectAction) -> &'static str {
    match action {
        GuardedProjectAction::NewProject => "creating a new project",
        GuardedProjectAction::OpenProject => "opening another project",
        GuardedProjectAction::Quit => "closing RuViE",
    }
}
