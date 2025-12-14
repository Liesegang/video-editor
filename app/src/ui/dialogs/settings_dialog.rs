use crate::command::{Command, CommandId, CommandRegistry};
use crate::{config, model::ui_types::SettingsTab};
use eframe::egui::{CollapsingHeader, ComboBox, RichText, ScrollArea, TextEdit, Ui, Key};
use egui_extras::{Column, TableBuilder};
use crate::config::AppConfig;
use library::plugin::PluginManager;
use std::sync::Arc;

#[derive(Clone)]
struct SettingsState {
    search_query: String,
    listening_for: Option<CommandId>,
    active_tab: SettingsTab,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            listening_for: None,
            active_tab: SettingsTab::Shortcuts,
        }
    }
}

// ... SettingsResult ...
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
    
    pub command_registry: CommandRegistry,
    pub config: AppConfig,
    pub plugin_manager: Arc<PluginManager>, // Added

    pub editing_registry: CommandRegistry,
    pub editing_config: AppConfig,
}

impl SettingsDialog {
    pub fn new(initial_registry: CommandRegistry, initial_config: AppConfig, plugin_manager: Arc<PluginManager>) -> Self {
        Self {
            is_open: false,
            show_close_warning: false,
            command_registry: initial_registry.clone(),
            config: initial_config.clone(),
            plugin_manager, // Added
            editing_registry: initial_registry,
            editing_config: initial_config,
        }
    }

    pub fn open(&mut self, current_registry: &CommandRegistry, current_config: &AppConfig) {
        self.command_registry = current_registry.clone();
        self.config = current_config.clone();
        
        self.editing_registry = current_registry.clone();
        self.editing_config = current_config.clone();
        
        self.is_open = true;
        self.show_close_warning = false;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> (bool, Option<SettingsResult>) {
        let mut is_listening_for_shortcut = false;
        let mut returned_result: Option<SettingsResult> = None;

        if self.is_open {
            let mut still_open = true;
            let mut close_confirmed = false;

            egui::Window::new("Settings")
                .open(&mut still_open)
                .min_width(600.0)
                .min_height(400.0)
                .resizable(true)
                .show(ctx, |ui| {
                    let output = settings_panel(
                        ui, 
                        &mut self.editing_registry,
                        &mut self.editing_config,
                        &self.plugin_manager
                    );
                    is_listening_for_shortcut = output.is_listening;

                    if let Some(result) = output.result {
                        returned_result = Some(result);
                        match result {
                            SettingsResult::Save => {
                                // 1. Update config shortcuts from editing registry
                                // We only save shortcuts that differ from the defaults.
                                // This allows us to persist "Unbound" (None) if the default was "Bound" (Some),
                                // while correctly inheriting future defaults if the user hasn't changed them.
                                let default_config = AppConfig::new();
                                let default_registry = CommandRegistry::new(&default_config);
                                
                                let mut shortcuts = std::collections::HashMap::new();
                                for cmd in &self.editing_registry.commands {
                                    let default_cmd = default_registry.find(cmd.id);
                                    let is_different = if let Some(def) = default_cmd {
                                        def.shortcut != cmd.shortcut
                                    } else {
                                        true 
                                    };

                                    if is_different {
                                        shortcuts.insert(cmd.id, cmd.shortcut);
                                    }
                                }
                                self.editing_config.shortcuts = shortcuts;

                                // 2. Save
                                config::save_config(&self.editing_config);
                                
                                // 3. Commit
                                self.command_registry = self.editing_registry.clone();
                                self.config = self.editing_config.clone();
                                
                                close_confirmed = true;
                            }
                            SettingsResult::Cancel => {
                                close_confirmed = true;
                            }
                            SettingsResult::RestoreDefaults => {
                                let default_config = AppConfig::new();
                                self.editing_config = default_config.clone();
                                self.editing_registry = CommandRegistry::new(&default_config);
                            }
                        }
                    }
                });

            if !still_open {
               close_confirmed = true;
            }

            if close_confirmed {
                self.is_open = false;
                self.show_close_warning = false;
            }
        }
        
        (is_listening_for_shortcut, returned_result)
    }
}

use egui_extras::{StripBuilder, Size};

// ... (existing helper functions)

fn settings_panel(
    ui: &mut Ui, 
    registry: &mut CommandRegistry, 
    config: &mut AppConfig,
    plugin_manager: &PluginManager,
) -> SettingsOutput {
    let mut result = None;

    let mut state = ui.memory_mut(|m| {
        m.data
            .get_temp_mut_or_default::<SettingsState>(ui.id())
            .clone()
    });

    // Use StripBuilder for the main layout: Sidebar (Fixed) | Content (Remainder)
    StripBuilder::new(ui)
        .size(Size::exact(150.0)) // Sidebar width
        .size(Size::remainder())  // Content width
        .horizontal(|mut strip| {
            // --- Sidebar (Tabs) ---
            strip.cell(|ui| {
                ui.add_space(8.0);
                ScrollArea::vertical().show(ui, |ui| { // Add scroll for sidebar
                    ui.vertical(|ui| {
                        if ui.selectable_label(matches!(state.active_tab, SettingsTab::Shortcuts), "Shortcuts").clicked() {
                            state.active_tab = SettingsTab::Shortcuts;
                        }
                        
                        let is_plugin_tab = matches!(state.active_tab, SettingsTab::PluginPaths | SettingsTab::PluginList(_, _));
                        
                        egui::collapsing_header::CollapsingHeader::new("Plugins")
                            .default_open(is_plugin_tab)
                            .show(ui, |ui| {
                                if ui.selectable_label(matches!(state.active_tab, SettingsTab::PluginPaths), "Paths").clicked() {
                                    state.active_tab = SettingsTab::PluginPaths;
                                }
                                
                                use library::plugin::PluginCategory;
                                
                                // Loaders
                                if ui.selectable_label(matches!(state.active_tab, SettingsTab::PluginList(PluginCategory::Load, _)), "Loaders").clicked() {
                                    state.active_tab = SettingsTab::PluginList(PluginCategory::Load, None);
                                }
                                
                                // Exporters
                                if ui.selectable_label(matches!(state.active_tab, SettingsTab::PluginList(PluginCategory::Export, _)), "Exporters").clicked() {
                                    state.active_tab = SettingsTab::PluginList(PluginCategory::Export, None);
                                }
                                
                                // Effects
                                let is_effect_tab = matches!(state.active_tab, SettingsTab::PluginList(PluginCategory::Effect, _));
                                egui::collapsing_header::CollapsingHeader::new("Effects")
                                    .default_open(is_effect_tab)
                                    .show(ui, |ui| {
                                         if ui.selectable_label(matches!(state.active_tab, SettingsTab::PluginList(PluginCategory::Effect, None)), "All").clicked() {
                                             state.active_tab = SettingsTab::PluginList(PluginCategory::Effect, None);
                                         }
                                         if ui.selectable_label(matches!(state.active_tab, SettingsTab::PluginList(PluginCategory::Effect, Some(ref s)) if s == "Native"), "CPU").clicked() {
                                             state.active_tab = SettingsTab::PluginList(PluginCategory::Effect, Some("Native".to_string()));
                                         }
                                         if ui.selectable_label(matches!(state.active_tab, SettingsTab::PluginList(PluginCategory::Effect, Some(ref s)) if s == "SkSL"), "SkSL").clicked() {
                                             state.active_tab = SettingsTab::PluginList(PluginCategory::Effect, Some("SkSL".to_string()));
                                         }
                                    });

                                // Entity Converters
                                if ui.selectable_label(matches!(state.active_tab, SettingsTab::PluginList(PluginCategory::EntityConverter, _)), "Entity Converters").clicked() {
                                    state.active_tab = SettingsTab::PluginList(PluginCategory::EntityConverter, None);
                                }
                            });

                        if ui.selectable_label(matches!(state.active_tab, SettingsTab::Theme), "Theme").clicked() {
                            state.active_tab = SettingsTab::Theme;
                        }
                    });
                });
            });

            // --- Content Area ---
            strip.cell(|ui| {
                ui.add_space(8.0); // Top padding
                
                // Vertical strip for Content vs Footer
                StripBuilder::new(ui)
                    .size(Size::remainder()) // Main tab content
                    .size(Size::exact(40.0)) // Footer buttons
                    .vertical(|mut strip| {
                        strip.cell(|ui| {
                            match &state.active_tab {
                                SettingsTab::Shortcuts => {
                                    shortcuts_tab(ui, &mut registry.commands, &mut state);
                                }
                                SettingsTab::PluginPaths => {
                                    plugins_paths_tab(ui, config);
                                }
                                SettingsTab::PluginList(category, filter) => {
                                    plugins_list_tab(ui, plugin_manager, category.clone(), filter.clone());
                                }
                                SettingsTab::Theme => {
                                    theme_tab(ui, config);
                                }
                            }
                        });

                        strip.cell(|ui| {
                            ui.separator();
                            ui.horizontal(|ui| {
                                if ui.button("Restore Defaults").clicked() {
                                    result = Some(SettingsResult::RestoreDefaults);
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                     if ui.button("Save").clicked() {
                                        result = Some(SettingsResult::Save);
                                    }
                                    if ui.button("Cancel").clicked() {
                                        result = Some(SettingsResult::Cancel);
                                    }
                                });
                            });
                        });
                    });
            });
        });

    let is_listening = state.listening_for.is_some();
    ui.memory_mut(|m| m.data.insert_temp(ui.id(), state));

    SettingsOutput {
        result,
        is_listening,
    }
}
// ... shortcuts_tab ...
// ... get_shortcut_text ...

// Rename plugins_tab to plugins_paths_tab
fn plugins_paths_tab(ui: &mut Ui, config: &mut AppConfig) {
    ui.heading("Plugin Paths");
    ui.add_space(5.0);
    ui.label("Configure paths where the editor looks for plugins.");
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        if ui.button("Add Path").clicked() {
             if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                 let path_str = folder.to_string_lossy().to_string();
                 if !config.plugins.paths.contains(&path_str) {
                     config.plugins.paths.push(path_str);
                 }
             }
        }
    });

    ui.separator();
    
    let mut to_remove = None;
    ScrollArea::vertical().show(ui, |ui| {
        for (i, path) in config.plugins.paths.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(path);
                if ui.button("Remove").clicked() {
                    to_remove = Some(i);
                }
            });
        }
    });

    if let Some(i) = to_remove {
        config.plugins.paths.remove(i);
    }
}

fn plugins_list_tab(ui: &mut Ui, plugin_manager: &PluginManager, category: library::plugin::PluginCategory, filter: Option<String>) {
    ui.heading(format!("Loaded Plugins: {:?}", category));
    if let Some(f) = &filter {
        ui.label(format!("Filter: {}", f));
    }
    ui.add_space(10.0);

    let all_plugins = plugin_manager.get_all_plugins();
    
    // Filter logic
    let filtered_plugins: Vec<_> = all_plugins.iter().filter(|p| {
        if p.plugin_type != category {
            return false;
        }
        if let Some(f) = &filter {
            if &p.impl_type != f {
                return false;
            }
        }
        true
    }).collect();

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(150.0)) // ID
        .column(Column::auto().at_least(150.0)) // Name
        .column(Column::auto().at_least(100.0)) // Type (Enum)
        .column(Column::auto().at_least(100.0)) // Category (String)
        .column(Column::auto().at_least(80.0))  // Impl (String)
        .column(Column::remainder())            // Version
        .header(20.0, |mut header| {
            header.col(|ui| { ui.strong("ID"); });
            header.col(|ui| { ui.strong("Name"); });
            header.col(|ui| { ui.strong("Type"); });
            header.col(|ui| { ui.strong("Category"); });
            header.col(|ui| { ui.strong("Impl"); });
            header.col(|ui| { ui.strong("Version"); });
        })
        .body(|mut body| {
            for plugin in filtered_plugins {
                 body.row(18.0, |mut row| {
                    row.col(|ui| { ui.label(&plugin.id); });
                    row.col(|ui| { ui.label(&plugin.name); });
                    row.col(|ui| { ui.label(format!("{:?}", plugin.plugin_type)); });
                    row.col(|ui| { ui.label(&plugin.category); });
                    row.col(|ui| { ui.label(&plugin.impl_type); });
                    row.col(|ui| { ui.label(&plugin.version); });
                });
            }
        });
}

fn shortcuts_tab(ui: &mut Ui, commands: &mut Vec<Command>, state: &mut SettingsState) {
    ui.heading("Shortcut Settings");
    ui.add_space(10.0);
    
    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.add(TextEdit::singleline(&mut state.search_query).hint_text("Search commands..."));
    });
    ui.add_space(10.0);

    let listening_for_id = state.listening_for;
    let mut new_shortcut: Option<(CommandId, eframe::egui::Modifiers, Key)> = None;

    if let Some(command_id) = listening_for_id {
         ui.ctx().input(|i| {
            // Filter keys so we don't catch modifiers alone
            if i.keys_down.is_empty() { return; }
             
            for event in &i.events {
                if let egui::Event::Key { key, pressed: true, .. } = event {

                     new_shortcut = Some((command_id, i.modifiers, *key));
                     break; 
                }
            }
        });
    }

    if let Some((id, mods, key)) = new_shortcut {
        if let Some(cmd) = commands.iter_mut().find(|c| c.id == id) {
             let new_shortcut_val = Some((mods, key));
             cmd.shortcut = new_shortcut_val;
             cmd.shortcut_text = get_shortcut_text(&new_shortcut_val);
        }
        state.listening_for = None;
    }

    // Table View
    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(200.0)) // Command Name
        .column(Column::remainder())             // Shortcut
        .header(20.0, |mut header| {
            header.col(|ui| { ui.strong("Command"); });
            header.col(|ui| { ui.strong("Shortcut"); });
        })
        .body(|mut body| {
            let query = state.search_query.to_lowercase();
            // Filter commands first to know row count? 
            // TableBuilder body iterates dynamically or we can feed it rows.
            // Using logic inside rows:

            for command in commands.iter_mut() {
                if !query.is_empty() && !command.text.to_lowercase().contains(&query) {
                    continue;
                }

                body.row(30.0, |mut row| {
                    row.col(|ui| {
                        ui.label(&command.text);
                    });
                    row.col(|ui| {
                        let is_listening = state.listening_for == Some(command.id);
                        
                        let button_text = if is_listening {
                            "Press keys...".to_string()
                        } else if !command.shortcut_text.is_empty() {
                            command.shortcut_text.clone()
                        } else {
                            "Set Shortcut".to_string()
                        };

                        if ui.button(button_text).clicked() {
                            if is_listening {
                                state.listening_for = None;
                            } else {
                                state.listening_for = Some(command.id);
                            }
                        }

                        if command.shortcut.is_some() && !is_listening {
                            if ui.button("❌").on_hover_text("Clear").clicked() {
                                command.shortcut = None;
                                command.shortcut_text = "".to_string();
                            }
                        }
                    });
                });
            }
        });
}

fn theme_tab(ui: &mut Ui, config: &mut AppConfig) {
    ui.heading("Theme Settings");
    ui.add_space(10.0);

    egui::ComboBox::from_label("Theme Mode")
        .selected_text(format!("{:?}", config.theme.theme_type))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut config.theme.theme_type, crate::config::ThemeType::Dark, "Dark");
            ui.selectable_value(&mut config.theme.theme_type, crate::config::ThemeType::Light, "Light");
            ui.selectable_value(&mut config.theme.theme_type, crate::config::ThemeType::Latte, "Latte");
            ui.selectable_value(&mut config.theme.theme_type, crate::config::ThemeType::Frappe, "Frappe");
            ui.selectable_value(&mut config.theme.theme_type, crate::config::ThemeType::Macchiato, "Macchiato");
            ui.selectable_value(&mut config.theme.theme_type, crate::config::ThemeType::Mocha, "Mocha");
        });


}


fn get_shortcut_text(shortcut: &Option<(eframe::egui::Modifiers, Key)>) -> String {
    if let Some((m, k)) = shortcut {
        let mut parts = Vec::new();
        if m.command { parts.push("Ctrl"); } // Simplified for cross-platform visual
        if m.ctrl && !m.command { parts.push("Ctrl"); }
        if m.shift { parts.push("Shift"); }
        if m.alt { parts.push("Alt"); }
        let key_str = format!("{:?}", k);
        parts.push(&key_str);
        parts.join("+")
    } else {
        String::new()
    }
}

