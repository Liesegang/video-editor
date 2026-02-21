use crate::command::{Command, CommandId, CommandRegistry};
use eframe::egui::{self, Align2, Key, RichText, Window};

pub struct CommandPalette {
    pub is_open: bool,
    query: String,
    selected_index: usize,
    filtered_commands: Vec<Command>,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            is_open: false,
            query: String::new(),
            selected_index: 0,
            filtered_commands: Vec::new(),
        }
    }

    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
        if self.is_open {
            self.query.clear();
            self.selected_index = 0;
            // potential optimization: don't filter immediately, wait for update or pre-fill
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, registry: &CommandRegistry) -> Option<CommandId> {
        if !self.is_open {
            return None;
        }

        let mut executed_command = None;

        // Filter commands if query changed or just opened.
        // For simplicity, we can do it every frame or cache it.
        // Given the small number of commands, every frame is likely fine for now,
        // but let's be slightly smarter.
        // Actually, for immediate feedback, filtering every frame is standard.
        // Let's optimize by only constructing the vector when needed if list gets large.

        // Simple case-insensitive search
        self.filtered_commands = registry
            .commands
            .iter()
            .filter(|cmd| {
                if self.query.is_empty() {
                    return true;
                }
                cmd.text.to_lowercase().contains(&self.query.to_lowercase())
            })
            .cloned()
            .collect();

        // Clamp selected index
        if !self.filtered_commands.is_empty() {
            if self.selected_index >= self.filtered_commands.len() {
                self.selected_index = self.filtered_commands.len() - 1;
            }
        } else {
            self.selected_index = 0;
        }

        let window_height = 300.0;
        let window_width = 500.0;

        let mut open = self.is_open;
        Window::new("Command Palette")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, [0.0, 100.0])
            .fixed_size([window_width, window_height])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                // Search Input
                let text_edit = egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Type a command...")
                    .desired_width(f32::INFINITY)
                    .lock_focus(true); // Keep focus

                let output = ui.add(text_edit);

                // Auto-focus when opened
                if output.lost_focus() && ctx.input(|i| i.key_pressed(Key::Escape)) {
                    self.is_open = false;
                } else {
                    output.request_focus();
                }

                // Keyboard Navigation
                if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
                    if !self.filtered_commands.is_empty() {
                        self.selected_index =
                            (self.selected_index + 1) % self.filtered_commands.len();
                    }
                }
                if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
                    if !self.filtered_commands.is_empty() {
                        if self.selected_index == 0 {
                            self.selected_index = self.filtered_commands.len() - 1;
                        } else {
                            self.selected_index -= 1;
                        }
                    }
                }
                if ctx.input(|i| i.key_pressed(Key::Enter)) {
                    if let Some(cmd) = self.filtered_commands.get(self.selected_index) {
                        executed_command = Some(cmd.id);
                        self.is_open = false;
                    }
                }

                ui.separator();

                // Results List
                egui::ScrollArea::vertical()
                    .max_height(window_height - 60.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0; // Compact list
                        for (i, cmd) in self.filtered_commands.iter().enumerate() {
                            let is_selected = i == self.selected_index;

                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 24.0),
                                egui::Sense::click(),
                            );

                            // Handle clicking
                            if response.clicked() {
                                executed_command = Some(cmd.id);
                                self.is_open = false;
                            }

                            // Background for selected item
                            if is_selected {
                                ui.painter()
                                    .rect_filled(rect, 2.0, ui.visuals().selection.bg_fill);
                            }

                            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.add_space(8.0);
                                        let text_color = if is_selected {
                                            ui.visuals().selection.stroke.color
                                        } else {
                                            ui.visuals().text_color()
                                        };

                                        ui.label(RichText::new(&cmd.text).color(text_color));

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.add_space(8.0);
                                                if !cmd.shortcut_text.is_empty() {
                                                    ui.label(
                                                        RichText::new(&cmd.shortcut_text)
                                                            .weak()
                                                            .color(text_color),
                                                    );
                                                }
                                            },
                                        );
                                    },
                                );
                            });

                            // Ensure selected item is visible
                            if is_selected {
                                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                            }
                        }
                        if self.filtered_commands.is_empty() {
                            ui.label("No commands found.");
                        }
                    });
            });

        if !open {
            self.is_open = false;
        }

        executed_command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Domain: Toggle Lifecycle ──

    #[test]
    fn toggle_opens_when_closed() {
        let mut palette = CommandPalette::new();
        assert!(!palette.is_open);

        palette.toggle();

        assert!(palette.is_open);
    }

    #[test]
    fn toggle_closes_when_open() {
        let mut palette = CommandPalette::new();
        palette.toggle(); // open
        palette.toggle(); // close

        assert!(!palette.is_open);
    }

    #[test]
    fn toggle_clears_query_on_reopen() {
        let mut palette = CommandPalette::new();
        palette.toggle(); // open
        palette.query = "test query".to_string();
        palette.toggle(); // close
        palette.toggle(); // reopen

        assert!(
            palette.query.is_empty(),
            "Query should be cleared on reopen"
        );
    }

    #[test]
    fn toggle_resets_selected_index_on_reopen() {
        let mut palette = CommandPalette::new();
        palette.toggle(); // open
        palette.selected_index = 5;
        palette.toggle(); // close
        palette.toggle(); // reopen

        assert_eq!(palette.selected_index, 0, "Index should reset on reopen");
    }
}
