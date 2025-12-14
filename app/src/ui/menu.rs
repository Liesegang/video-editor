use eframe::egui::{self, Button};
use egui_dock::DockState;
use egui_phosphor::regular as icons;

use crate::command::{CommandId, CommandRegistry};
use crate::model::ui_types::Tab;

pub fn menu_bar(
    ui: &mut egui::Ui,
    command_registry: &CommandRegistry,
    dock_state: &mut DockState<Tab>,
    triggered_action: &mut Option<CommandId>,
) {
    egui::MenuBar::new().ui(ui, |ui| {
        file_menu(ui, command_registry, triggered_action);
        edit_menu(ui, command_registry, triggered_action);
        view_menu(ui, dock_state, command_registry, triggered_action);
    });
}

fn file_menu(
    ui: &mut egui::Ui,
    command_registry: &CommandRegistry,
    triggered_action: &mut Option<CommandId>,
) {
    ui.menu_button("File", |ui| {
        for cmd_id in [
            CommandId::NewProject,
            CommandId::LoadProject,
            CommandId::Save,
            CommandId::SaveAs,
            CommandId::Export,
            CommandId::Quit,
        ] {
            if let Some(cmd) = command_registry.find(cmd_id) {
                let icon = match cmd_id {
                    CommandId::NewProject => icons::FILE_PLUS,
                    CommandId::LoadProject => icons::FOLDER_OPEN,
                    CommandId::Save => icons::FLOPPY_DISK,
                    CommandId::SaveAs => icons::FLOPPY_DISK_BACK,
                    CommandId::Export => icons::EXPORT,
                    CommandId::Quit => icons::SIGN_OUT,
                    _ => unreachable!(), // Should not happen
                };
                let button = Button::new(egui::RichText::new(format!("{} {}", icon, cmd.text)))
                    .shortcut_text(cmd.shortcut_text.clone());
                if ui.add(button).clicked() {
                    *triggered_action = Some(cmd.id);
                    ui.close();
                }
            }
        }
    });
}

fn edit_menu(
    ui: &mut egui::Ui,
    command_registry: &CommandRegistry,
    triggered_action: &mut Option<CommandId>,
) {
    ui.menu_button("Edit", |ui| {
        for cmd_id in [
            CommandId::Undo,
            CommandId::Redo,
            CommandId::Delete,
            CommandId::Settings,
        ] {
            if let Some(cmd) = command_registry.find(cmd_id) {
                let button = Button::new(&cmd.text).shortcut_text(cmd.shortcut_text.clone());
                if ui.add(button).clicked() {
                    *triggered_action = Some(cmd.id);
                    ui.close();
                }
            }
        }
    });
}

fn view_menu(
    ui: &mut egui::Ui,
    dock_state: &mut DockState<Tab>,
    command_registry: &CommandRegistry,
    triggered_action: &mut Option<CommandId>,
) {
    ui.menu_button("View", |ui| {
        // Panel Toggles
        for tab in Tab::all() {
            let cmd_id = CommandId::TogglePanel(*tab);
            if let Some(cmd) = command_registry.find(cmd_id) {
                // Determine if tab is open by checking if it exists in the dock state
                let mut is_open = dock_state.find_tab(tab).is_some();

                // Show checkbox with command text.
                // Note: Checkbox doesn't inherently support right-aligned shortcut text like Button.
                // We rely on the Command system being the source of truth.
                if ui.checkbox(&mut is_open, &cmd.text).changed() {
                    *triggered_action = Some(cmd_id);
                }

                // Optional: Tooltip for shortcut hint if needed
                if !cmd.shortcut_text.is_empty() {
                    ui.ctx().set_cursor_icon(eframe::egui::CursorIcon::Default);
                    // This is hard to attach to the checkbox without complex layout.
                    // Skipping visual shortcut for now as per standard egui usage,
                    // or user can inspect settings.
                }
            }
        }

        ui.separator();

        if let Some(cmd) = command_registry.find(CommandId::ShowCommandPalette) {
             let button = Button::new(&cmd.text).shortcut_text(cmd.shortcut_text.clone());
            if ui.add(button).clicked() {
                *triggered_action = Some(cmd.id);
                ui.close();
            }
        }

        ui.separator();

        if let Some(cmd) = command_registry.find(CommandId::ResetLayout) {
            let button = Button::new(&cmd.text).shortcut_text(cmd.shortcut_text.clone());
            if ui.add(button).clicked() {
                *triggered_action = Some(cmd.id);
                ui.close();
            }
        }
    });
}
