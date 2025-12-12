use crate::command::{Command, CommandId, CommandRegistry};
use crate::config;
use eframe::egui::{self, Key, ScrollArea, TextEdit, Ui};

#[derive(Clone, Default)]
struct SettingsState {
    search_query: String,
    listening_for: Option<CommandId>,
}

#[derive(Debug, Clone, Copy)]
pub enum SettingsResult {
    Save,
    Cancel,
    RestoreDefaults,
}

pub struct SettingsOutput {
    pub result: Option<SettingsResult>,
    pub is_listening: bool,
}

pub struct SettingsDialog {
    pub is_open: bool,
    pub show_close_warning: bool,
    pub command_registry: CommandRegistry, // The main registry (committed)
    pub editing_registry: CommandRegistry, // The temporary registry being edited
}

impl SettingsDialog {
    pub fn new(initial_registry: CommandRegistry) -> Self {
        Self {
            is_open: false,
            show_close_warning: false,
            command_registry: initial_registry.clone(),
            editing_registry: initial_registry,
        }
    }

    pub fn open(&mut self, current_registry: &CommandRegistry) {
        self.command_registry = current_registry.clone();
        self.editing_registry = current_registry.clone();
        self.is_open = true;
        self.show_close_warning = false;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut is_listening_for_shortcut = false;

        if self.is_open {
            let mut still_open = true;
            let mut close_confirmed = false;

            egui::Window::new("Settings")
                .open(&mut still_open)
                .vscroll(true)
                .show(ctx, |ui| {
                    let output = settings_panel(ui, &mut self.editing_registry.commands);
                    is_listening_for_shortcut = output.is_listening;

                    if let Some(result) = output.result {
                        match result {
                            SettingsResult::Save => {
                                self.command_registry = self.editing_registry.clone();
                                let mut shortcuts = std::collections::HashMap::new();
                                for cmd in &self.command_registry.commands {
                                    if let Some(shortcut) = cmd.shortcut {
                                        shortcuts.insert(cmd.id, shortcut);
                                    }
                                }
                                let config = config::ShortcutConfig { shortcuts };
                                config::save_config(&config);
                                close_confirmed = true;
                            }
                            SettingsResult::Cancel => {
                                if self.editing_registry != self.command_registry {
                                    self.show_close_warning = true;
                                } else {
                                    close_confirmed = true;
                                }
                            }
                            SettingsResult::RestoreDefaults => {
                                self.editing_registry =
                                    CommandRegistry::new(&config::ShortcutConfig::new());
                            }
                        }
                    }
                });

            if !still_open {
                // 'x' button was clicked
                if self.editing_registry != self.command_registry {
                    self.show_close_warning = true;
                } else {
                    close_confirmed = true;
                }
            }

            if close_confirmed {
                self.is_open = false;
                self.show_close_warning = false;
            }
        }

        // Unsaved Changes Dialog
        if self.show_close_warning {
            egui::Window::new("Unsaved Changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes. Are you sure you want to discard them?");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Discard").clicked() {
                            self.is_open = false;
                            self.show_close_warning = false;
                        }
                        if ui.button("Go Back").clicked() {
                            self.show_close_warning = false;
                        }
                    });
                });
        }

        is_listening_for_shortcut
    }
}

fn settings_panel(ui: &mut Ui, commands: &mut Vec<Command>) -> SettingsOutput {
    let mut result = None;

    // Retain state between frames
    let mut state = ui.memory_mut(|m| {
        m.data
            .get_temp_mut_or_default::<SettingsState>(ui.id())
            .clone()
    });

    ui.heading("Shortcut Settings");
    ui.add_space(10.0);
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.add(TextEdit::singleline(&mut state.search_query).hint_text("Search commands..."));
    });
    ui.add_space(10.0);
    ui.separator();

    let listening_for_id = state.listening_for;
    let mut new_shortcut: Option<(CommandId, eframe::egui::Modifiers, Key)> = None;

    // --- Input listening logic ---
    if let Some(command_id) = listening_for_id {
        ui.ctx().input(|i| {
            for key in i.events.iter().filter_map(|e| match e {
                eframe::egui::Event::Key {
                    key, pressed: true, ..
                } => Some(*key),
                _ => None,
            }) {
                new_shortcut = Some((command_id, i.modifiers, key));
                break;
            }
        });
    }

    if let Some((id, mods, key)) = new_shortcut {
        if let Some(cmd) = commands.iter_mut().find(|c| c.id == id) {
            let new_shortcut_val = Some((mods, key));
            cmd.shortcut = new_shortcut_val;
            cmd.shortcut_text = get_shortcut_text(&new_shortcut_val);
        }
        state.listening_for = None; // Stop listening
    }

    // --- UI Drawing ---
    ScrollArea::vertical().show(ui, |ui| {
        for command in &mut *commands {
            if state.listening_for.is_some() && state.listening_for != Some(command.id) {
                // Dim other commands when listening
                ui.add_enabled_ui(false, |ui| {
                    draw_command_row(ui, command, &mut state);
                });
            } else {
                draw_command_row(ui, command, &mut state);
            }
        }
    });

    let is_listening = state.listening_for.is_some();

    // Persist state for next frame
    ui.memory_mut(|m| m.data.insert_temp(ui.id(), state));

    // --- Action Buttons ---
    ui.add_space(10.0);
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Restore Defaults").clicked() {
            result = Some(SettingsResult::RestoreDefaults);
        }
        ui.with_layout(
            eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
            |ui| {
                if ui.button("Save").clicked() {
                    result = Some(SettingsResult::Save);
                }
                if ui.button("Cancel").clicked() {
                    result = Some(SettingsResult::Cancel);
                }
            },
        );
    });

    SettingsOutput {
        result,
        is_listening,
    }
}

fn draw_command_row(ui: &mut Ui, command: &mut Command, state: &mut SettingsState) {
    if !state.search_query.is_empty()
        && !command
            .text
            .to_lowercase()
            .contains(&state.search_query.to_lowercase())
    {
        return;
    }

    ui.horizontal(|ui| {
        ui.label(&command.text);
        ui.with_layout(
            eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
            |ui| {
                let is_listening = state.listening_for == Some(command.id);

                if command.shortcut.is_some() && !is_listening {
                    if ui.button("x").on_hover_text("Clear shortcut").clicked() {
                        command.shortcut = None;
                        command.shortcut_text = get_shortcut_text(&None);
                    }
                }

                let button_text = if is_listening {
                    "Press new shortcut...".to_string()
                } else if command.shortcut_text.is_empty() {
                    "Set Shortcut".to_string()
                } else {
                    command.shortcut_text.clone()
                };

                if ui.button(button_text).clicked() {
                    if is_listening {
                        state.listening_for = None; // Cancel listening
                    } else {
                        state.listening_for = Some(command.id); // Start listening
                    }
                }
            },
        );
    });
    ui.separator();
}

fn get_shortcut_text(shortcut: &Option<(eframe::egui::Modifiers, Key)>) -> String {
    if let Some((m, k)) = shortcut {
        let mut parts = Vec::new();
        if m.command {
            parts.push("Ctrl");
        }
        if m.shift {
            parts.push("Shift");
        }
        if m.alt {
            parts.push("Alt");
        }
        let key_str = format!("{:?}", k);
        parts.push(&key_str);
        parts.join("+")
    } else {
        String::new()
    }
}
