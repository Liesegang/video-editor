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

mod shaped_runs;

pub(crate) use shaped_runs::ShapedTextLayout;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLayoutMetrics {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone)]
struct ElementSource {
    source: String,
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
    crate::model::frame::appearance::appearance_outsets(styles).visual
}

/// Resolve render-only text-element metadata from the production shaping owner.
///
/// Elements start at Unicode grapheme boundaries, then adjacent graphemes
/// crossed by one SkParagraph shaping cluster are kept atomic. This preserves
/// contextual shaping without pretending that a grapheme and a glyph are 1:1.
pub(crate) fn layout_runtime_text_shape(
    text: &str,
    primary_font_name: &str,
    size: f32,
) -> RuntimeTextShape {
    ShapedTextLayout::new(text, primary_font_name, size).metadata
}

pub(super) fn runtime_text_shape_from_paragraph(
    paragraph: &Paragraph,
    text: &str,
    primary_font_name: &str,
    size: f32,
    glyph_source_starts: &[usize],
) -> RuntimeTextShape {
    let lines = paragraph.get_line_metrics();
    let mut grapheme_sources = Vec::new();
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
        grapheme_sources.push(ElementSource {
            source: grapheme.to_string(),
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

    let cluster_spans = resolved_cluster_spans(paragraph, glyph_source_starts, text);
    let sources = merge_sources_crossed_by_clusters(grapheme_sources, &cluster_spans);

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
            source: source.source,
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

fn resolved_cluster_spans(
    paragraph: &Paragraph,
    glyph_source_starts: &[usize],
    text: &str,
) -> Vec<std::ops::Range<usize>> {
    let mut spans = glyph_source_starts
        .iter()
        .copied()
        .filter(|start| *start < text.len() && text.is_char_boundary(*start))
        .filter_map(|start| paragraph.get_glyph_cluster_at(start))
        .map(|cluster| cluster.text_range.start..cluster.text_range.end)
        .filter(|span| {
            span.start < span.end
                && span.end <= text.len()
                && text.is_char_boundary(span.start)
                && text.is_char_boundary(span.end)
        })
        .collect::<Vec<_>>();
    spans.sort_unstable_by_key(|span| (span.start, span.end));
    spans.dedup();

    // A fallback run may contribute another glyph slice for the same logical
    // cluster. Coalesce overlap, but never merge merely adjacent clusters.
    let mut disjoint: Vec<std::ops::Range<usize>> = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(last) = disjoint.last_mut()
            && span.start < last.end
        {
            last.end = last.end.max(span.end);
            continue;
        }
        disjoint.push(span);
    }
    disjoint
}

fn merge_sources_crossed_by_clusters(
    mut sources: Vec<ElementSource>,
    cluster_spans: &[std::ops::Range<usize>],
) -> Vec<ElementSource> {
    let mut merged = Vec::with_capacity(sources.len());
    let mut source_index = 0_usize;
    let mut cluster_index = 0_usize;
    let mut line_index = usize::MAX;
    let mut line_element_index = 0_usize;

    while source_index < sources.len() {
        let first = &sources[source_index];
        while cluster_index < cluster_spans.len()
            && cluster_spans[cluster_index].end <= first.utf8_range.start
        {
            cluster_index += 1;
        }

        let component_start = first.utf8_range.start;
        let mut component_end = first.utf8_range.end;
        let mut last_index = source_index;
        let mut span_index = cluster_index;
        loop {
            let previous_end = component_end;
            while let Some(span) = cluster_spans.get(span_index) {
                if span.start >= component_end {
                    break;
                }
                if component_start < span.end {
                    component_end = component_end.max(span.end);
                }
                span_index += 1;
            }
            while let Some(next) = sources.get(last_index + 1) {
                if next.line_index != first.line_index || next.utf8_range.start >= component_end {
                    break;
                }
                last_index += 1;
                component_end = component_end.max(next.utf8_range.end);
            }
            if component_end == previous_end {
                break;
            }
        }
        cluster_index = span_index;
        let last = &sources[last_index];
        if line_index != first.line_index {
            line_index = first.line_index;
            line_element_index = 0;
        }
        let utf8_range = first.utf8_range.start..last.utf8_range.end;
        let utf16_range = first.utf16_range.start..last.utf16_range.end;
        let source_line_index = first.line_index;
        // Most elements are already atomic. Move their owned source into the
        // result instead of allocating a second String for every grapheme.
        let mut source = std::mem::take(&mut sources[source_index].source);
        for next in &sources[source_index + 1..last_index + 1] {
            source.push_str(&next.source);
        }
        merged.push(ElementSource {
            source,
            utf8_range,
            utf16_range,
            line_index: source_line_index,
            line_element_index,
        });
        line_element_index += 1;
        source_index = last_index + 1;
    }

    merged
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
    use super::{
        ElementSource, layout_runtime_text_shape, measure_text_layout,
        merge_sources_crossed_by_clusters,
    };

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

    #[test]
    fn cluster_and_grapheme_overlaps_form_one_transitive_atomic_component() {
        let text = "abcdef";
        let sources = vec![
            ElementSource {
                source: "abc".to_string(),
                utf8_range: 0..3,
                utf16_range: 0..3,
                line_index: 0,
                line_element_index: 0,
            },
            ElementSource {
                source: "de".to_string(),
                utf8_range: 3..5,
                utf16_range: 3..5,
                line_index: 0,
                line_element_index: 1,
            },
            ElementSource {
                source: "f".to_string(),
                utf8_range: 5..6,
                utf16_range: 5..6,
                line_index: 0,
                line_element_index: 2,
            },
        ];
        let clusters = vec![0..1, 1..4, 4..6];

        let merged = merge_sources_crossed_by_clusters(sources, &clusters);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source, text);
        assert_eq!(merged[0].utf8_range, 0..6);
        assert_eq!(merged[0].utf16_range, 0..6);
        assert_eq!(merged[0].line_element_index, 0);
    }
}
