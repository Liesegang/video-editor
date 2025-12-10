

// pub fn setup_fonts(ctx: &Context) {
//     let mut fonts = egui::FontDefinitions::default();

//     // Windows specific font path for MS Gothic
//     let font_path = "C:\\Windows\\Fonts\\msgothic.ttc";

//     if let Ok(font_data) = fs::read(font_path) {
//         fonts.font_data.insert(
//             "my_font".to_owned(),
//             egui::FontData::from_owned(font_data).tweak(egui::FontTweak {
//                 scale: 1.2,
//                 ..Default::default()
//             }),
//         );

//         fonts
//             .families
//             .entry(egui::FontFamily::Proportional)
//             .or_default()
//             .insert(0, "my_font".to_owned());
//         fonts
//             .families
//             .entry(egui::FontFamily::Monospace)
//             .or_default()
//             .insert(0, "my_font".to_owned());

//         ctx.set_fonts(fonts);
//     } else {
//         eprintln!("Warning: Failed to load font from {}", font_path);
//     }
// }
pub fn setup_fonts(_ctx: &egui::Context) {
    // Rely on default egui fonts for now.
    // Custom font loading is commented out to troubleshoot garbled characters.
}
