#[cfg(target_os = "windows")]
use std::sync::Arc;

use super::*;

#[cfg(target_os = "windows")]
const TEST_FONT_SIZE: f32 = 14.0;

#[cfg(target_os = "windows")]
fn layout(definitions: egui::FontDefinitions, text: &str) -> Arc<egui::Galley> {
    let context = egui::Context::default();
    context.set_fonts(definitions);
    let mut result = None;
    drop(context.run(egui::RawInput::default(), |context| {
        result = Some(context.fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                text.to_owned(),
                egui::FontId::new(TEST_FONT_SIZE, egui::FontFamily::Proportional),
                egui::Color32::WHITE,
            )
        }));
    }));
    result.expect("the egui pass should lay out the requested text")
}

#[cfg(target_os = "windows")]
fn assert_visible_glyph(definitions: egui::FontDefinitions, text: &str, character: char) {
    let galley = layout(definitions, text);
    let glyph = galley
        .rows
        .iter()
        .flat_map(|row| &row.glyphs)
        .find(|glyph| glyph.chr == character)
        .unwrap_or_else(|| {
            panic!(
                "layout omitted {character:?} (U+{:04X})",
                u32::from(character)
            )
        });
    assert!(
        !glyph.uv_rect.is_nothing() && glyph.uv_rect.size.x > 0.0 && glyph.uv_rect.size.y > 0.0,
        "{character:?} resolved without visible atlas geometry"
    );
    assert!(
        galley.num_vertices > 0
            && galley
                .rows
                .iter()
                .any(|row| !row.visuals.mesh.vertices.is_empty()),
        "{text:?} produced no visible Galley mesh"
    );
}

// This verifies the real system-font chain used by the supported Windows build.
// Cross-platform CI still exercises the platform-independent ordering test below.
#[cfg(target_os = "windows")]
#[test]
fn windows_production_families_have_real_glyphs_for_multilingual_authoring() {
    let context = egui::Context::default();
    install(&context);
    drop(context.run(egui::RawInput::default(), |context| {
        context.fonts_mut(|fonts| {
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                let font = egui::FontId::new(TEST_FONT_SIZE, family);
                for sample in [
                    "ABC \u{65e5}\u{672c}\u{8a9e}",
                    "\u{0395}\u{03bb}\u{03bb}\u{03b7}\u{03bd}\u{03b9}\u{03ba}\u{03ac} \u{041a}\u{0438}\u{0440}\u{0438}\u{043b}\u{043b}\u{0438}\u{0446}\u{0430}",
                    "\u{05e2}\u{05d1}\u{05e8}\u{05d9}\u{05ea}",
                    "\u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629}",
                    "\u{1f642}",
                ] {
                    for character in sample.chars().filter(|character| !character.is_whitespace()) {
                        assert!(
                            fonts.has_glyph(&font, character),
                            "{:?} uses a replacement glyph for {character:?} (U+{:04X})",
                            font.family,
                            u32::from(character),
                        );
                    }
                }
            }
        });
    }));
}

#[cfg(target_os = "windows")]
#[test]
fn windows_selected_fallback_coverage_produces_visible_galley_meshes() {
    for (text, character) in [
        ("\u{3042}", '\u{3042}'),
        ("\u{05d0}", '\u{05d0}'),
        ("\u{05d0}\u{05b0}", '\u{05b0}'),
        ("\u{0633}", '\u{0633}'),
        ("\u{0633}\u{064e}", '\u{064e}'),
        ("\u{1f642}", '\u{1f642}'),
    ] {
        assert_visible_glyph(definitions(), text, character);
    }
}

#[test]
fn fallback_chain_is_ordered_bounded_and_contains_no_duplicate_face_data() {
    let mut baseline = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut baseline, egui_phosphor::Variant::Regular);
    let actual = definitions();
    let fallback_names = actual
        .families
        .get(&egui::FontFamily::Proportional)
        .expect("production proportional family")
        .iter()
        .filter(|name| name.starts_with("system_fallback:"))
        .cloned()
        .collect::<Vec<_>>();

    assert!(fallback_names.len() <= FALLBACK_COVERAGE.len());
    assert_eq!(
        fallback_names.len(),
        actual
            .font_data
            .keys()
            .filter(|name| name.starts_with("system_fallback:"))
            .count()
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let actual_order = actual.families.get(&family).expect("production family");
        let baseline_order = baseline.families.get(&family).expect("baseline family");
        let bundled_order = actual_order
            .iter()
            .filter(|name| *name != "ui_font" && !name.starts_with("system_fallback:"))
            .collect::<Vec<_>>();
        assert_eq!(bundled_order, baseline_order.iter().collect::<Vec<_>>());
        assert_eq!(
            actual_order
                .iter()
                .filter(|name| name.starts_with("system_fallback:"))
                .collect::<Vec<_>>(),
            fallback_names.iter().collect::<Vec<_>>()
        );
        let mut expected_order = Vec::new();
        if actual.font_data.contains_key("ui_font") {
            expected_order.push("ui_font".to_owned());
        }
        expected_order.extend(baseline_order.iter().cloned());
        expected_order.extend(fallback_names.iter().cloned());
        assert_eq!(actual_order, &expected_order);
        for name in actual.font_data.keys() {
            assert!(
                actual_order.iter().filter(|entry| *entry == name).count() <= 1,
                "{name} was inserted more than once into {family:?}"
            );
        }
    }

    let selected_names = actual
        .font_data
        .iter()
        .filter(|(name, _)| *name == "ui_font" || name.starts_with("system_fallback:"))
        .collect::<Vec<_>>();
    for (index, (left_name, left)) in selected_names.iter().enumerate() {
        for (right_name, right) in &selected_names[index + 1..] {
            assert!(
                left.index != right.index || left.font.as_ref() != right.font.as_ref(),
                "the same face data was loaded as both {left_name} and {right_name}"
            );
        }
    }
}

#[cfg(target_os = "windows")]
#[test]
fn windows_ms_gothic_index_zero_preserves_the_previous_layout_and_icon_chain() {
    let stock_bytes = std::fs::read(r"C:\Windows\Fonts\msgothic.ttc")
        .expect("the Windows MS Gothic collection should be installed");

    let actual = definitions();
    let actual_primary = actual
        .font_data
        .get("ui_font")
        .expect("the production resolver should expose Windows MS Gothic as the primary UI font");
    assert_eq!(actual_primary.index, 0);
    assert_eq!(actual_primary.tweak.scale, 1.2);
    assert_eq!(actual_primary.font.as_ref(), stock_bytes.as_slice());

    let mut previous = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut previous, egui_phosphor::Variant::Regular);
    let previous_font = egui::FontData::from_owned(stock_bytes).tweak(egui::FontTweak {
        scale: 1.2,
        ..Default::default()
    });
    previous
        .font_data
        .insert("ui_font".to_owned(), previous_font.into());
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        previous
            .families
            .entry(family)
            .or_default()
            .insert(0, "ui_font".to_owned());
    }

    let sample = format!(
        "ABC \u{65e5}\u{672c}\u{8a9e} {}",
        egui_phosphor::regular::TIMER
    );
    assert_eq!(layout(actual, &sample), layout(previous, &sample));
}

#[cfg(target_os = "windows")]
#[test]
fn installed_collection_face_index_is_preserved_when_nonzero() {
    let manager = FontMgr::default();
    let stock_bytes = std::fs::read(r"C:\Windows\Fonts\msgothic.ttc")
        .expect("the Windows MS Gothic collection should be installed");
    let (face, provider_index) = (1..=8)
        .filter_map(|index| {
            manager
                .new_from_data(&stock_bytes, index)
                .map(|face| (face, index))
        })
        .find_map(|(face, requested_index)| {
            ["MS PGothic", "MS UI Gothic"]
                .contains(&face.family_name().as_str())
                .then(|| {
                    let (_, provider_index) = face
                        .to_font_data()
                        .expect("the indexed collection face should retain its source data");
                    assert_eq!(provider_index, requested_index);
                    (face, provider_index)
                })
        })
        .expect("msgothic.ttc should expose MS PGothic or MS UI Gothic at a nonzero face index");
    assert_ne!(provider_index, 0);

    let data = font_data(&face, 1.0).expect("egui should accept the installed collection face");
    assert_eq!(
        data.index,
        u32::try_from(provider_index).expect("non-negative face index")
    );
}
