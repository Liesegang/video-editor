use skia_safe::textlayout::{
    FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, RectHeightStyle, RectWidthStyle,
    TextStyle,
};
use skia_safe::{FontMgr, Paint, Rect};

use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::StyleConfig;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLayoutMetrics {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextCharacterLayout {
    pub value: char,
    pub x: f32,
    pub baseline: f32,
    pub advance: f32,
    pub top: f32,
    pub bottom: f32,
    pub line_index: usize,
    pub line_char_index: usize,
    pub line_char_count: usize,
}

/// Build the Paragraph used by both measurement and standard text painting.
/// Keeping this in one place prevents Preview bounds and rendered line layout
/// from silently drifting apart as typography settings evolve.
pub(crate) fn build_text_paragraph(
    text: &str,
    primary_font_name: &str,
    size: f32,
    foreground: Option<&Paint>,
) -> Paragraph {
    let mut font_collection = FontCollection::new();
    font_collection.set_default_font_manager(FontMgr::default(), None);

    let mut text_style = TextStyle::new();
    text_style.set_font_families(&[primary_font_name]);
    text_style.set_font_size(size.max(1.0));
    if let Some(foreground) = foreground {
        text_style.set_foreground_paint(foreground);
    }

    let mut paragraph_style = ParagraphStyle::new();
    paragraph_style.set_text_style(&text_style);
    let mut builder = ParagraphBuilder::new(&paragraph_style, font_collection);
    builder.add_text(text);

    let mut paragraph = builder.build();
    paragraph.layout(f32::MAX);
    paragraph
}

pub fn measure_text_layout(text: &str, primary_font_name: &str, size: f32) -> TextLayoutMetrics {
    let paragraph = build_text_paragraph(text, primary_font_name, size, None);
    TextLayoutMetrics {
        width: paragraph.max_intrinsic_width(),
        height: paragraph.height(),
    }
}

/// The authored style stack can paint outside the Paragraph's logical box.
/// Return the largest symmetric expansion used by the actual text paints.
pub fn text_style_outset(styles: &[StyleConfig]) -> f32 {
    styles.iter().fold(0.0_f32, |outset, config| {
        let style_outset = match &config.style {
            DrawStyle::Fill { offset, .. } => offset.max(0.0) as f32,
            DrawStyle::Stroke { width, offset, .. } => (width / 2.0 + offset).max(0.0) as f32,
        };
        outset.max(style_outset)
    })
}

/// Resolve per-character positions from the same shaped Paragraph used by the
/// normal renderer. Ensemble still paints each character independently so it
/// can transform it, but no longer invents a separate line height or advance.
pub(crate) fn layout_text_characters(
    text: &str,
    primary_font_name: &str,
    size: f32,
) -> Vec<TextCharacterLayout> {
    let paragraph = build_text_paragraph(text, primary_font_name, size, None);
    let lines = paragraph.get_line_metrics();
    let line_char_counts = text
        .split('\n')
        .map(|line| line.chars().count())
        .collect::<Vec<_>>();
    let mut line_index = 0_usize;
    let mut line_char_index = 0_usize;
    let mut characters = Vec::new();
    let mut utf16_index = 0_usize;

    for value in text.chars() {
        // SkParagraph exposes text selection positions as UTF-16 code units,
        // matching its cross-platform text API rather than Rust UTF-8 bytes.
        let start = utf16_index;
        let end = start + value.len_utf16();
        utf16_index = end;
        if value == '\n' {
            line_index += 1;
            line_char_index = 0;
            continue;
        }

        let Some(line) = lines
            .iter()
            .find(|line| line.line_number == line_index)
            .or_else(|| lines.get(line_index))
        else {
            continue;
        };
        let boxes =
            paragraph.get_rects_for_range(start..end, RectHeightStyle::Max, RectWidthStyle::Tight);
        let rect = boxes.iter().fold(None, |bounds: Option<Rect>, text_box| {
            Some(match bounds {
                Some(bounds) => Rect::new(
                    bounds.left.min(text_box.rect.left),
                    bounds.top.min(text_box.rect.top),
                    bounds.right.max(text_box.rect.right),
                    bounds.bottom.max(text_box.rect.bottom),
                ),
                None => text_box.rect,
            })
        });

        // Selection boxes are present for visible glyphs and whitespace. A
        // zero-width control character may not have one; keeping it at the
        // line origin is safer than fabricating visible geometry.
        let (x, advance) = rect
            .map(|rect| (rect.left, rect.width().max(0.0)))
            .unwrap_or((line.left as f32, 0.0));
        let baseline = line.baseline as f32;
        let top = (baseline - line.ascent as f32).max(0.0);
        let bottom = (baseline + line.descent as f32).min(paragraph.height());
        characters.push(TextCharacterLayout {
            value,
            x,
            baseline,
            advance,
            top,
            bottom,
            line_index,
            line_char_index,
            line_char_count: line_char_counts
                .get(line_index)
                .copied()
                .unwrap_or_default(),
        });
        line_char_index += 1;
    }

    characters
}

#[cfg(test)]
mod tests {
    use super::{layout_text_characters, measure_text_layout};

    #[test]
    fn paragraph_metrics_and_character_layout_share_multiline_baselines() {
        let metrics = measure_text_layout("Ag\nTy", "Arial", 42.0);
        let characters = layout_text_characters("Ag\nTy", "Arial", 42.0);

        assert!(metrics.width > 20.0);
        assert!(metrics.height > 60.0);
        assert_eq!(characters.len(), 4);
        assert_eq!(characters[0].line_index, 0);
        assert_eq!(characters[2].line_index, 1);
        assert!(characters[2].baseline > characters[0].baseline);
        assert!(characters.iter().all(|character| {
            character.top >= -0.01 && character.bottom <= metrics.height + 0.01
        }));
    }

    #[test]
    fn character_ranges_support_multibyte_text() {
        let characters = layout_text_characters("A日é", "Arial", 36.0);
        assert_eq!(
            characters
                .iter()
                .map(|character| character.value)
                .collect::<String>(),
            "A日é"
        );
        assert!(characters.iter().all(|character| character.advance > 0.0));
    }
}
