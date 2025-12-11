use egui::Context;
use log::warn;
use std::fs;

pub fn setup_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    // Windows specific font path for MS Gothic
    let font_path = "C:\\Windows\\Fonts\\msgothic.ttc";

    if let Ok(font_data) = fs::read(font_path) {
        fonts.font_data.insert(
            "my_font".to_owned(),
            egui::FontData::from_owned(font_data)
                .tweak(egui::FontTweak {
                    scale: 1.2,
                    ..Default::default()
                })
                .into(),
        );

        // Add my_font to the proportional and monospace families
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "my_font".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "my_font".to_owned());

        ctx.set_fonts(fonts);
    } else {
        warn!("Warning: Failed to load font from {}", font_path);
        // Fallback to default egui fonts if MS Gothic fails to load
        ctx.set_fonts(fonts);
    }
}
