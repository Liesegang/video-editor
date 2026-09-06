//! One ordered font configuration for every production egui editor surface.

use eframe::egui;
use egui::epaint::text::FontPriority;
use skia_safe::{FontMgr, FontStyle, Typeface};

const PRIMARY_FAMILY: &str = "MS Gothic";
// A bounded set of OS fallback requests, not a scan of every installed font.
// Marks can resolve to a different face from their script's base letters.
const FALLBACK_COVERAGE: &[(&str, char)] = &[
    ("ja", 'あ'),
    ("he", 'א'),
    ("he", '\u{05b0}'),
    ("ar", 'س'),
    ("ar", '\u{064e}'),
    ("und-Zsye", '🙂'),
];

pub(crate) fn install(context: &egui::Context) {
    context.set_fonts(definitions());
}

fn definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let manager = FontMgr::default();
    let mut loaded_faces = std::collections::HashSet::new();
    if let Some(face) = manager.match_family_style(PRIMARY_FAMILY, FontStyle::normal()) {
        if let Some(data) = font_data(&face, 1.2) {
            register_font(&mut fonts, "ui_font", data, FontPriority::Highest);
            loaded_faces.insert(face.unique_id());
        }
    }
    if !fonts.font_data.contains_key("ui_font") {
        log::warn!("System {PRIMARY_FAMILY} is unavailable; using the bundled UI font");
    }
    for &(language, character) in FALLBACK_COVERAGE {
        let Some(face) = manager.match_family_style_character(
            PRIMARY_FAMILY,
            FontStyle::normal(),
            &[language],
            character as i32,
        ) else {
            log::warn!(
                "No system UI fallback for {language} U+{:04X}",
                u32::from(character)
            );
            continue;
        };
        if loaded_faces.contains(&face.unique_id()) {
            continue;
        }
        let Some(data) = font_data(&face, 1.0) else {
            continue;
        };
        register_font(
            &mut fonts,
            &format!("system_fallback:{}", face.unique_id()),
            data,
            FontPriority::Lowest,
        );
        loaded_faces.insert(face.unique_id());
    }
    fonts
}

fn font_data(face: &Typeface, scale: f32) -> Option<egui::FontData> {
    let (bytes, index) = face.to_font_data()?;
    let index = u32::try_from(index).ok()?;
    // Validate with egui's existing parser before installing external OS data.
    if ab_glyph::FontRef::try_from_slice_and_index(&bytes, index).is_err() {
        log::warn!(
            "System font {} cannot be read by the UI font parser",
            face.family_name()
        );
        return None;
    }
    Some(egui::FontData {
        font: bytes.into(),
        index,
        tweak: egui::FontTweak {
            scale,
            ..Default::default()
        },
    })
}

fn register_font(
    fonts: &mut egui::FontDefinitions,
    name: &str,
    data: egui::FontData,
    priority: FontPriority,
) {
    fonts.font_data.insert(name.to_owned(), data.into());
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let ordered = fonts.families.entry(family).or_default();
        match priority {
            FontPriority::Highest => ordered.insert(0, name.to_owned()),
            FontPriority::Lowest => ordered.push(name.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests;
