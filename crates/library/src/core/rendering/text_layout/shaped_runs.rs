use skia_safe::textlayout::Paragraph;
use skia_safe::{Font, GlyphId, Point, Rect};

use crate::model::frame::runtime_shape::RuntimeTextShape;

use super::{build_text_paragraph, runtime_text_shape_from_paragraph};

/// Render-local text resolved by one SkParagraph layout.
///
/// The semantic elements and drawable glyph runs deliberately come from the
/// same Paragraph. This keeps font fallback, shaping, BiDi ordering, glyph
/// offsets, and line origins identical for plain and Ensemble rendering.
#[derive(Clone, Debug)]
pub(crate) struct ShapedTextLayout {
    pub(crate) metadata: RuntimeTextShape,
    pub(crate) runs: Vec<ShapedGlyphRun>,
}

/// One drawable run copied from SkParagraph's actual TextBlob cache.
#[derive(Clone, Debug)]
pub(crate) struct ShapedGlyphRun {
    pub(crate) font: Font,
    pub(crate) origin: Point,
    pub(crate) glyphs: Vec<GlyphId>,
    pub(crate) positions: Vec<Point>,
    /// Bounds relative to each glyph position. The run origin is not included.
    pub(crate) bounds: Vec<Rect>,
    /// UTF-8 source starts for each glyph plus SkParagraph's terminal entry.
    pub(crate) source_starts: Vec<u32>,
}

impl ShapedTextLayout {
    pub(crate) fn new(text: &str, primary_font_name: &str, size: f32) -> Self {
        let mut paragraph = build_text_paragraph(text, primary_font_name, size, None);
        let runs = extract_shaped_runs(&mut paragraph);
        let source_starts = distinct_glyph_source_starts(&runs);
        let metadata = runtime_text_shape_from_paragraph(
            &paragraph,
            text,
            primary_font_name,
            size,
            &source_starts,
        );
        Self { metadata, runs }
    }
}

pub(super) fn extract_shaped_runs(paragraph: &mut Paragraph) -> Vec<ShapedGlyphRun> {
    let mut runs = Vec::new();
    paragraph.visit(|_, info| {
        let Some(info) = info else {
            return;
        };
        let glyphs = info.glyphs().to_vec();
        if glyphs.is_empty() {
            return;
        }
        let font = info.font().clone();
        let positions = info.positions().to_vec();
        let mut bounds = vec![Rect::new_empty(); glyphs.len()];
        font.get_bounds(&glyphs, &mut bounds, None);
        runs.push(ShapedGlyphRun {
            font,
            origin: info.origin(),
            glyphs,
            positions,
            bounds,
            source_starts: info.utf8_starts().to_vec(),
        });
    });
    runs
}

fn distinct_glyph_source_starts(runs: &[ShapedGlyphRun]) -> Vec<usize> {
    let mut starts = runs
        .iter()
        .flat_map(|run| run.source_starts.iter().take(run.glyphs.len()).copied())
        .map(|start| start as usize)
        .collect::<Vec<_>>();
    starts.sort_unstable();
    starts.dedup();
    starts
}

#[cfg(test)]
mod tests {
    use super::{ShapedTextLayout, distinct_glyph_source_starts, extract_shaped_runs};
    use crate::core::rendering::text_layout::{
        build_text_paragraph, runtime_text_shape_from_paragraph,
    };

    #[test]
    fn visited_runs_retain_exact_glyph_parallel_arrays_and_utf8_starts() {
        let text = "A\u{65e5}e\u{301} abc \u{5d0}\u{5d1}\u{5d2}";
        let layout = ShapedTextLayout::new(text, "Arial", 36.0);

        assert!(!layout.runs.is_empty());
        for run in &layout.runs {
            assert_eq!(run.positions.len(), run.glyphs.len());
            assert_eq!(run.bounds.len(), run.glyphs.len());
            assert_eq!(run.source_starts.len(), run.glyphs.len() + 1);
            assert!(
                run.source_starts
                    .iter()
                    .take(run.glyphs.len())
                    .all(|start| (*start as usize) < text.len()
                        && text.is_char_boundary(*start as usize))
            );
        }
    }

    #[test]
    fn runtime_elements_never_split_an_authoritative_shaping_cluster() {
        let text = "office A\u{65e5} e\u{301} \u{644}\u{627} \u{5d0}\u{5d1}";
        let mut paragraph = build_text_paragraph(text, "Arial", 40.0, None);
        let runs = extract_shaped_runs(&mut paragraph);
        let starts = distinct_glyph_source_starts(&runs);
        let metadata = runtime_text_shape_from_paragraph(&paragraph, text, "Arial", 40.0, &starts);

        for start in starts {
            let Some(cluster) = paragraph.get_glyph_cluster_at(start) else {
                continue;
            };
            let owners = metadata
                .elements
                .iter()
                .filter(|element| {
                    element.utf8_range.start < cluster.text_range.end
                        && cluster.text_range.start < element.utf8_range.end
                })
                .collect::<Vec<_>>();
            assert_eq!(
                owners.len(),
                1,
                "cluster {:?} was split across runtime elements",
                cluster.text_range
            );
            assert!(owners[0].utf8_range.start <= cluster.text_range.start);
            assert!(owners[0].utf8_range.end >= cluster.text_range.end);
        }
    }
}
