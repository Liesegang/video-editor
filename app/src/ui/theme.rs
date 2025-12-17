use eframe::egui;
use crate::config::{AppConfig, ThemeType};

pub fn apply_theme(ctx: &egui::Context, config: &AppConfig) {
    match config.theme.theme_type {
        ThemeType::Dark => ctx.set_visuals(egui::Visuals::dark()),
        ThemeType::Light => ctx.set_visuals(egui::Visuals::light()),
        _ => {
            let flavor = match config.theme.theme_type {
                ThemeType::Latte => catppuccin::PALETTE.latte,
                ThemeType::Frappe => catppuccin::PALETTE.frappe,
                ThemeType::Macchiato => catppuccin::PALETTE.macchiato,
                ThemeType::Mocha => catppuccin::PALETTE.mocha,
                _ => catppuccin::PALETTE.mocha,
            };

            let colors = flavor.colors;

            let mut visuals = if config.theme.theme_type == ThemeType::Latte {
                egui::Visuals::light()
            } else {
                egui::Visuals::dark()
            };

            let c = |c: catppuccin::Color| egui::Color32::from_rgb(c.rgb.r, c.rgb.g, c.rgb.b);

            visuals.panel_fill = c(colors.base);
            visuals.window_fill = c(colors.mantle);
            visuals.faint_bg_color = c(colors.surface0);
            visuals.extreme_bg_color = c(colors.crust);

            visuals.widgets.noninteractive.bg_fill = c(colors.surface0);
            visuals.widgets.noninteractive.fg_stroke.color = c(colors.text);
            visuals.widgets.noninteractive.bg_stroke.color = c(colors.surface1);

            visuals.widgets.inactive.bg_fill = c(colors.surface0); // Button normal
            visuals.widgets.inactive.fg_stroke.color = c(colors.text);

            visuals.widgets.hovered.bg_fill = c(colors.surface2);
            visuals.widgets.hovered.fg_stroke.color = c(colors.text);

            visuals.widgets.active.bg_fill = c(colors.surface1); // Pressed
            visuals.widgets.active.fg_stroke.color = c(colors.text);

            visuals.widgets.open.bg_fill = c(colors.surface1);

            visuals.selection.bg_fill = c(colors.blue);
            visuals.selection.stroke.color = c(colors.base); // Contrast text on selection? usually white or base

            visuals.hyperlink_color = c(colors.rosewater);
            visuals.warn_fg_color = c(colors.yellow);
            visuals.error_fg_color = c(colors.red);

            visuals.window_stroke.color = c(colors.overlay1);

            ctx.set_visuals(visuals);
        }
    }
}
