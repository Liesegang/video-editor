use egui_phosphor::regular as icons;

use crate::app::guarded_action::{GuardedProjectAction, UnsavedChoice};
use crate::ui::dialogs::{dialog_button, dialog_footer, DialogButtonRole};

const DIALOG_WIDTH: f32 = 440.0;

pub fn show(
    context: &egui::Context,
    project_name: &str,
    action: GuardedProjectAction,
) -> Option<UnsavedChoice> {
    let mut choice = None;
    let escape_requested = context.input(|input| input.key_pressed(egui::Key::Escape));
    let shown = crate::ui::widgets::modal::Modal::dialog("Unsaved Changes", DIALOG_WIDTH).show(
        context,
        |ui| {
            let body = ui
                .horizontal_top(|ui| {
                    let warning = ui.add_sized(
                        egui::vec2(44.0, 44.0),
                        egui::Label::new(
                            egui::RichText::new(icons::WARNING)
                                .size(32.0)
                                .color(egui::Color32::from_rgb(245, 181, 64)),
                        ),
                    );
                    crate::qa::register_component(
                        "unsaved.warning",
                        "unsaved_dialog_warning",
                        warning.rect,
                    );
                    ui.add_space(8.0);
                    let content = ui.vertical(|ui| {
                        ui.set_max_width(DIALOG_WIDTH - 84.0);
                        ui.label(egui::RichText::new(project_name).strong().size(15.0));
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!(
                                    "Save your changes before {}?",
                                    action.progress_description()
                                ))
                                .color(ui.visuals().weak_text_color()),
                            )
                            .wrap(),
                        );
                    });
                    crate::qa::register_component(
                        "unsaved.content",
                        "unsaved_dialog_content",
                        content.response.rect,
                    );
                })
                .response;
            crate::qa::register_component("unsaved.body", "unsaved_dialog_body", body.rect);

            let footer = dialog_footer(ui, |ui| {
                let cancel = dialog_button(ui, "Cancel", DialogButtonRole::Secondary);
                crate::qa::register_component_with_metadata(
                    "unsaved.cancel",
                    "unsaved_dialog_button",
                    cancel.rect,
                    true,
                    Some(serde_json::json!({"role": "secondary", "order": 2})),
                );
                if cancel.clicked() {
                    choice = Some(UnsavedChoice::Cancel);
                }
                let discard = dialog_button(
                    ui,
                    format!("{} Discard", icons::TRASH),
                    DialogButtonRole::Destructive,
                );
                crate::qa::register_component_with_metadata(
                    "unsaved.discard",
                    "unsaved_dialog_button",
                    discard.rect,
                    true,
                    Some(serde_json::json!({"role": "destructive", "order": 1})),
                );
                if discard.clicked() {
                    choice = Some(UnsavedChoice::Discard);
                }
                let save = dialog_button(
                    ui,
                    format!("{} Save", icons::FLOPPY_DISK),
                    DialogButtonRole::Primary,
                );
                crate::qa::register_component_with_metadata(
                    "unsaved.save",
                    "unsaved_dialog_button",
                    save.rect,
                    true,
                    Some(serde_json::json!({"role": "primary", "order": 0})),
                );
                if save.clicked() {
                    choice = Some(UnsavedChoice::Save);
                }
            });
            crate::qa::register_component(
                "unsaved.footer",
                "unsaved_dialog_footer",
                footer.response.rect,
            );
        },
    );
    if let Some(shown) = shown {
        crate::qa::register_component_with_metadata(
            "unsaved.dialog",
            "unsaved_dialog",
            shown.response.rect,
            true,
            Some(serde_json::json!({
                "action": action_id(action),
                "project_name": project_name,
                "content_width": DIALOG_WIDTH,
            })),
        );
    }
    if choice.is_none() && escape_requested {
        choice = Some(UnsavedChoice::Cancel);
    }
    choice
}

const fn action_id(action: GuardedProjectAction) -> &'static str {
    match action {
        GuardedProjectAction::NewProject => "new_project",
        GuardedProjectAction::OpenProject => "open_project",
        GuardedProjectAction::Quit => "quit",
    }
}
