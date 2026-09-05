use skia_safe::textlayout::{
    FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, RectHeightStyle, RectWidthStyle,
    TextStyle,
};
use skia_safe::{FontMgr, Paint, Rect};
use unicode_segmentation::UnicodeSegmentation;

use crate::model::frame::entity::StyleConfig;
use crate::model::frame::runtime_shape::{
    RuntimeBounds, RuntimeLine, RuntimeTextElement, RuntimeTextShape,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLayoutMetrics {
    pub width: f32,
    pub height: f32,
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
        let style_outset = config.style.visual_outset();
        outset.max(style_outset)
    })
}

/// Resolve render-only Unicode grapheme metadata from the same Paragraph used
/// by normal text painting. Glyph IDs/outlines remain SkParagraph-owned: this
/// function does not pretend that a grapheme and a shaped glyph are 1:1.
pub(crate) fn layout_runtime_text_shape(
    text: &str,
    primary_font_name: &str,
    size: f32,
) -> RuntimeTextShape {
    let paragraph = build_text_paragraph(text, primary_font_name, size, None);
    let lines = paragraph.get_line_metrics();
    #[derive(Clone)]
    struct ElementSource<'a> {
        source: &'a str,
        utf8_range: std::ops::Range<usize>,
        utf16_range: std::ops::Range<usize>,
        line_index: usize,
        line_element_index: usize,
    }

    #[derive(Clone)]
    struct LineSource {
        utf8_range: std::ops::Range<usize>,
        utf16_range: std::ops::Range<usize>,
    }

    let mut sources = Vec::new();
    let mut line_sources = Vec::new();
    let mut line_index = 0_usize;
    let mut line_element_index = 0_usize;
    let mut utf16_index = 0_usize;
    let mut line_utf8_start = 0_usize;
    let mut line_utf16_start = 0_usize;
    for (utf8_start, grapheme) in text.grapheme_indices(true) {
        let utf8_end = utf8_start + grapheme.len();
        let utf16_start = utf16_index;
        let utf16_end = utf16_start + grapheme.encode_utf16().count();
        utf16_index = utf16_end;
        if grapheme.contains('\n') {
            line_sources.push(LineSource {
                utf8_range: line_utf8_start..utf8_start,
                utf16_range: line_utf16_start..utf16_start,
            });
            line_index += 1;
            line_element_index = 0;
            line_utf8_start = utf8_end;
            line_utf16_start = utf16_end;
            continue;
        }
        sources.push(ElementSource {
            source: grapheme,
            utf8_range: utf8_start..utf8_end,
            utf16_range: utf16_start..utf16_end,
            line_index,
            line_element_index,
        });
        line_element_index += 1;
    }
    line_sources.push(LineSource {
        utf8_range: line_utf8_start..text.len(),
        utf16_range: line_utf16_start..utf16_index,
    });

    let block_group_id = stable_text_group_id(
        0x42,
        0..text.len(),
        0..utf16_index,
        line_sources.len(),
        sources.len(),
    );
    let line_group_ids = line_sources
        .iter()
        .enumerate()
        .map(|(index, line)| {
            stable_text_group_id(
                0x4c,
                line.utf8_range.clone(),
                line.utf16_range.clone(),
                index,
                block_group_id as usize,
            )
        })
        .collect::<Vec<_>>();

    let mut elements = Vec::with_capacity(sources.len());
    for (block_element_index, source) in sources.into_iter().enumerate() {
        let Some(line) = lines
            .iter()
            .find(|line| line.line_number == source.line_index)
            .or_else(|| lines.get(source.line_index))
        else {
            continue;
        };
        let boxes = paragraph.get_rects_for_range(
            source.utf16_range.clone(),
            RectHeightStyle::Max,
            RectWidthStyle::Tight,
        );
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
        let (left, right) = rect
            .map(|rect| (rect.left, rect.right))
            .unwrap_or((line.left as f32, line.left as f32));
        let baseline = line.baseline as f32;
        let top = (baseline - line.ascent as f32).max(0.0);
        let bottom = (baseline + line.descent as f32).min(paragraph.height());
        let line_group_id = line_group_ids
            .get(source.line_index)
            .copied()
            .unwrap_or(block_group_id);
        let element_group_id = stable_text_group_id(
            0x43,
            source.utf8_range.clone(),
            source.utf16_range.clone(),
            source.line_index,
            source.line_element_index,
        );
        elements.push(RuntimeTextElement {
            source: source.source.to_string(),
            utf8_range: source.utf8_range,
            utf16_range: source.utf16_range,
            line_index: source.line_index,
            line_element_index: source.line_element_index,
            block_element_index,
            block_group_id,
            line_group_id,
            element_group_id,
            bounds: RuntimeBounds::new(left, top, right, bottom),
            advance: (right - left).max(0.0),
            baseline,
        });
    }

    let runtime_lines = line_sources
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            let element_start = elements
                .iter()
                .position(|element| element.line_index == index)
                .unwrap_or_else(|| {
                    elements
                        .iter()
                        .take_while(|element| element.line_index < index)
                        .count()
                });
            let element_end = element_start
                + elements
                    .iter()
                    .filter(|element| element.line_index == index)
                    .count();
            let bounds = elements[element_start..element_end]
                .iter()
                .map(|element| element.bounds)
                .reduce(RuntimeBounds::union)
                .unwrap_or_default();
            RuntimeLine {
                index,
                element_range: element_start..element_end,
                utf8_range: source.utf8_range,
                utf16_range: source.utf16_range,
                group_id: line_group_ids.get(index).copied().unwrap_or(block_group_id),
                bounds,
            }
        })
        .collect::<Vec<_>>();
    let block_bounds = elements
        .iter()
        .map(|element| element.bounds)
        .reduce(RuntimeBounds::union)
        .unwrap_or_default();

    RuntimeTextShape {
        text: text.to_string(),
        font: primary_font_name.to_string(),
        size: f64::from(size),
        elements,
        lines: runtime_lines,
        block_group_id,
        block_bounds,
    }
}

fn stable_text_group_id(
    kind: u8,
    utf8_range: std::ops::Range<usize>,
    utf16_range: std::ops::Range<usize>,
    group_index: usize,
    element_index: usize,
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in [
        u64::from(kind),
        utf8_range.start as u64,
        utf8_range.end as u64,
        utf16_range.start as u64,
        utf16_range.end as u64,
        group_index as u64,
        element_index as u64,
    ]
    .into_iter()
    .flat_map(u64::to_le_bytes)
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{layout_runtime_text_shape, measure_text_layout};

    #[test]
    fn paragraph_metrics_and_character_layout_share_multiline_baselines() {
        let metrics = measure_text_layout("Ag\nTy", "Arial", 42.0);
        let shape = layout_runtime_text_shape("Ag\nTy", "Arial", 42.0);
        let elements = &shape.elements;

        assert!(metrics.width > 20.0);
        assert!(metrics.height > 60.0);
        assert_eq!(elements.len(), 4);
        assert_eq!(elements[0].line_index, 0);
        assert_eq!(elements[2].line_index, 1);
        assert!(elements[2].baseline > elements[0].baseline);
        assert!(elements.iter().all(|element| {
            element.bounds.top >= -0.01 && element.bounds.bottom <= metrics.height + 0.01
        }));
        assert_eq!(shape.lines.len(), 2);
        assert_eq!(shape.lines[0].element_range, 0..2);
        assert_eq!(shape.lines[1].element_range, 2..4);
    }

    #[test]
    fn grapheme_source_ranges_support_multibyte_and_combining_text() {
        let shape = layout_runtime_text_shape("A日e\u{301}", "Arial", 36.0);
        assert_eq!(
            shape
                .elements
                .iter()
                .map(|element| element.source.as_str())
                .collect::<String>(),
            "A日e\u{301}"
        );
        assert_eq!(shape.elements.len(), 3);
        assert_eq!(shape.elements[2].source.chars().count(), 2);
        assert_eq!(shape.elements[2].utf8_range, 4..7);
        assert_eq!(shape.elements[2].utf16_range, 2..4);
        assert!(shape.elements.iter().all(|element| element.advance > 0.0));
        let clone = shape.clone();
        assert_eq!(
            clone.elements[2].element_group_id,
            shape.elements[2].element_group_id
        );
    }
}
